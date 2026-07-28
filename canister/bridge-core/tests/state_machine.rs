use bridge_core::{
    deposit_refund_amount, resolve_deposit_hold, resolve_withdrawal_hold, Account, AccountingState,
    Amount, ApplyOutcome, BaseMintSnapshot, CoreError, DepositEvent, DepositHoldResolution,
    DepositId, DepositQuote, DepositRecord, DepositRequest, DepositState, FeeKind, HoldId,
    LedgerOperation, LedgerTransferIdentity, MintAuthorization, MintAuthorizationDomain,
    MintAuthorizationOrigin, MintAuthorizationRecord, MintExpiryEvidence, MintFinalizationEvidence,
    ReconciliationHoldRecord, ReconciliationHoldState, RequestReference, ReservePolicy, Settlement,
    TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId, WithdrawalRecord,
    WithdrawalState,
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
    DepositRecord::accept(DepositRequest {
        id: DepositId::new([1; 32]),
        payload_hash: [2; 32],
        gross_amount: Amount::new(110),
        user_max_service_fee: Amount::new(10),
        transfer: transfer(LedgerOperation::PullDeposit, 110, 1, 10),
    })
    .expect("valid deposit")
}

fn test_deposit_quote() -> DepositQuote {
    DepositQuote {
        service_fee: Amount::new(10),
        net_amount: Amount::new(100),
    }
}

fn refund_identity(
    deposit: &DepositRecord,
    created_at_time_ns: u64,
    memo: [u8; 32],
) -> LedgerTransferIdentity {
    LedgerTransferIdentity {
        operation: LedgerOperation::RefundDeposit,
        created_at_time_ns,
        memo,
        amount: Amount::new(
            deposit_refund_amount(deposit.gross_amount.get(), 10).expect("valid refund"),
        ),
        fee: Amount::new(10),
        from: deposit.transfer.to.clone(),
        to: deposit.transfer.from.clone(),
        spender: None,
    }
}

fn authorization_record(deposit: &DepositRecord) -> MintAuthorizationRecord {
    let deadline = MintAuthorization::deadline_from_finalized_timestamp(1)
        .expect("valid test authorization deadline");
    MintAuthorizationRecord {
        authorization: MintAuthorization {
            deposit_id: deposit.id.bytes(),
            recipient: [3; 20],
            gross_amount: deposit.gross_amount,
            max_service_fee: deposit.max_service_fee,
            charged_service_fee: Amount::new(10),
            deadline,
            authorization_epoch: 1,
        },
        domain: MintAuthorizationDomain::bridge(8453, [4; 20]),
        digest: [5; 32],
        origin: MintAuthorizationOrigin {
            finalized_block_number: 7,
            finalized_block_hash: [6; 32],
            finalized_block_timestamp: 1,
        },
        signature_dispatch_attempt: 0,
        signature_dispatched: false,
        signature: None,
    }
}

fn expiry_evidence(timestamp: u64) -> MintExpiryEvidence {
    MintExpiryEvidence {
        authorization_digest: [5; 32],
        chain_id: 8453,
        finalized_block_number: 8,
        finalized_block_hash: [7; 32],
        finalized_block_timestamp: timestamp,
        bridge_signer: [8; 20],
        mint_authorization_epoch: 2,
        runtime_sha256: [9; 32],
        rpc_request_digest: [10; 32],
        rpc_response_digest: [11; 32],
    }
}

fn finalization_evidence() -> MintFinalizationEvidence {
    MintFinalizationEvidence {
        authorization_digest: [5; 32],
        chain_id: 8453,
        transaction_hash: [7; 32],
        receipt_block_number: 8,
        receipt_block_hash: [8; 32],
        finalized_block_number: 9,
        finalized_block_hash: [9; 32],
        rpc_request_digest: [10; 32],
        rpc_response_digest: [11; 32],
    }
}

