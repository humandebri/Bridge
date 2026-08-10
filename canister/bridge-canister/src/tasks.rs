use crate::{
    evm_rpc, ledger,
    phases::{DepositPhase, SettlementState, WithdrawalPhase},
    signer, storage_or_trap, STORE,
};
use bridge_core::{
    Amount, DepositEvent, DepositHoldResolution, DepositQuote, DepositRefundReason,
    LedgerCallOutcome, LedgerOperation, LedgerTransferIdentity, ReconciliationHoldRecord,
    ReconciliationScanProgress, ReconciliationTarget, RequestReference, TransferAttempt,
    WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId, WithdrawalState,
};
use candid::{CandidType, Deserialize};
use sha2::{Digest, Sha256};

fn retry_memo(domain: &[u8], hold_id: u64, identity: &LedgerTransferIdentity) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(hold_id.to_be_bytes());
    digest.update(identity.created_at_time_ns.to_be_bytes());
    digest.finalize().into()
}

fn deposit_refund_memo(deposit_id: [u8; 32], attempt_no: u64) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"KINIC-DEPOSIT-REFUND");
    digest.update(deposit_id);
    digest.update(attempt_no.to_be_bytes());
    digest.finalize().into()
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettlementStopReason {
    LedgerUnavailable,
    LedgerAmbiguous,
    LedgerRejected(String),
    RpcUnavailable,
    RpcInconsistent,
    InvalidBaseResponse,
    SigningUnavailable,
    BaseStateMismatch,
    BridgeSignerMismatch,
    LedgerFeeExceedsServiceFee,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettlementActionResult {
    Complete {
        state: SettlementState,
    },
    ReconciliationProgress {
        state: SettlementState,
    },
    Deferred {
        state: SettlementState,
        next_run_at_ns: u64,
    },
    Stopped {
        state: SettlementState,
        reason: SettlementStopReason,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FeePayoutActionResult {
    Complete {
        state: crate::admin::FeePayoutState,
    },
    ReconciliationProgress {
        state: crate::admin::FeePayoutState,
    },
    Stopped {
        state: crate::admin::FeePayoutState,
        reason: SettlementStopReason,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettlementActionError {
    AnonymousCaller,
    InvalidId,
    NotFound,
    Unauthorized,
    Busy,
    StorageFailure,
    WrongState,
    AutomaticProgressPending { next_run_at_ns: Option<u64> },
    RateLimited { retry_after_seconds: u64 },
    InsufficientCycles,
}

pub(crate) fn stop_reason_text(result: &SettlementActionResult) -> Option<String> {
    match result {
        SettlementActionResult::Stopped { reason, .. } => Some(match reason {
            SettlementStopReason::LedgerUnavailable => "Ledger unavailable".into(),
            SettlementStopReason::LedgerAmbiguous => "Ledger result is ambiguous".into(),
            SettlementStopReason::LedgerRejected(message) => {
                format!("Ledger rejected the transfer: {message}")
            }
            SettlementStopReason::RpcUnavailable => "Base RPC unavailable".into(),
            SettlementStopReason::RpcInconsistent => "Base RPC providers disagreed".into(),
            SettlementStopReason::InvalidBaseResponse => "Invalid Base response".into(),
            SettlementStopReason::SigningUnavailable => "Threshold signing unavailable".into(),
            SettlementStopReason::BaseStateMismatch => {
                "Confirmed Base withdrawal state does not match the creation receipt".into()
            }
            SettlementStopReason::BridgeSignerMismatch => {
                "Confirmed Base bridge signer does not match the chain-key signer".into()
            }
            SettlementStopReason::LedgerFeeExceedsServiceFee => {
                "Ledger fee exceeds the charged withdrawal service fee".into()
            }
        }),
        _ => None,
    }
}

const LEDGER_DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

fn prepared_release_fee_matches_configured(prepared: u128, configured: u128) -> bool {
    prepared == configured
}

fn withdrawal_hold_step_requires_new_call(phase: WithdrawalPhase) -> bool {
    matches!(phase, WithdrawalPhase::ReleasePending)
}

fn resolve_reconciliation_success(
    config: &crate::config::BridgeInitArgs,
    target: ReconciliationTarget,
    block_index: u128,
) {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let scan_target = target.clone();
        match target {
            ReconciliationTarget::Hold(hold_id) => {
                let hold = storage_or_trap(
                    "reconciliation hold read",
                    store.reconciliation_hold(hold_id.get()),
                )
                .unwrap_or_else(|| ic_cdk::trap("missing reconciliation hold"));
                match hold.request {
                    RequestReference::DepositFunding(id) => advance_deposit_hold(
                        &mut store,
                        id,
                        hold_id,
                        DepositHoldResolution::FundingSucceeded {
                            funding_ledger_block_index: block_index,
                        },
                        Some(&scan_target),
                    ),
                    RequestReference::DepositRefund(id) => advance_deposit_hold(
                        &mut store,
                        id,
                        hold_id,
                        DepositHoldResolution::RefundSucceeded {
                            refund_ledger_block_index: block_index,
                        },
                        Some(&scan_target),
                    ),
                    RequestReference::Withdrawal(id) => advance_withdrawal_hold(
                        &mut store,
                        config,
                        id,
                        hold_id,
                        WithdrawalHoldResolution::Succeeded {
                            release_ledger_block_index: block_index,
                        },
                        Some(&scan_target),
                    ),
                }
            }
            ReconciliationTarget::FeePayout(id) => storage_or_trap(
                "fee payout completion",
                store.complete_fee_payout_success_and_scan(id, block_index, &scan_target),
            ),
            ReconciliationTarget::FundingAttempt(_) => {
                ic_cdk::trap("funding-attempt scans use the dedicated recovery lane")
            }
        }
    });
}

fn resolve_reconciliation_absence(
    config: &crate::config::BridgeInitArgs,
    target: ReconciliationTarget,
    transfer: LedgerTransferIdentity,
    history_watermark: u128,
) {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let scan_target = target.clone();
        match target {
            ReconciliationTarget::Hold(hold_id) => {
                let hold = storage_or_trap(
                    "reconciliation hold read",
                    store.reconciliation_hold(hold_id.get()),
                )
                .unwrap_or_else(|| ic_cdk::trap("missing reconciliation hold"));
                match hold.request {
                    RequestReference::DepositFunding(id) => advance_deposit_hold(
                        &mut store,
                        id,
                        hold_id,
                        DepositHoldResolution::FundingAbsent { history_watermark },
                        Some(&scan_target),
                    ),
                    RequestReference::DepositRefund(id) => {
                        let mut next_identity = transfer;
                        next_identity.created_at_time_ns = ic_cdk::api::time()
                            .max(next_identity.created_at_time_ns.saturating_add(1));
                        next_identity.memo = retry_memo(
                            b"KINIC-DEPOSIT-REFUND-RETRY",
                            hold_id.get(),
                            &next_identity,
                        );
                        advance_deposit_hold(
                            &mut store,
                            id,
                            hold_id,
                            DepositHoldResolution::RefundAbsent {
                                history_watermark,
                                next_identity: Box::new(next_identity),
                            },
                            Some(&scan_target),
                        );
                    }
                    RequestReference::Withdrawal(id) => {
                        let mut next_identity = transfer;
                        next_identity.created_at_time_ns = ic_cdk::api::time()
                            .max(next_identity.created_at_time_ns.saturating_add(1));
                        next_identity.memo =
                            retry_memo(b"KINIC-WITHDRAWAL-RETRY", hold_id.get(), &next_identity);
                        advance_withdrawal_hold(
                            &mut store,
                            config,
                            id,
                            hold_id,
                            WithdrawalHoldResolution::Absent {
                                history_watermark,
                                next_identity: Box::new(next_identity),
                            },
                            Some(&scan_target),
                        );
                    }
                }
            }
            ReconciliationTarget::FeePayout(id) => storage_or_trap(
                "failed fee payout persistence",
                store.complete_fee_payout_failure_and_scan(id, &scan_target),
            ),
            ReconciliationTarget::FundingAttempt(_) => {
                ic_cdk::trap("funding-attempt scans use the dedicated recovery lane")
            }
        }
    });
}

fn advance_deposit_hold(
    store: &mut crate::storage::StableStore,
    deposit_id: bridge_core::DepositId,
    hold_id: bridge_core::HoldId,
    resolution: DepositHoldResolution,
    scan_target: Option<&ReconciliationTarget>,
) {
    store
        .resolve_deposit_hold_and_scan(deposit_id, hold_id, resolution, scan_target)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!(
                "deposit reconciliation persistence failed: {error}"
            ))
        });
}

