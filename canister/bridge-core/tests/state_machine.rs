use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, Account, AccountingState, Amount, ApplyOutcome,
    BaseMintSnapshot, CoreError, DepositEvent, DepositHoldResolution, DepositId, DepositRecord,
    DepositRequest, DepositState, EvmOperationEvent, EvmOperationId, EvmOperationKind,
    EvmOperationRecord, EvmOperationState, EvmRecoveryResolution, FeeKind, HoldId, LedgerOperation,
    LedgerTransferIdentity, ReconciliationHoldRecord, ReconciliationHoldState, RequestReference,
    ReservePolicy, Settlement, TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution,
    WithdrawalId, WithdrawalRecord, WithdrawalState,
};

fn account(tag: u8) -> Account {
    Account::new(vec![tag], [tag; 32]).expect("test principal must be valid")
}

fn transfer(
    operation: LedgerOperation,
    amount: u128,
    fee: u128,
    tag: u8,
) -> LedgerTransferIdentity {
    LedgerTransferIdentity {
        operation,
        created_at_time_ns: u64::from(tag),
        memo: [tag; 32],
        amount: Amount::new(amount),
        fee: Amount::new(fee),
        from: account(tag),
        to: account(tag + 1),
        spender: Some(account(tag + 2)),
    }
}

fn base_snapshot(service_fee: u128) -> BaseMintSnapshot {
    BaseMintSnapshot {
        finalized_head_block_number: 1,
        confirmed_block_timestamp: 1,
        service_fee: Amount::new(service_fee),
        max_service_fee: Amount::new(20),
        per_deposit_limit: Amount::new(1_000),
        mint_window_limit: Amount::new(10_000),
        mint_window_started_at: 0,
        mint_window_duration: 100,
        minted_in_window: Amount::new(100),
    }
}

fn attempt(identity: LedgerTransferIdentity) -> TransferAttempt {
    TransferAttempt {
        attempt_no: 0,
        identity,
    }
}

fn accepted_deposit() -> DepositRecord {
    DepositRecord::accept(
        DepositRequest {
            id: DepositId::new([1; 32]),
            payload_hash: [2; 32],
            gross_amount: Amount::new(110),
            user_max_service_fee: Amount::new(10),
            transfer: transfer(LedgerOperation::PullDeposit, 110, 1, 10),
        },
        base_snapshot(10),
    )
    .expect("valid deposit")
}

#[test]
fn amount_and_quote_boundaries_are_checked() {
    assert_eq!(
        Amount::new(u128::MAX).checked_add(Amount::new(1)),
        Err(CoreError::ArithmeticOverflow)
    );
    assert_eq!(
        Amount::ZERO.checked_sub(Amount::new(1)),
        Err(CoreError::ArithmeticUnderflow)
    );
    assert_eq!(
        base_snapshot(10).quote(Amount::new(110), Amount::new(10)),
        Ok(Amount::new(100))
    );
    assert_eq!(
        base_snapshot(10).quote(Amount::new(9), Amount::new(10)),
        Err(CoreError::ArithmeticUnderflow)
    );

    let mut above_user = base_snapshot(10);
    above_user.service_fee = Amount::new(11);
    assert_eq!(
        above_user.quote(Amount::new(100), Amount::new(10)),
        Err(CoreError::ServiceFeeAboveUserMaximum)
    );

    let mut overflow_window = base_snapshot(0);
    overflow_window.minted_in_window = Amount::new(u128::MAX);
    overflow_window.mint_window_limit = Amount::new(u128::MAX);
    assert_eq!(
        overflow_window.quote(Amount::new(1), Amount::ZERO),
        Err(CoreError::ArithmeticOverflow)
    );

    let mut expired_full_window = base_snapshot(10);
    expired_full_window.minted_in_window = expired_full_window.mint_window_limit;
    expired_full_window.confirmed_block_timestamp =
        expired_full_window.mint_window_started_at + expired_full_window.mint_window_duration;
    assert_eq!(
        expired_full_window.quote(Amount::new(110), Amount::new(10)),
        Ok(Amount::new(100))
    );
}

