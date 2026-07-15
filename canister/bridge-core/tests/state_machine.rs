use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, terminal_liability_residual, Account,
    AccountingState, Amount, ApplyOutcome, BaseMintSnapshot, CoreError, DepositEvent,
    DepositHoldResolution, DepositId, DepositRecord, DepositRequest, DepositState, EvmCallIntent,
    EvmOperationEvent, EvmOperationId, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    FeeKind, HoldId, LedgerOperation, LedgerTransferIdentity, ReconciliationHoldRecord,
    ReconciliationHoldState, RefundEligibility, RefundReason, RequestReference, ReservePolicy,
    Settlement, TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId,
    WithdrawalRecord, WithdrawalState,
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
        confirmed_block_number: 1,
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

fn refund_eligibility() -> RefundEligibility {
    RefundEligibility {
        confirmed_base_block: 100,
        base_status_pending: true,
        release_transfer_proven_absent: true,
        reason: RefundReason::AmountBelowMinimum,
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
        vec![1],
        [5; 32],
        Amount::new(100),
        Amount::new(80),
        Amount::new(10),
    )
    .expect("valid withdrawal")
}

fn settlement() -> Settlement {
    Settlement {
        amount_out: Amount::new(85),
        service_fee: Amount::new(10),
        ledger_fee: Amount::new(5),
    }
}

#[test]
fn withdrawal_release_is_terminal_and_fee_is_not_double_counted() {
    let mut withdrawal = observed_withdrawal();
    let release_transfer = transfer(LedgerOperation::ReleaseWithdrawal, 85, 5, 20);
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
        Amount::new(10)
    );
    assert_eq!(
        withdrawal
            .apply(released)
            .expect("release replay")
            .fee_delta,
        Amount::ZERO
    );

    let prepare = WithdrawalEvent::PrepareAcknowledgement {
        operation_id: EvmOperationId::new(8),
    };
    withdrawal
        .apply(prepare.clone())
        .expect("prepare acknowledgement");
    assert_eq!(
        withdrawal.apply(prepare).expect("prepare replay").outcome,
        ApplyOutcome::Idempotent
    );
    let confirmed = WithdrawalEvent::AcknowledgementConfirmed {
        operation_id: EvmOperationId::new(8),
    };
    withdrawal.apply(confirmed.clone()).expect("acknowledge");
    assert_eq!(
        withdrawal.apply(confirmed).expect("ack replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert!(matches!(withdrawal.state, WithdrawalState::Released { .. }));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(9),
            eligibility: refund_eligibility(),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn release_and_bad_fee_repricing_are_fail_closed() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(transfer(
                LedgerOperation::ReleaseWithdrawal,
                85,
                5,
                70,
            ))),
            settlement: settlement(),
        })
        .expect("prepare release");
    assert!(matches!(
        withdrawal.state,
        WithdrawalState::ReleasePending { .. }
    ));

    let mut repriced_identity = transfer(LedgerOperation::ReleaseWithdrawal, 85, 5, 70);
    repriced_identity.created_at_time_ns = 71;
    repriced_identity.memo = [71; 32];
    repriced_identity.amount = Amount::new(84);
    repriced_identity.fee = Amount::new(6);
    let repriced = TransferAttempt {
        attempt_no: 1,
        identity: repriced_identity.clone(),
    };
    withdrawal
        .apply(WithdrawalEvent::RepriceRelease {
            attempt: Box::new(repriced.clone()),
            settlement: Settlement {
                amount_out: Amount::new(84),
                service_fee: Amount::new(10),
                ledger_fee: Amount::new(6),
            },
        })
        .expect("definitive BadFee can be repriced");
    assert!(matches!(
        withdrawal.state,
        WithdrawalState::ReleasePending { ref attempt, .. } if attempt == &repriced
    ));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::RepriceRelease {
            attempt: Box::new(TransferAttempt {
                attempt_no: 2,
                identity: {
                    repriced_identity.created_at_time_ns = 72;
                    repriced_identity.memo = [72; 32];
                    repriced_identity.amount = Amount::new(79);
                    repriced_identity.fee = Amount::new(11);
                    repriced_identity
                },
            }),
            settlement: Settlement {
                amount_out: Amount::new(79),
                service_fee: Amount::new(10),
                ledger_fee: Amount::new(11),
            },
        }),
        Err(CoreError::MinimumAmountNotMet)
    ));

    withdrawal
        .apply(WithdrawalEvent::PrepareReleaseCancellation {
            operation_id: EvmOperationId::new(71),
            expected_ledger_fee: Amount::new(11),
        })
        .expect("prepare cancellation below minimum");
    withdrawal
        .apply(WithdrawalEvent::ReleaseCancellationConfirmed {
            operation_id: EvmOperationId::new(71),
        })
        .expect("confirm cancellation");
    withdrawal
        .apply(WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(72),
            eligibility: refund_eligibility(),
        })
        .expect("refund after proven cancellation");
}