fn advance_withdrawal_hold(
    store: &mut crate::storage::StableStore,
    _config: &crate::config::BridgeInitArgs,
    withdrawal_id: WithdrawalId,
    hold_id: bridge_core::HoldId,
    resolution: WithdrawalHoldResolution,
    scan_target: Option<&ReconciliationTarget>,
) {
    store
        .resolve_withdrawal_hold_and_scan(withdrawal_id, hold_id, resolution, scan_target)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!(
                "withdrawal reconciliation persistence failed: {error}"
            ))
        });
}

enum HoldAdvance {
    Continue,
    Progress,
    Stopped(SettlementStopReason),
}

fn ledger_stop(outcome: &LedgerCallOutcome) -> SettlementStopReason {
    match outcome {
        LedgerCallOutcome::Ambiguous => SettlementStopReason::LedgerAmbiguous,
        LedgerCallOutcome::DefinitiveFailure { code } => {
            SettlementStopReason::LedgerRejected(format!("{code:?}"))
        }
        LedgerCallOutcome::RetryableFailure { .. } => SettlementStopReason::LedgerUnavailable,
        LedgerCallOutcome::Succeeded { .. } | LedgerCallOutcome::Duplicate { .. } => {
            unreachable!("confirmed outcomes are handled before stop mapping")
        }
    }
}