#[test]
fn deposit_fee_is_confirmed_only_on_first_confirmed_mint() {
    let mut deposit = accepted_deposit();
    assert_eq!(deposit.net_amount, Amount::new(100));
    assert_eq!(deposit.verify_retry([2; 32]), Ok(()));
    assert_eq!(
        deposit.verify_retry([3; 32]),
        Err(CoreError::PayloadConflict)
    );

    let pull = DepositEvent::PullSucceeded {
        ledger_block_index: 42,
    };
    assert_eq!(deposit.apply(pull).expect("pull").fee_delta, Amount::ZERO);
    assert_eq!(
        deposit.apply(pull).expect("replay").outcome,
        ApplyOutcome::Idempotent
    );

    let prepare = DepositEvent::PrepareMint {
        operation_id: EvmOperationId::new(7),
    };
    deposit.apply(prepare).expect("prepare mint");
    assert_eq!(
        deposit.apply(prepare).expect("prepare replay").outcome,
        ApplyOutcome::Idempotent
    );

    let confirmed = DepositEvent::MintConfirmed {
        operation_id: EvmOperationId::new(7),
    };
    assert_eq!(
        deposit.apply(confirmed).expect("mint confirmed").fee_delta,
        Amount::new(10)
    );
    assert_eq!(
        deposit.apply(confirmed).expect("mint replay").fee_delta,
        Amount::ZERO
    );
    assert!(matches!(deposit.state, DepositState::Minted { .. }));
}

#[test]
fn deposit_hold_requires_matching_evidence_resolution() {
    let mut deposit = accepted_deposit();
    let hold_id = HoldId::new(9);
    deposit
        .apply(DepositEvent::PullAmbiguous { hold_id })
        .expect("enter hold");
    assert_eq!(
        deposit.apply(DepositEvent::PrepareMint {
            operation_id: EvmOperationId::new(1),
        }),
        Err(CoreError::InvalidTransition {
            entity: "deposit",
            event: "prepare_mint",
        })
    );
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Deposit(deposit.id),
        deposit.transfer.clone(),
    );
    let mut mismatched = hold.clone();
    mismatched.id = HoldId::new(10);
    assert_eq!(
        resolve_deposit_hold(
            &mut deposit,
            &mut mismatched,
            DepositHoldResolution::Succeeded {
                ledger_block_index: 88,
            },
        ),
        Err(CoreError::HoldMismatch)
    );
    let mut wrong_request = hold.clone();
    wrong_request.request = RequestReference::Withdrawal(WithdrawalId::new([9; 32]));
    assert_eq!(
        resolve_deposit_hold(
            &mut deposit,
            &mut wrong_request,
            DepositHoldResolution::Succeeded {
                ledger_block_index: 88,
            },
        ),
        Err(CoreError::HoldMismatch)
    );
    let mut wrong_transfer = hold.clone();
    wrong_transfer.transfer.memo = [99; 32];
    assert_eq!(
        resolve_deposit_hold(
            &mut deposit,
            &mut wrong_transfer,
            DepositHoldResolution::Succeeded {
                ledger_block_index: 88,
            },
        ),
        Err(CoreError::HoldMismatch)
    );
    resolve_deposit_hold(
        &mut deposit,
        &mut hold,
        DepositHoldResolution::Succeeded {
            ledger_block_index: 88,
        },
    )
    .expect("resolve hold");
    assert_eq!(
        deposit.state,
        DepositState::Escrowed {
            ledger_block_index: 88
        }
    );
}