#[test]
fn minted_state_requires_and_persists_exact_canonical_evidence() {
    let mut deposit = accepted_deposit();
    deposit
        .apply(DepositEvent::FundingSucceeded {
            ledger_block_index: 1,
        })
        .expect("escrow");
    deposit
        .apply(DepositEvent::CommitAuthorization {
            quote: test_deposit_quote(),
            authorization: Box::new(authorization_record(&deposit)),
        })
        .expect("authorization");
    deposit
        .mint_authorization
        .as_mut()
        .expect("authorization record")
        .dispatch_signature()
        .expect("dispatch");
    deposit
        .apply(DepositEvent::AuthorizationSigned {
            signature: vec![12; 65],
        })
        .expect("signed");
    deposit
        .apply(DepositEvent::BeginExpiryReconciliation)
        .expect("reconciliation");

    let mut invalid = finalization_evidence();
    invalid.rpc_response_digest = [0; 32];
    let snapshot = deposit.clone();
    assert_eq!(
        deposit.apply(DepositEvent::MintReconciled {
            evidence: Box::new(invalid),
        }),
        Err(CoreError::ConflictingReplay)
    );
    assert_eq!(deposit, snapshot);

    let evidence = finalization_evidence();
    let event = DepositEvent::MintReconciled {
        evidence: Box::new(evidence.clone()),
    };
    assert_eq!(
        deposit.apply(event.clone()).expect("mint proof").outcome,
        ApplyOutcome::Applied
    );
    assert_eq!(
        deposit.apply(event).expect("exact replay").outcome,
        ApplyOutcome::Idempotent
    );
    assert_eq!(deposit.mint_finalization_evidence, Some(evidence));
    assert!(matches!(deposit.state, DepositState::Minted { .. }));
}

#[test]
fn expired_authorization_refund_requires_persisted_finalized_unprocessed_evidence() {
    let mut deposit = accepted_deposit();
    deposit
        .apply(DepositEvent::FundingSucceeded {
            ledger_block_index: 1,
        })
        .expect("escrow");
    deposit
        .apply(DepositEvent::CommitAuthorization {
            quote: test_deposit_quote(),
            authorization: Box::new(authorization_record(&deposit)),
        })
        .expect("authorization");
    deposit
        .mint_authorization
        .as_mut()
        .expect("authorization record")
        .dispatch_signature()
        .expect("dispatch");
    deposit
        .apply(DepositEvent::AuthorizationSigned {
            signature: vec![12; 65],
        })
        .expect("signed");
    deposit
        .apply(DepositEvent::BeginExpiryReconciliation)
        .expect("expiry reconciliation");

    let refund = TransferAttempt {
        attempt_no: 0,
        identity: refund_identity(&deposit, 100, [13; 32]),
    };
    let snapshot = deposit.clone();
    assert_eq!(
        deposit.apply(DepositEvent::StartRefund {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            attempt: Box::new(refund.clone()),
            expiry_evidence: Some(Box::new(expiry_evidence(
                MintAuthorization::deadline_from_finalized_timestamp(1)
                    .expect("valid deadline boundary"),
            ))),
        }),
        Err(CoreError::RefundIneligible)
    );
    assert_eq!(deposit, snapshot);

    let post_deadline = MintAuthorization::deadline_from_finalized_timestamp(1)
        .and_then(|deadline| deadline.checked_add(1))
        .expect("valid post-deadline timestamp");
    let mut wrong_digest = expiry_evidence(post_deadline);
    wrong_digest.authorization_digest = [0xff; 32];
    assert_eq!(
        deposit.apply(DepositEvent::StartRefund {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            attempt: Box::new(refund.clone()),
            expiry_evidence: Some(Box::new(wrong_digest)),
        }),
        Err(CoreError::RefundIneligible)
    );

    let evidence = expiry_evidence(post_deadline);
    deposit
        .apply(DepositEvent::StartRefund {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            attempt: Box::new(refund),
            expiry_evidence: Some(Box::new(evidence.clone())),
        })
        .expect("strictly post-deadline refund");
    assert_eq!(deposit.mint_expiry_evidence, Some(evidence));
    assert!(matches!(
        deposit.state,
        DepositState::RefundPending {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            ..
        }
    ));
}