async fn advance_hold(
    config: &crate::config::BridgeInitArgs,
    hold: ReconciliationHoldRecord,
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<HoldAdvance, SettlementActionError> {
    if ic_cdk::api::time().saturating_sub(hold.transfer.created_at_time_ns) <= LEDGER_DEDUP_NS {
        lease.renew_before_external_call()?;
        let outcome = match hold.transfer.operation {
            LedgerOperation::PullDeposit => {
                ledger::pull(config.ledger_canister_id, &hold.transfer).await
            }
            LedgerOperation::RefundDeposit => {
                ledger::refund(config.ledger_canister_id, &hold.transfer).await
            }
            LedgerOperation::ReleaseWithdrawal => {
                ledger::release(config.ledger_canister_id, &hold.transfer).await
            }
            LedgerOperation::FeePayout => {
                return Ok(HoldAdvance::Stopped(
                    SettlementStopReason::LedgerUnavailable,
                ));
            }
        };
        lease.ensure_current()?;
        let Some(block_index) = outcome.confirmed_block() else {
            return Ok(HoldAdvance::Stopped(ledger_stop(&outcome)));
        };
        STORE.with(|store| {
            let mut store = store.borrow_mut();
            match hold.request {
                RequestReference::DepositFunding(id) => advance_deposit_hold(
                    &mut store,
                    id,
                    hold.id,
                    DepositHoldResolution::FundingSucceeded {
                        funding_ledger_block_index: block_index,
                    },
                    None,
                ),
                RequestReference::DepositRefund(id) => advance_deposit_hold(
                    &mut store,
                    id,
                    hold.id,
                    DepositHoldResolution::RefundSucceeded {
                        refund_ledger_block_index: block_index,
                    },
                    None,
                ),
                RequestReference::Withdrawal(id) => advance_withdrawal_hold(
                    &mut store,
                    config,
                    id,
                    hold.id,
                    WithdrawalHoldResolution::Succeeded {
                        release_ledger_block_index: block_index,
                    },
                    None,
                ),
            }
        });
        return Ok(HoldAdvance::Continue);
    }

    let target = ReconciliationTarget::Hold(hold.id);
    let progress = STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(progress) = store
            .reconciliation_scan(&target)
            .map_err(|_| SettlementActionError::StorageFailure)?
        {
            return Ok(progress);
        }
        let progress = ReconciliationScanProgress::new(target.clone(), hold.transfer.clone());
        store
            .put_reconciliation_scan(&progress)
            .map_err(|_| SettlementActionError::StorageFailure)?;
        Ok(progress)
    })?;
    lease.renew_before_external_call()?;
    let outcome = ledger::reconcile_step(
        config.ledger_canister_id,
        config.index_canister_id,
        progress,
    )
    .await;
    lease.ensure_current()?;
    match outcome {
        ledger::ReconciliationOutcome::Progress(progress) => {
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .put_reconciliation_scan(&progress)
                    .map_err(|_| SettlementActionError::StorageFailure)
            })?;
            Ok(HoldAdvance::Progress)
        }
        ledger::ReconciliationOutcome::Succeeded { block_index } => {
            match bridge_core::hold_resolution_decision(true, false) {
                bridge_core::HoldResolutionDecision::ResolveSucceeded => {
                    resolve_reconciliation_success(config, target, block_index);
                    Ok(HoldAdvance::Continue)
                }
                _ => Ok(HoldAdvance::Progress),
            }
        }
        ledger::ReconciliationOutcome::Absent {
            ledger_watermark,
            index_watermark,
        } => {
            let complete_absence = index_watermark >= ledger_watermark;
            match bridge_core::hold_resolution_decision(false, complete_absence) {
                bridge_core::HoldResolutionDecision::ResolveAbsent => {
                    resolve_reconciliation_absence(config, target, hold.transfer, index_watermark);
                    Ok(HoldAdvance::Continue)
                }
                _ => Ok(HoldAdvance::Progress),
            }
        }
    }
}

enum EscrowPreparation {
    Authorization {
        quote: DepositQuote,
        authorization: Box<bridge_core::MintAuthorizationRecord>,
    },
    RefundAvailable(DepositRefundReason),
    Stopped(SettlementStopReason),
}

pub(crate) fn prepare_deposit_refund(
    deposit_id: [u8; 32],
    reason: DepositRefundReason,
    expiry_evidence: Option<bridge_core::MintExpiryEvidence>,
) -> Result<(bridge_core::DepositRecord, bridge_core::ApplyResult), SettlementActionError> {
    STORE.with(|store| {
        let store = store.borrow();
        let mut deposit = store
            .deposit(deposit_id)
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)?;
        let fee = ledger::KINIC_LEDGER_FEE;
        let charged_service_fee = if deposit
            .mint_authorization
            .as_ref()
            .is_some_and(|authorization| authorization.signature.is_some())
        {
            deposit
                .quote
                .map_or(Amount::ZERO, |quote| quote.service_fee)
        } else {
            Amount::ZERO
        };
        let amount = bridge_core::deposit_refund_amount(
            deposit.gross_amount.get(),
            charged_service_fee.get(),
            fee.get(),
        )
        .map(Amount::new)
        .ok_or(SettlementActionError::StorageFailure)?;
        let identity = LedgerTransferIdentity {
            operation: LedgerOperation::RefundDeposit,
            created_at_time_ns: ic_cdk::api::time(),
            memo: deposit_refund_memo(deposit_id, 0),
            amount,
            fee,
            from: deposit.transfer.to.clone(),
            to: deposit.transfer.from.clone(),
            spender: None,
        };
        let result = deposit
            .apply(DepositEvent::StartRefund {
                reason,
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity,
                }),
                expiry_evidence: expiry_evidence.map(Box::new),
            })
            .map_err(|_| SettlementActionError::StorageFailure)?;
        Ok((deposit, result))
    })
}