#[test]
fn definitive_pull_failure_cancels_and_releases_the_deposit_path() {
    let mut deposit = accepted_deposit();
    let failure = bridge_core::LedgerFailure::InsufficientAllowance {
        allowance: Amount::ZERO,
    };
    let event = DepositEvent::PullFailed { code: failure };
    assert_eq!(
        deposit.apply(event).expect("cancel").outcome,
        ApplyOutcome::Applied
    );
    assert_eq!(
        deposit.apply(event).expect("cancel replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        deposit.state,
        DepositState::Cancelled {
            hold_id: None,
            history_watermark: None,
            ledger_failure: Some(current),
        } if current == failure
    ));
    assert!(matches!(
        deposit.apply(DepositEvent::PullSucceeded {
            ledger_block_index: 1,
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

fn observed_withdrawal() -> WithdrawalRecord {
    WithdrawalRecord::observed(
        WithdrawalId::new([4; 32]),
        [0; 20],
        vec![1],
        [0; 32],
        [5; 32],
        Amount::new(100),
        Amount::new(20),
        Amount::new(10),
        Amount::new(90),
        1,
    )
    .expect("valid withdrawal")
}

fn settlement() -> Settlement {
    Settlement {
        amount_out: Amount::new(90),
        service_fee: Amount::new(10),
        ledger_fee: Amount::new(5),
    }
}

fn withdrawal_transfer(amount: u128, fee: u128, tag: u8) -> LedgerTransferIdentity {
    let mut identity = transfer(LedgerOperation::ReleaseWithdrawal, amount, fee, tag);
    identity.to = Account::new(vec![1], [0; 32]).expect("withdrawal destination must be valid");
    identity
}

#[test]
fn withdrawal_payment_is_terminal_and_fee_reserve_is_net_of_ledger_fee() {
    let mut withdrawal = observed_withdrawal();
    let release_transfer = withdrawal_transfer(90, 5, 20);
    let start = WithdrawalEvent::StartRelease {
        attempt: Box::new(attempt(release_transfer)),
        settlement: settlement(),
    };
    withdrawal.apply(start.clone()).expect("start release");
    assert_eq!(
        withdrawal.apply(start).expect("start replay").outcome,
        ApplyOutcome::Idempotent
    );
    let released = WithdrawalEvent::ReleaseSucceeded {
        ledger_block_index: 71,
    };
    assert_eq!(
        withdrawal
            .apply(released.clone())
            .expect("release")
            .fee_delta,
        Amount::new(5)
    );
    assert_eq!(
        withdrawal
            .apply(released)
            .expect("release replay")
            .fee_delta,
        Amount::ZERO
    );
    assert!(matches!(withdrawal.state, WithdrawalState::Paid { .. }));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(withdrawal_transfer(90, 5, 21))),
            settlement: settlement(),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_release_rejects_a_ledger_fee_not_bound_to_settlement() {
    let mut withdrawal = observed_withdrawal();
    let mismatched = WithdrawalEvent::StartRelease {
        attempt: Box::new(attempt(withdrawal_transfer(90, 6, 70))),
        settlement: settlement(),
    };

    assert_eq!(
        withdrawal.apply(mismatched),
        Err(CoreError::SettlementMismatch)
    );
    assert_eq!(withdrawal.state, WithdrawalState::Observed);
}

#[test]
fn withdrawal_release_rejects_a_destination_not_bound_to_the_record() {
    let mut wrong_owner = withdrawal_transfer(90, 5, 71);
    wrong_owner.to = Account::new(vec![2], [0; 32]).expect("alternate owner must be valid");
    let mut wrong_subaccount = withdrawal_transfer(90, 5, 72);
    wrong_subaccount.to =
        Account::new(vec![1], [1; 32]).expect("alternate subaccount must be valid");

    for identity in [wrong_owner, wrong_subaccount] {
        let mut withdrawal = observed_withdrawal();
        assert_eq!(
            withdrawal.apply(WithdrawalEvent::StartRelease {
                attempt: Box::new(attempt(identity)),
                settlement: settlement(),
            }),
            Err(CoreError::SettlementMismatch)
        );
        assert_eq!(withdrawal.state, WithdrawalState::Observed);
    }
}

#[test]
fn withdrawal_hold_requires_evidence_before_payment_becomes_terminal() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(withdrawal_transfer(90, 5, 40))),
            settlement: settlement(),
        })
        .expect("start release");
    let hold_id = HoldId::new(44);
    withdrawal
        .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
        .expect("enter hold");
    let hold_transfer = match &withdrawal.state {
        WithdrawalState::ReconciliationHold { attempt, .. } => attempt.identity.clone(),
        _ => panic!("withdrawal must be held"),
    };
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Withdrawal(withdrawal.id),
        hold_transfer,
    );
    assert_eq!(
        resolve_withdrawal_hold(
            &mut withdrawal,
            &mut hold,
            WithdrawalHoldResolution::Succeeded {
                ledger_block_index: 46,
            },
        )
        .expect("resolve success")
        .fee_delta,
        Amount::new(5)
    );
}