#[test]
fn withdrawal_refund_path_cannot_become_released() {
    let mut withdrawal = observed_withdrawal();
    let refund = WithdrawalEvent::StartRefund {
        operation_id: EvmOperationId::new(30),
        eligibility: refund_eligibility(),
    };
    withdrawal.apply(refund.clone()).expect("start refund");
    assert_eq!(
        withdrawal.apply(refund).expect("refund replay").outcome,
        ApplyOutcome::Idempotent
    );
    withdrawal
        .apply(WithdrawalEvent::RefundConfirmed {
            operation_id: EvmOperationId::new(30),
        })
        .expect("confirm refund");
    assert!(matches!(withdrawal.state, WithdrawalState::Refunded { .. }));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::AcknowledgementConfirmed {
            operation_id: EvmOperationId::new(30),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_refund_operation_binds_id_payload_and_exact_calldata() {
    let operation_id = EvmOperationId::new(30);
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRefund {
            operation_id,
            eligibility: refund_eligibility(),
        })
        .expect("start refund");
    let operation = EvmOperationRecord::queued(
        operation_id,
        withdrawal.payload_hash,
        EvmOperationKind::RefundWithdrawal,
    );
    let mut calldata = vec![0xf0, 0x65, 0xe1, 0xff];
    calldata.extend_from_slice(&withdrawal.id.bytes());
    let intent = EvmCallIntent {
        operation_id,
        payload_hash: withdrawal.payload_hash,
        chain_id: 1,
        contract: [1; 20],
        calldata,
        gas_limit: 1,
        max_fee_per_gas: 1,
        max_priority_fee_per_gas: 1,
    };
    assert!(withdrawal.refund_operation_matches(&operation, &intent));

    let mut wrong_id = intent.clone();
    wrong_id.calldata[35] ^= 1;
    assert!(!withdrawal.refund_operation_matches(&operation, &wrong_id));
    let mut wrong_selector = intent.clone();
    wrong_selector.calldata[0] ^= 1;
    assert!(!withdrawal.refund_operation_matches(&operation, &wrong_selector));
    let wrong_operation = EvmOperationRecord::queued(
        EvmOperationId::new(31),
        withdrawal.payload_hash,
        EvmOperationKind::RefundWithdrawal,
    );
    assert!(!withdrawal.refund_operation_matches(&wrong_operation, &intent));
}

#[test]
fn withdrawal_acknowledgement_revert_is_terminal_and_idempotent() {
    let operation_id = EvmOperationId::new(31);
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(transfer(
                LedgerOperation::ReleaseWithdrawal,
                85,
                5,
                31,
            ))),
            settlement: settlement(),
        })
        .expect("start release");
    withdrawal
        .apply(WithdrawalEvent::ReleaseSucceeded {
            ledger_block_index: 32,
        })
        .expect("release");
    withdrawal
        .apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })
        .expect("prepare acknowledgement");
    let reverted = WithdrawalEvent::AcknowledgementReverted { operation_id };
    assert_eq!(
        withdrawal.apply(reverted.clone()).expect("revert").outcome,
        ApplyOutcome::Applied
    );
    assert_eq!(
        withdrawal.apply(reverted).expect("revert replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        withdrawal.state,
        WithdrawalState::AcknowledgeReverted { .. }
    ));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::AcknowledgementConfirmed { operation_id }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_refund_revert_is_terminal_and_idempotent() {
    let operation_id = EvmOperationId::new(32);
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRefund {
            operation_id,
            eligibility: refund_eligibility(),
        })
        .expect("start refund");
    let reverted = WithdrawalEvent::RefundReverted { operation_id };
    assert_eq!(
        withdrawal.apply(reverted.clone()).expect("revert").outcome,
        ApplyOutcome::Applied
    );
    assert_eq!(
        withdrawal.apply(reverted).expect("revert replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        withdrawal.state,
        WithdrawalState::RefundReverted { .. }
    ));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::RefundConfirmed { operation_id }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_hold_blocks_refund_until_evidence_resolves_original_release() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(transfer(
                LedgerOperation::ReleaseWithdrawal,
                85,
                5,
                40,
            ))),
            settlement: settlement(),
        })
        .expect("start release");
    let hold_id = HoldId::new(44);
    withdrawal
        .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
        .expect("enter hold");
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(45),
            eligibility: refund_eligibility(),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
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
        Amount::new(10)
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
        confirmed_block_number: 77,
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
            confirmed_block_number: 77,
            ..
        }
    ));
    assert_eq!(
        operation.apply(EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 71,
            confirmed_block_number: 77,
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
        confirmed_block_number: 78,
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
            confirmed_block_number: 78,
            ..
        }
    ));
    assert!(matches!(
        operation.apply(EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 70,
            confirmed_block_number: 78,
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
    let original = transfer(LedgerOperation::ReleaseWithdrawal, 85, 5, 80);
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
            amount_out: Amount::new(80),
            service_fee: Amount::new(10),
            ledger_fee: Amount::new(9),
        }
        .validate(Amount::new(100), Amount::new(80), Amount::new(10)),
        Err(CoreError::SettlementMismatch)
    );
    assert_eq!(terminal_liability_residual(100, 85, 10, 5), Some(0));
    assert_eq!(terminal_liability_residual(100, 80, 10, 5), Some(5));
    assert_eq!(terminal_liability_residual(99, 85, 10, 5), None);
    assert_eq!(
        terminal_liability_residual(u128::MAX, u128::MAX, 1, 0),
        None
    );
    assert_eq!(
        Settlement {
            amount_out: Amount::new(u128::MAX),
            service_fee: Amount::new(1),
            ledger_fee: Amount::ZERO,
        }
        .terminal_liability_residual(Amount::new(u128::MAX)),
        Err(CoreError::ArithmeticOverflow)
    );
}