async fn prepare_escrowed_deposit(
    config: &crate::config::BridgeInitArgs,
    deposit: &bridge_core::DepositRecord,
) -> Result<EscrowPreparation, SettlementActionError> {
    let cached = crate::api::cached_authorization_observation(config, deposit.id.bytes())
        .map_err(|_| SettlementActionError::StorageFailure)?;
    let (finalized, observed_snapshot) = if let Some(observation) = cached {
        (observation.finalized, observation.snapshot)
    } else {
        let runtime_attested = crate::api::runtime_attested(config)
            .map_err(|_| SettlementActionError::StorageFailure)?;
        let observation = match evm_rpc::bridge_snapshot(config, runtime_attested).await {
            Ok(observation) => observation,
            Err(evm_rpc::ObservationError::Inconsistent) => {
                return Ok(EscrowPreparation::Stopped(
                    SettlementStopReason::RpcInconsistent,
                ));
            }
            Err(_) => {
                return Ok(EscrowPreparation::Stopped(
                    SettlementStopReason::RpcUnavailable,
                ));
            }
        };
        crate::api::cache_runtime_attestation(config, &observation)
            .map_err(|_| SettlementActionError::StorageFailure)?;
        (observation.finalized, observation.snapshot)
    };
    let snapshot = observed_snapshot.mint;
    if observed_snapshot.deposits_paused {
        return Ok(EscrowPreparation::RefundAvailable(
            DepositRefundReason::BasePaused,
        ));
    }
    let expected_signer = crate::api::cached_signer_address(config)
        .await
        .map_err(|_| SettlementActionError::StorageFailure)?;
    if observed_snapshot.bridge_signer != expected_signer {
        return Ok(EscrowPreparation::Stopped(
            SettlementStopReason::BridgeSignerMismatch,
        ));
    }
    let net_amount = match snapshot.quote(deposit.gross_amount, deposit.max_service_fee) {
        Ok(amount) => amount,
        Err(
            bridge_core::CoreError::ServiceFeeAboveMaximum
            | bridge_core::CoreError::ServiceFeeAboveUserMaximum
            | bridge_core::CoreError::InvalidAmount
            | bridge_core::CoreError::ArithmeticUnderflow,
        ) => {
            return Ok(EscrowPreparation::RefundAvailable(
                DepositRefundReason::ServiceFeeRejected,
            ));
        }
        Err(bridge_core::CoreError::PerDepositLimitExceeded) => {
            return Ok(EscrowPreparation::RefundAvailable(
                DepositRefundReason::PerDepositLimitExceeded,
            ));
        }
        Err(bridge_core::CoreError::MintWindowLimitExceeded) => {
            return Ok(EscrowPreparation::RefundAvailable(
                DepositRefundReason::MintWindowLimitExceeded,
            ));
        }
        Err(_) => return Err(SettlementActionError::StorageFailure),
    };
    let quote = DepositQuote {
        service_fee: snapshot.service_fee,
        net_amount,
    };
    let deadline = bridge_core::MintAuthorization::deadline_from_finalized_timestamp(
        snapshot.confirmed_block_timestamp,
    )
    .ok_or(SettlementActionError::StorageFailure)?;
    let authorization = bridge_core::MintAuthorization {
        deposit_id: deposit.id.bytes(),
        recipient: STORE.with(|store| {
            store
                .borrow()
                .deposit_intent(deposit.id.bytes())
                .map_err(|_| SettlementActionError::StorageFailure)?
                .map(|intent| intent.base_recipient)
                .ok_or(SettlementActionError::StorageFailure)
        })?,
        gross_amount: deposit.gross_amount,
        max_service_fee: deposit.max_service_fee,
        charged_service_fee: quote.service_fee,
        deadline,
        authorization_epoch: observed_snapshot.mint_authorization_epoch,
    };
    let domain = bridge_core::MintAuthorizationDomain::bridge(
        config.base_chain_id,
        config
            .bridge_contract
            .as_slice()
            .try_into()
            .map_err(|_| SettlementActionError::StorageFailure)?,
    );
    let authorization = bridge_core::MintAuthorizationRecord {
        digest: crate::mint_authorization::digest(&domain, authorization),
        authorization,
        domain,
        origin: bridge_core::MintAuthorizationOrigin {
            finalized_block_number: finalized.block_number,
            finalized_block_hash: finalized.block_hash,
            finalized_block_timestamp: snapshot.confirmed_block_timestamp,
        },
        signature_dispatch_attempt: 0,
        signature_dispatched: false,
        signature: None,
    };
    Ok(EscrowPreparation::Authorization {
        quote,
        authorization: Box::new(authorization),
    })
}