#[test]
fn accounting_is_checked_and_separates_fee_kinds() {
    let mut accounting = AccountingState::default();
    accounting
        .confirm_fee(FeeKind::Deposit, Amount::new(10))
        .expect("deposit fee");
    accounting
        .confirm_fee(FeeKind::Withdrawal, Amount::new(20))
        .expect("withdrawal fee");
    assert_eq!(accounting.fee_reserve, Amount::new(30));
    assert_eq!(accounting.confirmed_deposit_fees, Amount::new(10));
    assert_eq!(accounting.confirmed_withdrawal_fees, Amount::new(20));

    accounting.fee_reserve = Amount::new(u128::MAX);
    let snapshot = accounting;
    assert_eq!(
        accounting.confirm_fee(FeeKind::Deposit, Amount::new(1)),
        Err(CoreError::ArithmeticOverflow)
    );
    assert_eq!(accounting, snapshot);
}

#[test]
fn settlement_reserve_is_checked_per_nonterminal_withdrawal() {
    let policy = ReservePolicy {
        eth_floor_wei: 100,
        cycles_floor: 200,
        settlement_cycle_ceiling: 30,
        transaction_gas_limit: 10,
        max_fee_per_gas: 4,
    };
    let exact = policy.snapshot(2, 0, 0, 180, 260).expect("exact reserve");
    assert!(exact.sufficient);
    assert_eq!(exact.required_eth_wei, 180);
    assert_eq!(exact.required_cycles, 260);
    assert!(
        !policy
            .snapshot(2, 0, 0, 179, 260)
            .expect("low ETH")
            .sufficient
    );
    assert!(
        !policy
            .snapshot(2, 0, 0, 180, 259)
            .expect("low cycles")
            .sufficient
    );
    let candidate = policy
        .snapshot(1, 0, 1, 180, 260)
        .expect("candidate reservation");
    assert_eq!(candidate.reserved_operation_count, 2);
    let existing_withdrawal = policy
        .snapshot(1, 0, 0, 140, 230)
        .expect("existing withdrawal reserve");
    assert!(existing_withdrawal.sufficient);
    let competing_deposit = policy
        .snapshot(1, 0, 1, 140, 230)
        .expect("competing deposit reserve");
    assert!(!competing_deposit.sufficient);
    assert_eq!(competing_deposit.nonterminal_withdrawals, 1);
    assert_eq!(competing_deposit.candidate_deposits, 1);
    let overflow = ReservePolicy {
        eth_floor_wei: u128::MAX,
        ..policy
    };
    assert_eq!(
        overflow.snapshot(1, 0, 0, u128::MAX, u128::MAX),
        Err(CoreError::ArithmeticOverflow)
    );
}

#[test]
fn evm_operation_is_ordered_and_idempotent() {
    let mut operation = EvmOperationRecord::prepared(
        EvmOperationId::new(1),
        [9; 32],
        EvmOperationKind::MintDeposit,
    );
    let submitted = EvmOperationEvent::Submitted {
        transaction_hash: [8; 32],
    };
    assert_eq!(
        operation.apply(submitted).expect("submit"),
        ApplyOutcome::Applied
    );
    assert_eq!(
        operation.apply(submitted).expect("submit replay"),
        ApplyOutcome::Idempotent
    );
    let confirmed = EvmOperationEvent::Confirmed {
        transaction_hash: [8; 32],
        receipt_block_number: 70,
        finalized_head_block_number: 77,
    };
    operation.apply(confirmed).expect("confirm");
    assert_eq!(
        operation.apply(confirmed).expect("confirm replay"),
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        operation.state,
        EvmOperationState::Confirmed {
            receipt_block_number: 70,
            finalized_head_block_number: 77,
            ..
        }
    ));
    assert_eq!(
        operation.apply(EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 71,
            finalized_head_block_number: 77,
        }),
        Err(CoreError::ConflictingReplay)
    );
}