#[test]
fn withdrawal_terminal_transition_rechecks_zero_liability_residual() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt(transfer(
                LedgerOperation::ReleaseWithdrawal,
                85,
                5,
                90,
            ))),
            settlement: settlement(),
        })
        .expect("start release");
    withdrawal
        .apply(WithdrawalEvent::ReleaseSucceeded {
            ledger_block_index: 90,
        })
        .expect("transfer release");
    withdrawal
        .apply(WithdrawalEvent::PrepareAcknowledgement {
            operation_id: EvmOperationId::new(91),
        })
        .expect("prepare acknowledgement");

    if let WithdrawalState::AcknowledgePending { settlement, .. } = &mut withdrawal.state {
        settlement.amount_out = Amount::new(84);
    } else {
        panic!("acknowledgement must be pending");
    }

    assert_eq!(
        withdrawal.apply(WithdrawalEvent::AcknowledgementConfirmed {
            operation_id: EvmOperationId::new(91),
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
fn deposit_state_event_transition_table_is_exhaustive() {
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
    ];
    let expected = [
        [
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
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
fn withdrawal_state_event_transition_table_is_exhaustive() {
    let release_transfer = transfer(LedgerOperation::ReleaseWithdrawal, 85, 5, 60);
    let release_settlement = settlement();
    let states = [
        WithdrawalState::Observed,
        WithdrawalState::ReleasePending {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
        },
        WithdrawalState::ReleaseTransferred {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
        },
        WithdrawalState::AcknowledgePending {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalState::Released {
            attempt: attempt(release_transfer.clone()),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalState::RefundPending {
            operation_id: EvmOperationId::new(4),
            eligibility: refund_eligibility(),
        },
        WithdrawalState::Refunded {
            operation_id: EvmOperationId::new(4),
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
        WithdrawalEvent::PrepareAcknowledgement {
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalEvent::AcknowledgementConfirmed {
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(4),
            eligibility: refund_eligibility(),
        },
        WithdrawalEvent::RefundConfirmed {
            operation_id: EvmOperationId::new(4),
        },
    ];
    let expected = [
        [
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
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
fn evm_state_event_transition_table_is_exhaustive() {
    let states = [
        EvmOperationState::Prepared,
        EvmOperationState::Submitted {
            transaction_hash: [8; 32],
        },
        EvmOperationState::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            confirmed_block_number: 11,
        },
    ];
    let events = [
        EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        },
        EvmOperationEvent::Confirmed {
            transaction_hash: [8; 32],
            receipt_block_number: 10,
            confirmed_block_number: 11,
        },
    ];
    let expected = [
        [ExpectedTransition::Applied, ExpectedTransition::Rejected],
        [ExpectedTransition::Idempotent, ExpectedTransition::Applied],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
        ],
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
        confirmed_block_number: 11,
    };
    assert_eq!(
        confirmed.apply(EvmOperationEvent::Submitted {
            transaction_hash: [7; 32],
        }),
        Err(CoreError::ConflictingReplay)
    );
}