pub(crate) async fn advance_deposit(
    deposit_id: [u8; 32],
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<SettlementActionResult, SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::StorageFailure)
    })?;
    loop {
        let deposit = STORE.with(|store| {
            store
                .borrow()
                .deposit(deposit_id)
                .map_err(|_| SettlementActionError::StorageFailure)?
                .ok_or(SettlementActionError::NotFound)
        })?;
        let state = SettlementState::Deposit(DepositPhase::from(&deposit.state));
        match deposit.state {
            bridge_core::DepositState::FundingPending => {
                let callback_token = crate::storage::SettlementCallbackToken::for_deposit(
                    lease.job(),
                    &deposit.transfer,
                )
                .map_err(|_| SettlementActionError::StorageFailure)?;
                lease.renew_before_external_call()?;
                let outcome = ledger::pull(config.ledger_canister_id, &deposit.transfer).await;
                lease.ensure_current()?;
                match outcome {
                    LedgerCallOutcome::Succeeded { block_index }
                    | LedgerCallOutcome::Duplicate { block_index } => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut current = store
                                .deposit(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let result = current
                                .apply(DepositEvent::FundingSucceeded {
                                    funding_ledger_block_index: block_index,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_deposit_transition_funding_callback(
                                    &current,
                                    &callback_token,
                                    result,
                                )
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                    }
                    LedgerCallOutcome::Ambiguous => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let hold_id = store
                                .next_hold_id()
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let mut current = store
                                .deposit(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let result = current
                                .apply(DepositEvent::FundingAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            if result.deposit_effects
                                != Some(bridge_core::DepositAccountingEffects::ZERO)
                            {
                                return Err(SettlementActionError::StorageFailure);
                            }
                            let hold = ReconciliationHoldRecord::open(
                                hold_id,
                                RequestReference::DepositFunding(current.id),
                                current.transfer.clone(),
                            );
                            store
                                .commit_deposit_funding_hold_bundle(
                                    &current,
                                    &hold,
                                    &callback_token,
                                )
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: SettlementState::Deposit(
                                DepositPhase::FundingReconciliationHold,
                            ),
                            reason: SettlementStopReason::LedgerAmbiguous,
                        });
                    }
                    LedgerCallOutcome::DefinitiveFailure { code } => {
                        STORE.with(|store| {
                            crate::api::cancel_deposit_in_store(
                                &mut store.borrow_mut(),
                                deposit_id,
                                code,
                                &callback_token,
                            )
                            .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Complete {
                            state: SettlementState::Deposit(DepositPhase::Cancelled),
                        });
                    }
                    other => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: ledger_stop(&other),
                        });
                    }
                }
            }
            bridge_core::DepositState::EscrowedUnquoted { .. } => {
                lease.renew_before_external_call()?;
                let prepared = prepare_escrowed_deposit(&config, &deposit).await?;
                lease.ensure_current()?;
                match prepared {
                    EscrowPreparation::Authorization {
                        quote,
                        authorization,
                    } => {
                        let result = STORE.with(|store| {
                            crate::api::commit_deposit_authorization(
                                &mut store.borrow_mut(),
                                deposit_id,
                                quote,
                                *authorization,
                            )
                        });
                        match result {
                            Ok(()) => {}
                            Err(crate::api::DepositError::BaseObservationUnavailable) => {
                                return Ok(SettlementActionResult::Stopped {
                                    state,
                                    reason: SettlementStopReason::RpcUnavailable,
                                });
                            }
                            Err(_) => return Err(SettlementActionError::StorageFailure),
                        }
                    }
                    EscrowPreparation::RefundAvailable(reason) => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut current = store
                                .deposit(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let result = current
                                .apply(DepositEvent::MarkRefundAvailable {
                                    reason,
                                    finalized_timestamp: None,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_deposit_transition(&current, result)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Complete {
                            state: SettlementState::Deposit(DepositPhase::RefundAvailable),
                        });
                    }
                    EscrowPreparation::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason });
                    }
                }
            }
            bridge_core::DepositState::AuthorizationPending { .. } => {
                let digest = STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .deposit(deposit_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    let authorization = current
                        .mint_authorization
                        .as_mut()
                        .ok_or(SettlementActionError::StorageFailure)?;
                    authorization
                        .dispatch_signature()
                        .ok_or(SettlementActionError::StorageFailure)?;
                    let digest = authorization.digest;
                    store
                        .put_deposit(&current)
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    Ok::<_, SettlementActionError>(digest)
                })?;
                lease.renew_before_external_call()?;
                let expected_signer = crate::api::cached_signer_address(&config)
                    .await
                    .map_err(|_| SettlementActionError::StorageFailure)?;
                let signature = match signer::sign_mint_authorization_digest(digest, &config).await
                {
                    Ok(signature) => signature,
                    Err(_) => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: SettlementStopReason::SigningUnavailable,
                        });
                    }
                };
                lease.ensure_current()?;
                if !matches!(
                    signer::recover_ethereum_address(digest, &signature),
                    Ok(recovered) if recovered == expected_signer
                ) {
                    return Ok(SettlementActionResult::Stopped {
                        state,
                        reason: SettlementStopReason::BridgeSignerMismatch,
                    });
                }
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .deposit(deposit_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    let result = current
                        .apply(DepositEvent::AuthorizationSigned { signature })
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    store
                        .put_deposit_transition(&current, result)
                        .map_err(|_| SettlementActionError::StorageFailure)
                })?;
                return Ok(SettlementActionResult::Complete {
                    state: SettlementState::Deposit(DepositPhase::AuthorizationAvailable),
                });
            }
            bridge_core::DepositState::AuthorizationAvailable { .. } => {
                return Ok(SettlementActionResult::Complete { state });
            }
            bridge_core::DepositState::RefundAvailable { .. } => {
                return Ok(SettlementActionResult::Complete { state });
            }
            bridge_core::DepositState::FundingReconciliationHold { hold_id } => {
                let hold = STORE.with(|store| {
                    store
                        .borrow()
                        .reconciliation_hold(hold_id.get())
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)
                })?;
                match advance_hold(&config, hold, lease).await? {
                    HoldAdvance::Continue => continue,
                    HoldAdvance::Progress => {
                        return Ok(SettlementActionResult::ReconciliationProgress { state });
                    }
                    HoldAdvance::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason });
                    }
                }
            }
            bridge_core::DepositState::RefundReconciliationHold { hold_id, .. } => {
                let hold = STORE.with(|store| {
                    store
                        .borrow()
                        .reconciliation_hold(hold_id.get())
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)
                })?;
                match advance_hold(&config, hold, lease).await? {
                    HoldAdvance::Continue => continue,
                    HoldAdvance::Progress | HoldAdvance::Stopped(_) => {
                        return Ok(SettlementActionResult::Complete { state });
                    }
                }
            }
            bridge_core::DepositState::RefundPending { attempt, .. } => {
                lease.renew_before_external_call()?;
                let outcome = ledger::refund(config.ledger_canister_id, &attempt.identity).await;
                lease.ensure_current()?;
                match outcome {
                    LedgerCallOutcome::Succeeded { block_index }
                    | LedgerCallOutcome::Duplicate { block_index } => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut current = store
                                .deposit(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let result = current
                                .apply(DepositEvent::RefundSucceeded {
                                    refund_ledger_block_index: block_index,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_deposit_transition(&current, result)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                    }
                    LedgerCallOutcome::Ambiguous => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let hold_id = store
                                .next_hold_id()
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let mut current = store
                                .deposit(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let result = current
                                .apply(DepositEvent::RefundAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            if result.deposit_effects
                                != Some(bridge_core::DepositAccountingEffects::ZERO)
                            {
                                return Err(SettlementActionError::StorageFailure);
                            }
                            let hold = ReconciliationHoldRecord::open(
                                hold_id,
                                RequestReference::DepositRefund(current.id),
                                attempt.identity.clone(),
                            );
                            store
                                .commit_deposit_hold_bundle(&current, &hold)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Complete {
                            state: SettlementState::Deposit(DepositPhase::RefundProcessing),
                        });
                    }
                    LedgerCallOutcome::DefinitiveFailure { code }
                    | LedgerCallOutcome::RetryableFailure { code } => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: SettlementStopReason::LedgerRejected(format!("{code:?}")),
                        });
                    }
                }
            }
            bridge_core::DepositState::Minted { .. }
            | bridge_core::DepositState::Refunded { .. }
            | bridge_core::DepositState::Cancelled { .. } => {
                return Ok(SettlementActionResult::Complete { state });
            }
        }
    }
}