#[test]
fn confirmed_revert_is_terminal_and_propagates_to_owned_records() {
    let operation_id = EvmOperationId::new(12);
    let mut operation =
        EvmOperationRecord::prepared(operation_id, [9; 32], EvmOperationKind::MintDeposit);
    operation
        .apply(EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        })
        .expect("submit");
    let reverted = EvmOperationEvent::Reverted {
        transaction_hash: [8; 32],
        receipt_block_number: 70,
        finalized_head_block_number: 78,
    };
    assert_eq!(
        operation.apply(reverted).expect("revert"),
        ApplyOutcome::Applied
    );
    assert_eq!(
        operation.apply(reverted).expect("revert replay"),
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        operation.state,
        EvmOperationState::Reverted {
            finalized_head_block_number: 78,
            ..
        }
    ));
    assert!(matches!(
        operation.apply(EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 70,
            finalized_head_block_number: 78,
        }),
        Err(CoreError::ConflictingReplay)
    ));

    let mut deposit = accepted_deposit();
    deposit
        .apply(DepositEvent::PullSucceeded {
            ledger_block_index: 42,
        })
        .expect("pull");
    deposit
        .apply(DepositEvent::PrepareMint { operation_id })
        .expect("prepare mint");
    deposit
        .apply(DepositEvent::MintReverted { operation_id })
        .expect("propagate revert");
    assert!(matches!(deposit.state, DepositState::MintReverted { .. }));
    assert!(matches!(
        deposit.apply(DepositEvent::MintConfirmed { operation_id }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn reconciliation_resolution_is_evidence_typed_and_terminal() {
    let mut deposit = accepted_deposit();
    let hold_id = HoldId::new(1);
    deposit
        .apply(DepositEvent::PullAmbiguous { hold_id })
        .expect("hold deposit");
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Deposit(deposit.id),
        deposit.transfer.clone(),
    );
    let resolution = DepositHoldResolution::Absent {
        history_watermark: 100,
    };
    assert_eq!(
        resolve_deposit_hold(&mut deposit, &mut hold, resolution)
            .expect("resolve")
            .outcome,
        ApplyOutcome::Applied,
    );
    assert_eq!(
        resolve_deposit_hold(&mut deposit, &mut hold, resolution)
            .expect("resolve replay")
            .outcome,
        ApplyOutcome::Idempotent,
    );
    assert_eq!(
        resolve_deposit_hold(
            &mut deposit,
            &mut hold,
            DepositHoldResolution::Succeeded {
                ledger_block_index: 2,
            },
        ),
        Err(CoreError::ConflictingReplay)
    );
    assert_eq!(
        hold.state,
        ReconciliationHoldState::ResolvedAbsent {
            history_watermark: 100
        }
    );

    assert!(matches!(
        deposit.state,
        DepositState::Cancelled {
            hold_id: Some(current),
            history_watermark: Some(100),
            ledger_failure: None,
        } if current == hold_id
    ));
}

#[test]
fn cancelled_deposit_is_terminal_and_id_is_not_reopened() {
    let mut deposit = accepted_deposit();
    let hold_id = HoldId::new(70);
    deposit
        .apply(DepositEvent::PullAmbiguous { hold_id })
        .expect("hold deposit");
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Deposit(deposit.id),
        deposit.transfer.clone(),
    );
    resolve_deposit_hold(
        &mut deposit,
        &mut hold,
        DepositHoldResolution::Absent {
            history_watermark: 900,
        },
    )
    .expect("cancel with evidence");
    assert!(matches!(deposit.state, DepositState::Cancelled { .. }));
    assert_eq!(deposit.verify_retry([2; 32]), Ok(()));
    assert_eq!(
        deposit.verify_retry([9; 32]),
        Err(CoreError::PayloadConflict)
    );
    assert!(matches!(
        deposit.apply(DepositEvent::PullSucceeded {
            ledger_block_index: 901
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_attempt_changes_only_time_and_memo_after_absence() {
    let original = withdrawal_transfer(90, 5, 80);
    let first = attempt(original.clone());
    let mut replacement = original.clone();
    replacement.created_at_time_ns += 1;
    replacement.memo = [81; 32];
    let second = first
        .retry_after_absence(replacement.clone())
        .expect("valid replacement");
    assert_eq!(second.attempt_no, 1);
    assert_eq!(second.identity.amount, original.amount);
    assert_eq!(second.identity.fee, original.fee);
    assert_eq!(second.identity.from, original.from);
    assert_eq!(second.identity.to, original.to);
    let mut changed_amount = replacement;
    changed_amount.amount = Amount::new(84);
    assert_eq!(
        first.retry_after_absence(changed_amount),
        Err(CoreError::AttemptPayloadChanged)
    );
}

#[test]
fn withdrawal_absence_resolution_replay_binds_old_hold_to_exact_replacement() {
    let mut withdrawal = observed_withdrawal();
    let original = withdrawal_transfer(90, 5, 80);
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(original.clone())),
            settlement: settlement(),
        })
        .expect("start release");
    let hold_id = HoldId::new(81);
    withdrawal
        .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
        .expect("hold release");
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Withdrawal(withdrawal.id),
        original.clone(),
    );
    let mut replacement = original;
    replacement.created_at_time_ns += 1;
    replacement.memo = [81; 32];
    let resolution = WithdrawalHoldResolution::Absent {
        history_watermark: 100,
        next_identity: Box::new(replacement.clone()),
    };

    assert_eq!(
        resolve_withdrawal_hold(&mut withdrawal, &mut hold, resolution.clone())
            .expect("resolve absence")
            .outcome,
        ApplyOutcome::Applied
    );
    assert_eq!(
        resolve_withdrawal_hold(&mut withdrawal, &mut hold, resolution)
            .expect("replay absence")
            .outcome,
        ApplyOutcome::Idempotent
    );

    let mut different = replacement;
    different.created_at_time_ns += 1;
    different.memo = [82; 32];
    assert_eq!(
        resolve_withdrawal_hold(
            &mut withdrawal,
            &mut hold,
            WithdrawalHoldResolution::Absent {
                history_watermark: 100,
                next_identity: Box::new(different),
            },
        ),
        Err(CoreError::HoldMismatch)
    );
}

#[test]
fn principals_and_settlement_inputs_are_rejected_at_boundaries() {
    assert_eq!(
        Account::new(Vec::new(), [0; 32]),
        Err(CoreError::InvalidPrincipal)
    );
    assert_eq!(
        Account::new(vec![4], [0; 32]),
        Err(CoreError::InvalidPrincipal)
    );
    assert_eq!(
        Account::new(vec![1; 30], [0; 32]),
        Err(CoreError::InvalidPrincipal)
    );
    assert_eq!(
        Settlement {
            amount_out: Amount::new(90),
            service_fee: Amount::new(10),
            ledger_fee: Amount::new(5),
        }
        .validate_committed(Amount::new(100), Amount::new(10)),
        Ok(())
    );
    assert_eq!(
        Settlement {
            amount_out: Amount::new(90),
            service_fee: Amount::new(10),
            ledger_fee: Amount::new(11),
        }
        .validate_committed(Amount::new(100), Amount::new(10)),
        Err(CoreError::SettlementMismatch)
    );
}

#[test]
fn withdrawal_terminal_transition_rechecks_the_committed_quote() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(withdrawal_transfer(90, 5, 90))),
            settlement: settlement(),
        })
        .expect("start release");
    if let WithdrawalState::ReleasePending { settlement, .. } = &mut withdrawal.state {
        settlement.amount_out = Amount::new(89);
    } else {
        panic!("release must be pending");
    }

    assert_eq!(
        withdrawal.apply(WithdrawalEvent::ReleaseSucceeded {
            ledger_block_index: 90,
        }),
        Err(CoreError::SettlementMismatch)
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExpectedTransition {
    Applied,
    Idempotent,
    Rejected,
}

fn expected_apply(result: Result<bridge_core::ApplyResult, CoreError>) -> ExpectedTransition {
    match result {
        Ok(result) if result.outcome == ApplyOutcome::Applied => ExpectedTransition::Applied,
        Ok(result) if result.outcome == ApplyOutcome::Idempotent => ExpectedTransition::Idempotent,
        Ok(_) => unreachable!("ApplyOutcome has no other variants"),
        Err(_) => ExpectedTransition::Rejected,
    }
}

#[test]
fn deposit_state_event_transition_matrix_covers_all_current_events() {
    let states = [
        DepositState::PullPending,
        DepositState::Escrowed {
            ledger_block_index: 11,
        },
        DepositState::MintPending {
            ledger_block_index: 11,
            operation_id: EvmOperationId::new(2),
        },
        DepositState::Minted {
            ledger_block_index: 11,
            operation_id: EvmOperationId::new(2),
        },
        DepositState::MintReverted {
            ledger_block_index: 11,
            operation_id: EvmOperationId::new(2),
        },
        DepositState::ReconciliationHold {
            hold_id: HoldId::new(3),
        },
        DepositState::Cancelled {
            hold_id: None,
            history_watermark: None,
            ledger_failure: Some(bridge_core::LedgerFailure::InsufficientAllowance {
                allowance: Amount::ZERO,
            }),
        },
    ];
    let events = [
        DepositEvent::PullSucceeded {
            ledger_block_index: 11,
        },
        DepositEvent::PullAmbiguous {
            hold_id: HoldId::new(3),
        },
        DepositEvent::PullFailed {
            code: bridge_core::LedgerFailure::InsufficientAllowance {
                allowance: Amount::ZERO,
            },
        },
        DepositEvent::PrepareMint {
            operation_id: EvmOperationId::new(2),
        },
        DepositEvent::MintConfirmed {
            operation_id: EvmOperationId::new(2),
        },
        DepositEvent::MintReverted {
            operation_id: EvmOperationId::new(2),
        },
        DepositEvent::RetryMint {
            reverted_operation_id: EvmOperationId::new(2),
            replacement_operation_id: EvmOperationId::new(3),
        },
    ];
    use ExpectedTransition::{Applied as A, Idempotent as I, Rejected as R};
    let expected = [
        [A, A, A, R, R, R, R],
        [I, R, R, A, R, R, R],
        [R, R, R, I, A, A, R],
        [R, R, R, I, I, R, R],
        [R, R, R, R, R, I, A],
        [R, I, R, R, R, R, R],
        [R, R, I, R, R, R, R],
    ];

    for (state_index, state) in states.into_iter().enumerate() {
        for (event_index, event) in events.into_iter().enumerate() {
            let mut record = accepted_deposit();
            record.state = state.clone();
            assert_eq!(
                expected_apply(record.apply(event)),
                expected[state_index][event_index],
                "deposit state {state_index}, event {event_index}"
            );
        }
    }
}

#[test]
fn withdrawal_state_event_transition_matrix_covers_all_current_events() {
    let release_transfer = withdrawal_transfer(90, 5, 60);
    let release_settlement = settlement();
    let states = [
        WithdrawalState::Observed,
        WithdrawalState::ReleasePending {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
        },
        WithdrawalState::Paid {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
        },
        WithdrawalState::ReconciliationHold {
            hold_id: HoldId::new(3),
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
        },
    ];
    let events = [
        WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(release_transfer)),
            settlement: release_settlement,
        },
        WithdrawalEvent::ReleaseSucceeded {
            ledger_block_index: 11,
        },
        WithdrawalEvent::ReleaseAmbiguous {
            hold_id: HoldId::new(3),
        },
    ];
    let expected = [
        [
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
        ],
    ];

    for (state_index, state) in states.into_iter().enumerate() {
        for (event_index, event) in events.iter().cloned().enumerate() {
            let mut record = observed_withdrawal();
            record.state = state.clone();
            assert_eq!(
                expected_apply(record.apply(event)),
                expected[state_index][event_index],
                "withdrawal state {state_index}, event {event_index}"
            );
        }
    }
}