#[test]
fn expired_pending_authorization_does_not_require_a_late_signature_to_refund() {
    let mut deposit = accepted_deposit();
    deposit
        .apply(DepositEvent::FundingSucceeded {
            ledger_block_index: 1,
        })
        .expect("escrow");
    deposit
        .apply(DepositEvent::CommitAuthorization {
            quote: test_deposit_quote(),
            authorization: Box::new(authorization_record(&deposit)),
        })
        .expect("pending authorization");
    assert!(deposit
        .mint_authorization
        .as_ref()
        .expect("authorization")
        .signature
        .is_none());

    deposit
        .apply(DepositEvent::BeginExpiryReconciliation)
        .expect("expired pending authorization");
    deposit
        .apply(DepositEvent::StartRefund {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            attempt: Box::new(TransferAttempt {
                attempt_no: 0,
                identity: refund_identity(&deposit, 100, [14; 32]),
            }),
            expiry_evidence: Some(Box::new(expiry_evidence(
                MintAuthorization::deadline_from_finalized_timestamp(1)
                    .and_then(|deadline| deadline.checked_add(1))
                    .expect("valid post-deadline timestamp"),
            ))),
        })
        .expect("evidence-bound refund without signature");
    assert!(matches!(
        deposit.state,
        DepositState::RefundPending {
            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
            ..
        }
    ));
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
    assert_eq!(deposit_refund_amount(10_000, 10_000), None);
    assert_eq!(deposit_refund_amount(10_001, 10_000), Some(1));

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
fn definitive_pull_failure_cancels_and_releases_the_deposit_path() {
    let mut deposit = accepted_deposit();
    let failure = bridge_core::LedgerFailure::InsufficientAllowance {
        allowance: Amount::ZERO,
    };
    let event = DepositEvent::FundingFailed { code: failure };
    assert_eq!(
        deposit.apply(event.clone()).expect("cancel").outcome,
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
        deposit.apply(DepositEvent::FundingSucceeded {
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
fn settlement_reserve_uses_eth_only_for_governance_and_cycles_for_all_work() {
    let policy = ReservePolicy {
        governance_eth_floor_wei: 100,
        cycles_floor: 200,
        settlement_cycle_ceiling: 30,
    };
    assert_eq!(policy.required_cycles(2, 0, 0), Ok(260));
    let exact = policy.snapshot(2, 0, 0, 100, 260).expect("exact reserve");
    assert!(exact.sufficient);
    assert_eq!(exact.required_eth_wei, 100);
    assert_eq!(exact.required_cycles, 260);
    assert!(
        !policy
            .snapshot(2, 0, 0, 99, 260)
            .expect("low ETH")
            .sufficient
    );
    assert!(
        !policy
            .snapshot(2, 0, 0, 100, 259)
            .expect("low cycles")
            .sufficient
    );
    let candidate = policy
        .snapshot(1, 0, 1, 100, 260)
        .expect("candidate reservation");
    assert_eq!(candidate.reserved_operation_count, 2);
    let existing_withdrawal = policy
        .snapshot(1, 0, 0, 100, 230)
        .expect("existing withdrawal reserve");
    assert!(existing_withdrawal.sufficient);
    let competing_deposit = policy
        .snapshot(1, 0, 1, 100, 230)
        .expect("competing deposit reserve");
    assert!(!competing_deposit.sufficient);
    assert_eq!(competing_deposit.nonterminal_withdrawals, 1);
    assert_eq!(competing_deposit.candidate_deposits, 1);
    let overflow = ReservePolicy {
        cycles_floor: u128::MAX,
        ..policy
    };
    assert_eq!(
        overflow.snapshot(1, 0, 0, u128::MAX, u128::MAX),
        Err(CoreError::ArithmeticOverflow)
    );
}

#[test]
fn reconciliation_resolution_is_evidence_typed_and_terminal() {
    let mut deposit = accepted_deposit();
    let hold_id = HoldId::new(1);
    deposit
        .apply(DepositEvent::FundingAmbiguous { hold_id })
        .expect("hold deposit");
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::DepositFunding(deposit.id),
        deposit.transfer.clone(),
    );
    let resolution = DepositHoldResolution::FundingAbsent {
        history_watermark: 100,
    };
    assert_eq!(
        resolve_deposit_hold(&mut deposit, &mut hold, resolution.clone())
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
            DepositHoldResolution::FundingSucceeded {
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
        .apply(DepositEvent::FundingAmbiguous { hold_id })
        .expect("hold deposit");
    let mut hold = ReconciliationHoldRecord::open(
        hold_id,
        RequestReference::DepositFunding(deposit.id),
        deposit.transfer.clone(),
    );
    resolve_deposit_hold(
        &mut deposit,
        &mut hold,
        DepositHoldResolution::FundingAbsent {
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
        deposit.apply(DepositEvent::FundingSucceeded {
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