pub(crate) async fn advance_withdrawal(
    withdrawal_id: [u8; 32],
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<SettlementActionResult, SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::StorageFailure)
    })?;
    loop {
        let withdrawal = STORE.with(|store| {
            store
                .borrow()
                .withdrawal(withdrawal_id)
                .map_err(|_| SettlementActionError::StorageFailure)?
                .ok_or(SettlementActionError::NotFound)
        })?;
        let state = SettlementState::Withdrawal(WithdrawalPhase::from(&withdrawal.state));
        match withdrawal.state {
            WithdrawalState::ReleasePending { attempt, .. } => {
                let live_ledger_fee = ledger::KINIC_LEDGER_FEE;
                if !prepared_release_fee_matches_configured(
                    attempt.identity.fee.get(),
                    live_ledger_fee.get(),
                ) {
                    return Ok(SettlementActionResult::Stopped {
                        state,
                        reason: SettlementStopReason::LedgerRejected(format!(
                            "ledger fee changed after release preparation (prepared {}, live {}); drain pending withdrawals before the managed fee update",
                            attempt.identity.fee.get(),
                            live_ledger_fee.get(),
                        )),
                    });
                }
                lease.renew_before_external_call()?;
                let outcome = ledger::release(config.ledger_canister_id, &attempt.identity).await;
                lease.ensure_current()?;
                match outcome {
                    LedgerCallOutcome::Succeeded { block_index }
                    | LedgerCallOutcome::Duplicate { block_index } => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut current = store
                                .withdrawal(withdrawal_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            current
                                .apply(WithdrawalEvent::ReleaseSucceeded {
                                    release_ledger_block_index: block_index,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_withdrawal(&current)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                    }
                    LedgerCallOutcome::Ambiguous => {
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let hold_id = store
                                .next_hold_id()
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let mut current = store
                                .withdrawal(withdrawal_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            current
                                .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let hold = ReconciliationHoldRecord::open(
                                hold_id,
                                RequestReference::Withdrawal(current.id),
                                attempt.identity,
                            );
                            store
                                .commit_withdrawal_hold_bundle(&current, &hold)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: SettlementState::Withdrawal(WithdrawalPhase::ReconciliationHold),
                            reason: SettlementStopReason::LedgerAmbiguous,
                        });
                    }
                    other => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: ledger_stop(&other),
                        });
                    }
                }
            }
            WithdrawalState::ReconciliationHold { hold_id, .. } => {
                let hold = STORE.with(|store| {
                    store
                        .borrow()
                        .reconciliation_hold(hold_id.get())
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)
                })?;
                match advance_hold(&config, hold, lease).await? {
                    HoldAdvance::Continue => {
                        let current = STORE.with(|store| {
                            store
                                .borrow()
                                .withdrawal(withdrawal_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)
                        })?;
                        let phase = WithdrawalPhase::from(&current.state);
                        if withdrawal_hold_step_requires_new_call(phase) {
                            return Ok(SettlementActionResult::ReconciliationProgress {
                                state: SettlementState::Withdrawal(phase),
                            });
                        }
                        continue;
                    }
                    HoldAdvance::Progress => {
                        return Ok(SettlementActionResult::ReconciliationProgress { state });
                    }
                    HoldAdvance::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason });
                    }
                }
            }
            WithdrawalState::Paid { .. } => return Ok(SettlementActionResult::Complete { state }),
            WithdrawalState::Observed => {
                let ledger_fee = ledger::KINIC_LEDGER_FEE;
                if ledger_fee.get() > withdrawal.charged_service_fee.get() {
                    STORE.with(|store| {
                        let mut store = store.borrow_mut();
                        let mut current = store
                            .withdrawal(withdrawal_id)
                            .map_err(|_| SettlementActionError::StorageFailure)?
                            .ok_or(SettlementActionError::NotFound)?;
                        current.last_settlement_stop_reason =
                            Some("LedgerFeeExceedsServiceFee".to_owned());
                        let mut admin = store
                            .admin_state()
                            .map_err(|_| SettlementActionError::StorageFailure)?;
                        let now_ns = ic_cdk::api::time();
                        let guard_changed = admin.withdrawal_fee_guard.is_none_or(|guard| {
                            guard.ledger_fee != ledger_fee.get()
                                || guard.charged_service_fee != current.charged_service_fee.get()
                        });
                        admin.withdrawal_fee_guard = Some(if guard_changed {
                            crate::admin::WithdrawalFeeGuard {
                                ledger_fee: ledger_fee.get(),
                                charged_service_fee: current.charged_service_fee.get(),
                                tripped_at_ns: now_ns,
                            }
                        } else {
                            admin.withdrawal_fee_guard.expect("checked fee guard")
                        });
                        let audit = guard_changed
                            .then(
                                || crate::storage::AuditEventKind::WithdrawalFeeGuardTripped {
                                    ledger_fee: ledger_fee.get(),
                                    charged_service_fee: current.charged_service_fee.get(),
                                },
                            )
                            .into_iter()
                            .collect();
                        store
                            .commit_withdrawal_fee_guard_continue_bundle(
                                &current,
                                &admin,
                                ic_cdk::api::canister_self(),
                                now_ns,
                                audit,
                            )
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    return Ok(SettlementActionResult::Stopped {
                        state,
                        reason: SettlementStopReason::LedgerFeeExceedsServiceFee,
                    });
                }
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .withdrawal(withdrawal_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    let settlement = bridge_core::Settlement {
                        amount_out: current.amount_out,
                        service_fee: current.charged_service_fee,
                        ledger_fee,
                    };
                    let transfer = LedgerTransferIdentity {
                        operation: LedgerOperation::ReleaseWithdrawal,
                        created_at_time_ns: ic_cdk::api::time(),
                        memo: current.payload_hash,
                        amount: current.amount_out,
                        fee: ledger_fee,
                        from: bridge_core::Account::new(
                            ic_cdk::api::canister_self().as_slice().to_vec(),
                            [0; 32],
                        )
                        .map_err(|_| SettlementActionError::StorageFailure)?,
                        to: bridge_core::Account::new(current.owner.clone(), current.subaccount)
                            .map_err(|_| SettlementActionError::StorageFailure)?,
                        spender: None,
                    };
                    current
                        .apply(WithdrawalEvent::StartRelease {
                            attempt: Box::new(TransferAttempt {
                                attempt_no: 0,
                                identity: transfer,
                            }),
                            settlement,
                        })
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    current.last_settlement_stop_reason = None;
                    let mut admin = store
                        .admin_state()
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    let cleared = admin.withdrawal_fee_guard.take().is_some();
                    store
                        .commit_withdrawal_fee_guard_clear_bundle(
                            &current,
                            &admin,
                            ic_cdk::api::canister_self(),
                            ic_cdk::api::time(),
                            cleared
                                .then_some(
                                    crate::storage::AuditEventKind::WithdrawalFeeGuardCleared,
                                )
                                .into_iter()
                                .collect(),
                        )
                        .map_err(|_| SettlementActionError::StorageFailure)
                })?;
            }
        }
    }
}