#[test]
fn evm_state_event_transition_matrix_covers_all_current_states_and_events() {
    let states = [
        EvmOperationState::Queued,
        EvmOperationState::Prepared,
        EvmOperationState::Submitted {
            transaction_hash: [8; 32],
        },
        EvmOperationState::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
        },
        EvmOperationState::Reverted {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
        },
        EvmOperationState::RecoveryPending {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
            replacement_operation_id: EvmOperationId::new(2),
        },
        EvmOperationState::Recovered {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
            resolution: EvmRecoveryResolution::ReplacementConfirmed {
                replacement_operation_id: EvmOperationId::new(2),
            },
        },
    ];
    let events = [
        EvmOperationEvent::Prepared,
        EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        },
        EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
        },
        EvmOperationEvent::Reverted {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            finalized_head_block_number: 11,
        },
        EvmOperationEvent::StartRecovery {
            replacement_operation_id: EvmOperationId::new(2),
        },
        EvmOperationEvent::ResolveRecovery {
            resolution: EvmRecoveryResolution::ReplacementConfirmed {
                replacement_operation_id: EvmOperationId::new(2),
            },
        },
    ];
    use ExpectedTransition::{Applied as A, Idempotent as I, Rejected as R};
    let expected = [
        [A, R, R, R, R, R],
        [I, A, R, R, R, R],
        [I, I, A, A, R, R],
        [I, I, I, R, R, R],
        [I, I, R, I, A, A],
        [R, R, R, R, I, A],
        [R, R, R, R, R, I],
    ];

    for (state_index, state) in states.into_iter().enumerate() {
        for (event_index, event) in events.into_iter().enumerate() {
            let mut record = EvmOperationRecord::prepared(
                EvmOperationId::new(1),
                [9; 32],
                EvmOperationKind::MintDeposit,
            );
            record.state = state;
            let actual = match record.apply(event) {
                Ok(ApplyOutcome::Applied) => ExpectedTransition::Applied,
                Ok(ApplyOutcome::Idempotent) => ExpectedTransition::Idempotent,
                Err(_) => ExpectedTransition::Rejected,
            };
            assert_eq!(
                actual, expected[state_index][event_index],
                "EVM state {state_index}, event {event_index}"
            );
        }
    }

    let mut confirmed = EvmOperationRecord::prepared(
        EvmOperationId::new(1),
        [9; 32],
        EvmOperationKind::MintDeposit,
    );
    confirmed.state = EvmOperationState::Confirmed {
        transaction_hash: [8; 32],
        receipt_block_number: 10,
        finalized_head_block_number: 11,
    };
    assert_eq!(
        confirmed.apply(EvmOperationEvent::Submitted {
            transaction_hash: [7; 32],
        }),
        Err(CoreError::ConflictingReplay)
    );
}

