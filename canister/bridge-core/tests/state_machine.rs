use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, Account, AccountingState, Amount, ApplyOutcome,
    BaseMintSnapshot, CoreError, DepositEvent, DepositId, DepositRecord, DepositRequest,
    DepositState, EvmOperationEvent, EvmOperationId, EvmOperationKind, EvmOperationRecord,
    EvmOperationState, FeeKind, HoldId, HoldResolution, LedgerOperation, LedgerTransferIdentity,
    ReconciliationHoldRecord, ReconciliationHoldState, RequestReference, ResourceBudget,
    ResourceCost, Settlement, WithdrawalEvent, WithdrawalId, WithdrawalRecord, WithdrawalState,
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
        service_fee: Amount::new(service_fee),
        max_service_fee: Amount::new(20),
        per_deposit_limit: Amount::new(1_000),
        mint_window_limit: Amount::new(10_000),
        minted_in_window: Amount::new(100),
    }
}

fn resources() -> ResourceBudget {
    ResourceBudget {
        available: ResourceCost {
            eth_wei: 1_000,
            cycles: 2_000,
        },
        settlement_floor: ResourceCost {
            eth_wei: 100,
            cycles: 200,
        },
        pending_settlements: ResourceCost {
            eth_wei: 200,
            cycles: 300,
        },
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
        resources(),
        ResourceCost {
            eth_wei: 50,
            cycles: 60,
        },
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
}

#[test]
fn settlement_reserve_is_component_wise_and_checked() {
    assert_eq!(
        resources().ensure_deposit_can_reserve(ResourceCost {
            eth_wei: 700,
            cycles: 1_500,
        }),
        Ok(())
    );
    assert_eq!(
        resources().ensure_deposit_can_reserve(ResourceCost {
            eth_wei: 701,
            cycles: 1,
        }),
        Err(CoreError::InsufficientSettlementReserve)
    );
    let overflow = ResourceBudget {
        available: ResourceCost {
            eth_wei: u128::MAX,
            cycles: u128::MAX,
        },
        settlement_floor: ResourceCost {
            eth_wei: u128::MAX,
            cycles: 0,
        },
        pending_settlements: ResourceCost {
            eth_wei: 1,
            cycles: 0,
        },
    };
    assert_eq!(
        overflow.ensure_deposit_can_reserve(ResourceCost::default()),
        Err(CoreError::ArithmeticOverflow)
    );
}

#[test]
fn deposit_fee_is_confirmed_only_on_first_finalized_mint() {
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

    let finalized = DepositEvent::MintFinalized {
        operation_id: EvmOperationId::new(7),
    };
    assert_eq!(
        deposit.apply(finalized).expect("mint finalized").fee_delta,
        Amount::new(10)
    );
    assert_eq!(
        deposit.apply(finalized).expect("mint replay").fee_delta,
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
            HoldResolution::Succeeded {
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
            HoldResolution::Succeeded {
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
            HoldResolution::Succeeded {
                ledger_block_index: 88,
            },
        ),
        Err(CoreError::HoldMismatch)
    );
    resolve_deposit_hold(
        &mut deposit,
        &mut hold,
        HoldResolution::Succeeded {
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

fn observed_withdrawal() -> WithdrawalRecord {
    WithdrawalRecord::observed(
        WithdrawalId::new([4; 32]),
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
        transfer: Box::new(release_transfer),
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
    let finalized = WithdrawalEvent::AcknowledgementFinalized {
        operation_id: EvmOperationId::new(8),
    };
    withdrawal.apply(finalized.clone()).expect("acknowledge");
    assert_eq!(
        withdrawal.apply(finalized).expect("ack replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert!(matches!(withdrawal.state, WithdrawalState::Released { .. }));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(9),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_refund_path_cannot_become_released() {
    let mut withdrawal = observed_withdrawal();
    let refund = WithdrawalEvent::StartRefund {
        operation_id: EvmOperationId::new(30),
    };
    withdrawal.apply(refund.clone()).expect("start refund");
    assert_eq!(
        withdrawal.apply(refund).expect("refund replay").outcome,
        ApplyOutcome::Idempotent
    );
    withdrawal
        .apply(WithdrawalEvent::RefundFinalized {
            operation_id: EvmOperationId::new(30),
        })
        .expect("finalize refund");
    assert!(matches!(withdrawal.state, WithdrawalState::Refunded { .. }));
    assert!(matches!(
        withdrawal.apply(WithdrawalEvent::AcknowledgementFinalized {
            operation_id: EvmOperationId::new(30),
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
}

#[test]
fn withdrawal_hold_blocks_refund_until_evidence_resolves_original_release() {
    let mut withdrawal = observed_withdrawal();
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            transfer: Box::new(transfer(LedgerOperation::ReleaseWithdrawal, 85, 5, 40)),
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
        }),
        Err(CoreError::InvalidTransition { .. })
    ));
    let hold_transfer = match &withdrawal.state {
        WithdrawalState::ReconciliationHold { transfer, .. } => transfer.clone(),
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
            HoldResolution::Succeeded {
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
    let finalized = EvmOperationEvent::Finalized {
        transaction_hash: [8; 32],
        finalized_block_number: 77,
    };
    operation.apply(finalized).expect("finalize");
    assert_eq!(
        operation.apply(finalized).expect("finalize replay"),
        ApplyOutcome::Idempotent
    );
    assert!(matches!(
        operation.state,
        EvmOperationState::Finalized { .. }
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
    let resolution = HoldResolution::Absent {
        history_watermark: Some(100),
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
            HoldResolution::Succeeded {
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

    let mut missing_deposit = accepted_deposit();
    missing_deposit
        .apply(DepositEvent::PullAmbiguous { hold_id })
        .expect("hold deposit");
    let mut missing_hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::Deposit(missing_deposit.id),
        missing_deposit.transfer.clone(),
    );
    assert_eq!(
        resolve_deposit_hold(
            &mut missing_deposit,
            &mut missing_hold,
            HoldResolution::Absent {
                history_watermark: None,
            },
        ),
        Err(CoreError::MissingReconciliationEvidence)
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
        DepositState::ReconciliationHold {
            hold_id: HoldId::new(3),
        },
    ];
    let events = [
        DepositEvent::PullSucceeded {
            ledger_block_index: 11,
        },
        DepositEvent::PullAmbiguous {
            hold_id: HoldId::new(3),
        },
        DepositEvent::PrepareMint {
            operation_id: EvmOperationId::new(2),
        },
        DepositEvent::MintFinalized {
            operation_id: EvmOperationId::new(2),
        },
    ];
    let expected = [
        [
            ExpectedTransition::Applied,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Idempotent,
            ExpectedTransition::Rejected,
            ExpectedTransition::Applied,
            ExpectedTransition::Rejected,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Applied,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
            ExpectedTransition::Idempotent,
        ],
        [
            ExpectedTransition::Rejected,
            ExpectedTransition::Idempotent,
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
            transfer: release_transfer.clone(),
            settlement: release_settlement,
        },
        WithdrawalState::ReleaseTransferred {
            transfer: release_transfer.clone(),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
        },
        WithdrawalState::AcknowledgePending {
            transfer: release_transfer.clone(),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalState::Released {
            transfer: release_transfer.clone(),
            settlement: release_settlement,
            ledger_block_index: 11,
            source_hold: None,
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalState::RefundPending {
            operation_id: EvmOperationId::new(4),
        },
        WithdrawalState::Refunded {
            operation_id: EvmOperationId::new(4),
        },
        WithdrawalState::ReconciliationHold {
            hold_id: HoldId::new(3),
            transfer: release_transfer.clone(),
            settlement: release_settlement,
        },
    ];
    let events = [
        WithdrawalEvent::StartRelease {
            transfer: Box::new(release_transfer),
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
        WithdrawalEvent::AcknowledgementFinalized {
            operation_id: EvmOperationId::new(2),
        },
        WithdrawalEvent::StartRefund {
            operation_id: EvmOperationId::new(4),
        },
        WithdrawalEvent::RefundFinalized {
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
        EvmOperationState::Finalized {
            transaction_hash: [8; 32],
            finalized_block_number: 11,
        },
    ];
    let events = [
        EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        },
        EvmOperationEvent::Finalized {
            transaction_hash: [8; 32],
            finalized_block_number: 11,
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

    let mut finalized = EvmOperationRecord::prepared(
        EvmOperationId::new(1),
        [9; 32],
        EvmOperationKind::MintDeposit,
    );
    finalized.state = EvmOperationState::Finalized {
        transaction_hash: [8; 32],
        finalized_block_number: 11,
    };
    assert_eq!(
        finalized.apply(EvmOperationEvent::Submitted {
            transaction_hash: [7; 32],
        }),
        Err(CoreError::ConflictingReplay)
    );
}