pub(crate) async fn advance_fee_payout(
    payout_id: u64,
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<FeePayoutActionResult, SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::StorageFailure)
    })?;
    let payout = STORE.with(|store| {
        store
            .borrow()
            .fee_payout(payout_id)
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)
    })?;
    match payout.state {
        crate::admin::FeePayoutState::Succeeded { .. } => Ok(FeePayoutActionResult::Complete {
            state: payout.state,
        }),
        crate::admin::FeePayoutState::Failed => Ok(FeePayoutActionResult::Complete {
            state: payout.state,
        }),
        crate::admin::FeePayoutState::Pending => {
            lease.renew_before_external_call()?;
            let outcome = ledger::release(config.ledger_canister_id, &payout.transfer).await;
            lease.ensure_current()?;
            match outcome {
                LedgerCallOutcome::Succeeded { block_index }
                | LedgerCallOutcome::Duplicate { block_index } => {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .complete_fee_payout_success(payout_id, block_index)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(FeePayoutActionResult::Complete {
                        state: crate::admin::FeePayoutState::Succeeded { block_index },
                    })
                }
                LedgerCallOutcome::Ambiguous => {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .hold_fee_payout(payout_id)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(FeePayoutActionResult::Stopped {
                        state: crate::admin::FeePayoutState::ReconciliationHold,
                        reason: SettlementStopReason::LedgerAmbiguous,
                    })
                }
                LedgerCallOutcome::DefinitiveFailure { code } => {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .complete_fee_payout_failure(payout_id)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(FeePayoutActionResult::Stopped {
                        state: crate::admin::FeePayoutState::Failed,
                        reason: SettlementStopReason::LedgerRejected(format!("{code:?}")),
                    })
                }
                other => Ok(FeePayoutActionResult::Stopped {
                    state: crate::admin::FeePayoutState::Pending,
                    reason: ledger_stop(&other),
                }),
            }
        }
        crate::admin::FeePayoutState::ReconciliationHold => {
            if ic_cdk::api::time().saturating_sub(payout.transfer.created_at_time_ns)
                <= LEDGER_DEDUP_NS
            {
                lease.renew_before_external_call()?;
                let outcome = ledger::release(config.ledger_canister_id, &payout.transfer).await;
                lease.ensure_current()?;
                if let Some(block_index) = outcome.confirmed_block() {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .complete_fee_payout_success(payout_id, block_index)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    return Ok(FeePayoutActionResult::Complete {
                        state: crate::admin::FeePayoutState::Succeeded { block_index },
                    });
                }
                if let LedgerCallOutcome::DefinitiveFailure { code } = &outcome {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .complete_fee_payout_failure(payout_id)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    return Ok(FeePayoutActionResult::Stopped {
                        state: crate::admin::FeePayoutState::Failed,
                        reason: SettlementStopReason::LedgerRejected(format!("{code:?}")),
                    });
                }
                return Ok(FeePayoutActionResult::Stopped {
                    state: crate::admin::FeePayoutState::ReconciliationHold,
                    reason: ledger_stop(&outcome),
                });
            }
            let target = ReconciliationTarget::FeePayout(payout_id);
            let progress = STORE.with(|store| {
                let mut store = store.borrow_mut();
                if let Some(progress) = store
                    .reconciliation_scan(&target)
                    .map_err(|_| SettlementActionError::StorageFailure)?
                {
                    return Ok(progress);
                }
                let progress =
                    ReconciliationScanProgress::new(target.clone(), payout.transfer.clone());
                store
                    .commit_fee_payout_scan(&progress)
                    .map_err(|_| SettlementActionError::StorageFailure)?;
                Ok(progress)
            })?;
            let reconciliation_input = progress.clone();
            lease.renew_before_external_call()?;
            match ledger::reconcile_step(
                config.ledger_canister_id,
                config.index_canister_id,
                progress,
            )
            .await
            {
                ledger::ReconciliationOutcome::Progress(progress) => {
                    lease.ensure_current()?;
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .update_fee_payout_scan(&reconciliation_input, &progress)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(FeePayoutActionResult::ReconciliationProgress {
                        state: crate::admin::FeePayoutState::ReconciliationHold,
                    })
                }
                ledger::ReconciliationOutcome::Succeeded { block_index } => {
                    lease.ensure_current()?;
                    resolve_reconciliation_success(&config, target, block_index);
                    Ok(FeePayoutActionResult::Complete {
                        state: crate::admin::FeePayoutState::Succeeded { block_index },
                    })
                }
                ledger::ReconciliationOutcome::Absent {
                    ledger_watermark,
                    index_watermark,
                } => {
                    lease.ensure_current()?;
                    if index_watermark < ledger_watermark {
                        return Ok(FeePayoutActionResult::ReconciliationProgress {
                            state: crate::admin::FeePayoutState::ReconciliationHold,
                        });
                    }
                    resolve_reconciliation_absence(
                        &config,
                        target,
                        payout.transfer,
                        index_watermark,
                    );
                    Ok(FeePayoutActionResult::Complete {
                        state: crate::admin::FeePayoutState::Failed,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_release_requires_the_configured_ledger_fee() {
        assert!(prepared_release_fee_matches_configured(10_000, 10_000));
        assert!(!prepared_release_fee_matches_configured(10_000, 20_000));
        assert!(!prepared_release_fee_matches_configured(20_000, 10_000));
    }

    #[test]
    fn complete_absence_stops_after_saving_the_new_release_identity() {
        assert!(withdrawal_hold_step_requires_new_call(
            WithdrawalPhase::ReleasePending
        ));
        assert!(!withdrawal_hold_step_requires_new_call(
            WithdrawalPhase::Paid
        ));
    }
}