#[test]
fn reverted_operations_require_explicit_recovery_transitions() {
    let reverted_id = EvmOperationId::new(7);
    let replacement_id = EvmOperationId::new(8);
    let mut deposit = accepted_deposit();
    deposit
        .apply(DepositEvent::PullSucceeded {
            ledger_block_index: 11,
        })
        .expect("escrow deposit");
    deposit
        .apply(DepositEvent::PrepareMint {
            operation_id: reverted_id,
        })
        .expect("prepare mint");
    deposit
        .apply(DepositEvent::MintReverted {
            operation_id: reverted_id,
        })
        .expect("record revert");
    deposit
        .apply(DepositEvent::RetryMint {
            reverted_operation_id: reverted_id,
            replacement_operation_id: replacement_id,
        })
        .expect("explicit recovery");
    assert!(matches!(
        deposit.state,
        DepositState::MintPending { operation_id, .. } if operation_id == replacement_id
    ));

    let mut operation =
        EvmOperationRecord::prepared(reverted_id, [9; 32], EvmOperationKind::MintDeposit);
    operation.state = EvmOperationState::Reverted {
        transaction_hash: [3; 32],
        receipt_block_number: 10,
        finalized_head_block_number: 12,
    };
    operation
        .apply(EvmOperationEvent::StartRecovery {
            replacement_operation_id: replacement_id,
        })
        .expect("start operation recovery");
    assert!(matches!(
        operation.state,
        EvmOperationState::RecoveryPending {
            replacement_operation_id: id,
            ..
        } if id == replacement_id
    ));
}
