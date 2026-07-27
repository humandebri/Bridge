use crate::{
    evm_rpc, ledger,
    phases::{DepositPhase, SettlementState, WithdrawalPhase},
    signer, storage_or_trap, STORE,
};
use bridge_core::{
    Amount, DepositEvent, DepositHoldResolution, DepositQuote, DepositRefundReason,
    EvmOperationEvent, EvmOperationId, EvmOperationKind, EvmOperationState, LedgerCallOutcome,
    LedgerOperation, LedgerTransferIdentity, ReconciliationHoldRecord, ReconciliationScanProgress,
    ReconciliationTarget, RequestReference, TransferAttempt, WithdrawalEvent,
    WithdrawalHoldResolution, WithdrawalId, WithdrawalState,
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
    TransactionNotFound,
    TransactionNotConfirmed,
    TransactionReverted,
    SigningUnavailable,
    NonceUnavailable,
    NonceBlocked,
    NonceConflict,
    BaseStateMismatch,
    BridgeSignerMismatch,
    LedgerFeeExceedsServiceFee,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettlementActionResult {
    Complete {
        state: SettlementState,
    },
    Submitted {
        state: SettlementState,
        transaction_hash: Vec<u8>,
    },
    WaitingForConfirmation {
        state: SettlementState,
        transaction_hash: Vec<u8>,
    },
    ReconciliationProgress {
        state: SettlementState,
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
    ConfirmationRequired,
    InvalidConfirmationObservation,
    TransactionMismatch,
    WrongState,
    AutomaticProgressPending { next_run_at_ns: Option<u64> },
    RateLimited { retry_after_seconds: u64 },
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
            SettlementStopReason::TransactionNotFound => "Base transaction not found".into(),
            SettlementStopReason::TransactionNotConfirmed => {
                "Base transaction has not reached its required confirmation level".into()
            }
            SettlementStopReason::TransactionReverted => "Base transaction reverted".into(),
            SettlementStopReason::SigningUnavailable => "Threshold signing unavailable".into(),
            SettlementStopReason::NonceUnavailable => "Base nonce unavailable".into(),
            SettlementStopReason::NonceBlocked => {
                "Another Base operation is holding the next nonce".into()
            }
            SettlementStopReason::NonceConflict => {
                "Base nonce is occupied by a different transaction".into()
            }
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

pub(crate) enum NonceInitializationError {
    Observation,
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfirmationEvidence {
    Verified,
    WaitForFinalized,
    ReceiptMismatch,
}

fn confirmation_evidence(
    expected_receipt_block_number: Option<u64>,
    expected_finalized_block_number: Option<u64>,
    receipt_block_number: u64,
    finalized_head_block_number: u64,
) -> ConfirmationEvidence {
    if expected_receipt_block_number.is_none() || expected_finalized_block_number.is_none() {
        return ConfirmationEvidence::WaitForFinalized;
    }
    if expected_receipt_block_number.is_some_and(|expected| expected != receipt_block_number) {
        ConfirmationEvidence::ReceiptMismatch
    } else if expected_finalized_block_number
        .is_some_and(|expected| expected > finalized_head_block_number)
    {
        ConfirmationEvidence::WaitForFinalized
    } else {
        ConfirmationEvidence::Verified
    }
}

fn visible_confirmation_hash(
    stored_transaction_hash: [u8; 32],
    observed_transaction_hash: [u8; 32],
) -> [u8; 32] {
    if observed_transaction_hash != stored_transaction_hash {
        observed_transaction_hash
    } else {
        stored_transaction_hash
    }
}

pub(crate) async fn ensure_nonce_initialized(
    config: &crate::config::BridgeInitArgs,
) -> Result<(), NonceInitializationError> {
    ensure_nonce_initialized_inner(config, None).await
}

async fn ensure_nonce_initialized_inner(
    config: &crate::config::BridgeInitArgs,
    mut lease: Option<&mut crate::scheduler::SettlementLease>,
) -> Result<(), NonceInitializationError> {
    let initialized = STORE
        .with(|store| store.borrow().external_progress())
        .map_err(|_| NonceInitializationError::Storage)?
        .nonce_initialized;
    if initialized {
        return Ok(());
    }
    let address = match STORE
        .with(|store| store.borrow().signer_address())
        .map_err(|_| NonceInitializationError::Storage)?
    {
        Some(address) => address,
        None => {
            if let Some(lease) = lease.as_deref_mut() {
                lease
                    .renew_before_external_call()
                    .map_err(|_| NonceInitializationError::Storage)?;
            }
            let derived = signer::ethereum_address(config).await.map_err(|error| {
                ic_cdk::println!("failed to derive bridge signer address: {error:?}");
                NonceInitializationError::Observation
            })?;
            if let Some(lease) = lease.as_deref_mut() {
                lease
                    .ensure_current()
                    .map_err(|_| NonceInitializationError::Storage)?;
            }
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .set_signer_address_if_absent(derived)
                    .map_err(|_| NonceInitializationError::Storage)
            })?
        }
    };
    if let Some(lease) = lease.as_deref_mut() {
        lease
            .renew_before_external_call()
            .map_err(|_| NonceInitializationError::Storage)?;
    }
    let nonce = evm_rpc::transaction_count(config, address)
        .await
        .map_err(|error| {
            ic_cdk::println!("failed to observe bridge signer nonce: {error:?}");
            NonceInitializationError::Observation
        })?;
    if let Some(lease) = lease {
        lease
            .ensure_current()
            .map_err(|_| NonceInitializationError::Storage)?;
    }
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut progress = store
            .external_progress()
            .map_err(|_| NonceInitializationError::Storage)?;
        if !progress.nonce_initialized {
            progress.next_evm_nonce = nonce;
            progress.nonce_initialized = true;
            store
                .set_external_progress(&progress)
                .map_err(|_| NonceInitializationError::Storage)?;
        } else if progress.next_evm_nonce < nonce {
            return Err(NonceInitializationError::Storage);
        }
        Ok(())
    })
}

fn single_envelope(
    operation: bridge_core::EvmOperationRecord,
    intent: bridge_core::EvmCallIntent,
    nonce: u64,
) -> Option<bridge_core::EvmTransactionEnvelope> {
    if operation.id != intent.operation_id || operation.payload_hash != intent.payload_hash {
        return None;
    }
    Some(bridge_core::EvmTransactionEnvelope {
        operation_id: operation.id,
        payload_hash: operation.payload_hash,
        nonce,
        chain_id: intent.chain_id,
        contract: intent.contract,
        calldata: intent.calldata,
        gas_limit: intent.gas_limit,
        max_fee_per_gas: intent.max_fee_per_gas,
        max_priority_fee_per_gas: intent.max_priority_fee_per_gas,
        signed_transaction: None,
        initial_max_fee_per_gas: intent.max_fee_per_gas,
        initial_max_priority_fee_per_gas: intent.max_priority_fee_per_gas,
        fee_quote: intent.fee_quote,
        replacement_generation: 0,
        prior_signed_transactions: Vec::new(),
        first_broadcast_at_ns: 0,
        last_broadcast_at_ns: 0,
        rebroadcast_count: 0,
    })
}

const LEDGER_DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

fn prepared_release_fee_matches_configured(prepared: u128, configured: u128) -> bool {
    prepared == configured
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
                            ledger_block_index: block_index,
                        },
                        Some(&scan_target),
                    ),
                    RequestReference::DepositRefund(id) => advance_deposit_hold(
                        &mut store,
                        id,
                        hold_id,
                        DepositHoldResolution::RefundSucceeded {
                            ledger_block_index: block_index,
                        },
                        Some(&scan_target),
                    ),
                    RequestReference::Withdrawal(id) => advance_withdrawal_hold(
                        &mut store,
                        config,
                        id,
                        hold_id,
                        WithdrawalHoldResolution::Succeeded {
                            ledger_block_index: block_index,
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

fn confirm_evm_member(
    store: &mut crate::storage::StableStore,
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    finalized_head_block_number: u64,
    rpc_audit: Vec<crate::storage::AuditEventKind>,
) -> Result<(), ()> {
    operation
        .apply(EvmOperationEvent::Confirmed {
            transaction_hash,
            receipt_block_number,
            finalized_head_block_number,
        })
        .map_err(|_| ())?;
    let mut progress = store.external_progress().map_err(|_| ())?;
    progress.last_finalized_base_block = progress
        .last_finalized_base_block
        .max(finalized_head_block_number);
    if operation.kind == EvmOperationKind::MintDeposit {
        progress.last_finalized_mint_block = progress
            .last_finalized_mint_block
            .max(finalized_head_block_number);
    }
    progress.last_finalized_observation_ns = ic_cdk::api::time();
    store
        .commit_evm_terminal_bundle_with_rpc_audit(
            &operation,
            &progress,
            None,
            ic_cdk::api::canister_self(),
            ic_cdk::api::time(),
            rpc_audit,
        )
        .map_err(|_| ())
}

fn mark_evm_reverted(
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    finalized_head_block_number: u64,
    rpc_audit: Vec<crate::storage::AuditEventKind>,
) {
    operation
        .apply(EvmOperationEvent::Reverted {
            transaction_hash,
            receipt_block_number,
            finalized_head_block_number,
        })
        .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert transition failed: {error}")));
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut progress = store.external_progress().unwrap_or_else(|error| {
            ic_cdk::trap(format!("external progress read failed: {error}"))
        });
        progress.last_finalized_base_block = progress
            .last_finalized_base_block
            .max(finalized_head_block_number);
        progress.last_finalized_observation_ns = ic_cdk::api::time();
        store
            .commit_evm_terminal_bundle_with_rpc_audit(
                &operation,
                &progress,
                Some((
                    ic_cdk::api::canister_self(),
                    ic_cdk::api::time(),
                    finalized_head_block_number,
                )),
                ic_cdk::api::canister_self(),
                ic_cdk::api::time(),
                rpc_audit,
            )
            .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert bundle failed: {error}")));
    });
}

enum EvmAdvance {
    Complete,
    Submitted([u8; 32]),
    Waiting([u8; 32]),
    Stopped(SettlementStopReason),
}

enum ReplacementPreparation {
    Rebroadcast(bridge_core::EvmTransactionEnvelope),
    Replace(bridge_core::EvmTransactionEnvelope),
}

async fn mint_transaction_fits_reservation(
    config: &crate::config::BridgeInitArgs,
    envelope: &bridge_core::EvmTransactionEnvelope,
    raw: &[u8],
) -> Result<bool, evm_rpc::ObservationError> {
    let Some(quote) = envelope.fee_quote else {
        return Ok(true);
    };
    if ic_cdk::api::time() > quote.valid_until_ns {
        return Ok(false);
    }
    evm_rpc::signed_transaction_l1_fee(config, raw, quote.safe_block_hash)
        .await
        .map(|fee| fee <= quote.reserved_l1_fee_wei)
}

fn prepare_evm_replacement(
    mut envelope: bridge_core::EvmTransactionEnvelope,
    current_raw: &[u8],
    fee_policy: crate::config::EvmFeePolicy,
    policy: crate::config::EvmLivenessPolicy,
) -> ReplacementPreparation {
    let Some((next_max_fee, next_priority_fee)) = crate::config::next_replacement_fees(
        envelope.max_fee_per_gas,
        envelope.max_priority_fee_per_gas,
        envelope
            .fee_quote
            .map_or(fee_policy.max_fee_per_gas_ceiling, |quote| {
                quote.reachable_max_fee_per_gas
            }),
        fee_policy.max_priority_fee_per_gas_ceiling,
        policy,
    ) else {
        return ReplacementPreparation::Rebroadcast(envelope);
    };
    envelope
        .prior_signed_transactions
        .push(current_raw.to_vec());
    envelope.max_fee_per_gas = next_max_fee;
    envelope.max_priority_fee_per_gas = next_priority_fee;
    envelope.replacement_generation = envelope.replacement_generation.saturating_add(1);
    envelope.signed_transaction = None;
    ReplacementPreparation::Replace(envelope)
}

async fn maintain_missing_evm_transaction(
    config: &crate::config::BridgeInitArgs,
    operation_id: EvmOperationId,
    transaction_hash: [u8; 32],
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<Option<EvmAdvance>, SettlementActionError> {
    let checks = lease.job.confirmation_checks.saturating_add(1);
    let policy = config.evm_liveness;
    let rebroadcast_checks = policy
        .rebroadcast_after_seconds
        .div_ceil(policy.check_interval_seconds)
        .max(1) as u8;
    let replacement_checks = policy
        .replacement_after_seconds
        .div_ceil(policy.check_interval_seconds)
        .max(1) as u8;
    if envelope_replacement_limit_reached(checks, replacement_checks, policy.max_replacements) {
        return Ok(Some(EvmAdvance::Stopped(
            SettlementStopReason::TransactionNotFound,
        )));
    }
    let mut envelope = STORE.with(|store| {
        store
            .borrow()
            .evm_envelope(operation_id.get())
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::StorageFailure)
    })?;
    let current_raw = envelope
        .signed_transaction
        .clone()
        .ok_or(SettlementActionError::StorageFailure)?;
    let current_hash = signer::transaction_hash(&current_raw);

    if current_hash != transaction_hash {
        return broadcast_pending_evm_replacement(
            config,
            operation_id,
            transaction_hash,
            envelope,
            lease,
        )
        .await;
    }
    if checks == 0 || !checks.is_multiple_of(rebroadcast_checks) {
        return Ok(None);
    }

    if !checks.is_multiple_of(replacement_checks)
        || envelope.replacement_generation >= policy.max_replacements
    {
        return rebroadcast_current_evm_transaction(
            config,
            operation_id,
            transaction_hash,
            envelope,
            current_raw,
            checks,
            rebroadcast_checks,
            lease,
        )
        .await;
    }

    let previous_envelope = envelope.clone();
    envelope = match prepare_evm_replacement(envelope, &current_raw, config.evm_fee, policy) {
        ReplacementPreparation::Rebroadcast(envelope) => {
            return rebroadcast_current_evm_transaction(
                config,
                operation_id,
                transaction_hash,
                envelope,
                current_raw,
                checks,
                rebroadcast_checks,
                lease,
            )
            .await;
        }
        ReplacementPreparation::Replace(envelope) => envelope,
    };
    lease.renew_before_external_call()?;
    let raw = signer::sign(&envelope, config)
        .await
        .map_err(|_| SettlementActionError::StorageFailure)?;
    lease.ensure_current()?;
    if !mint_transaction_fits_reservation(config, &envelope, &raw)
        .await
        .unwrap_or(false)
    {
        return Ok(Some(EvmAdvance::Stopped(
            SettlementStopReason::BaseStateMismatch,
        )));
    }
    let next_hash = signer::transaction_hash(&raw);
    if next_hash == transaction_hash {
        return rebroadcast_current_evm_transaction(
            config,
            operation_id,
            transaction_hash,
            previous_envelope,
            current_raw,
            checks,
            rebroadcast_checks,
            lease,
        )
        .await;
    }
    envelope.signed_transaction = Some(raw.clone());
    STORE
        .with(|store| store.borrow_mut().replace_submitted_evm_envelope(&envelope))
        .map_err(|_| SettlementActionError::StorageFailure)?;
    broadcast_pending_evm_replacement(config, operation_id, transaction_hash, envelope, lease).await
}

#[allow(clippy::too_many_arguments)]
async fn rebroadcast_current_evm_transaction(
    config: &crate::config::BridgeInitArgs,
    operation_id: EvmOperationId,
    transaction_hash: [u8; 32],
    mut envelope: bridge_core::EvmTransactionEnvelope,
    current_raw: Vec<u8>,
    checks: u8,
    rebroadcast_checks: u8,
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<Option<EvmAdvance>, SettlementActionError> {
    lease.renew_before_external_call()?;
    if !mint_transaction_fits_reservation(config, &envelope, &current_raw)
        .await
        .unwrap_or(false)
    {
        return Ok(Some(EvmAdvance::Stopped(
            SettlementStopReason::BaseStateMismatch,
        )));
    }
    let evidence = match evm_rpc::broadcast(config, &current_raw).await {
        Ok(evm_rpc::BroadcastOutcome::Submitted(evidence)) => evidence,
        Ok(evm_rpc::BroadcastOutcome::NonceConflict(rpc_audit)) => {
            lease.ensure_current()?;
            persist_nonce_conflict_pause("rebroadcast_evm_operation", &rpc_audit)?;
            return Ok(Some(EvmAdvance::Stopped(
                SettlementStopReason::NonceConflict,
            )));
        }
        Err(_) => return Ok(None),
    };
    lease.ensure_current()?;
    envelope.last_broadcast_at_ns = ic_cdk::api::time();
    envelope.rebroadcast_count = envelope.rebroadcast_count.saturating_add(1);
    STORE
        .with(|store| {
            let mut store = store.borrow_mut();
            store.record_evm_broadcast(&envelope)?;
            store.append_audit_events_atomically(
                ic_cdk::api::canister_self(),
                vec![
                    crate::rpc_audit_event_kind(&evidence),
                    crate::storage::AuditEventKind::EvmTransactionRebroadcasted {
                        operation_id: operation_id.get(),
                        transaction_hash: transaction_hash.to_vec(),
                        attempt: checks / rebroadcast_checks,
                    },
                ],
            )
        })
        .map_err(|_| SettlementActionError::StorageFailure)?;
    Ok(None)
}

async fn broadcast_pending_evm_replacement(
    config: &crate::config::BridgeInitArgs,
    operation_id: EvmOperationId,
    previous_hash: [u8; 32],
    mut envelope: bridge_core::EvmTransactionEnvelope,
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<Option<EvmAdvance>, SettlementActionError> {
    let raw = envelope
        .signed_transaction
        .clone()
        .ok_or(SettlementActionError::StorageFailure)?;
    let next_hash = signer::transaction_hash(&raw);
    if next_hash == previous_hash {
        return Err(SettlementActionError::StorageFailure);
    }
    lease.renew_before_external_call()?;
    if !mint_transaction_fits_reservation(config, &envelope, &raw)
        .await
        .unwrap_or(false)
    {
        return Ok(Some(EvmAdvance::Stopped(
            SettlementStopReason::BaseStateMismatch,
        )));
    }
    let evidence = match evm_rpc::broadcast(config, &raw).await {
        Ok(evm_rpc::BroadcastOutcome::Submitted(evidence)) => evidence,
        Ok(evm_rpc::BroadcastOutcome::NonceConflict(rpc_audit)) => {
            lease.ensure_current()?;
            persist_nonce_conflict_pause("broadcast_evm_replacement", &rpc_audit)?;
            return Ok(Some(EvmAdvance::Stopped(
                SettlementStopReason::NonceConflict,
            )));
        }
        Err(_) => return Ok(None),
    };
    lease.ensure_current()?;
    let now = ic_cdk::api::time();
    envelope.last_broadcast_at_ns = now;
    STORE
        .with(|store| {
            let mut store = store.borrow_mut();
            let mut operation = store
                .evm_operation(operation_id.get())?
                .ok_or(crate::storage::StorageError::RecordNotFound)?;
            if operation.state
                != (EvmOperationState::Submitted {
                    transaction_hash: previous_hash,
                })
            {
                return Err(crate::storage::StorageError::Core(
                    bridge_core::CoreError::ConflictingReplay,
                ));
            }
            operation.state = EvmOperationState::Submitted {
                transaction_hash: next_hash,
            };
            store.promote_submitted_evm_replacement_with_rpc_audit(
                &operation,
                &envelope,
                ic_cdk::api::canister_self(),
                now,
                vec![
                    crate::rpc_audit_event_kind(&evidence),
                    crate::storage::AuditEventKind::EvmTransactionReplaced {
                        operation_id: operation_id.get(),
                        previous_transaction_hash: previous_hash.to_vec(),
                        transaction_hash: next_hash.to_vec(),
                        generation: envelope.replacement_generation,
                        max_fee_per_gas: envelope.max_fee_per_gas,
                        max_priority_fee_per_gas: envelope.max_priority_fee_per_gas,
                    },
                ],
            )
        })
        .map_err(|_| SettlementActionError::StorageFailure)?;
    Ok(Some(EvmAdvance::Waiting(next_hash)))
}

fn envelope_replacement_limit_reached(checks: u8, replacement_checks: u8, maximum: u8) -> bool {
    checks >= replacement_checks.saturating_mul(maximum.saturating_add(1))
}

enum HoldAdvance {
    Continue,
    Progress,
    Stopped(SettlementStopReason),
}

fn map_observation_stop(error: evm_rpc::ObservationError) -> SettlementStopReason {
    match error {
        evm_rpc::ObservationError::Rpc => SettlementStopReason::RpcUnavailable,
        evm_rpc::ObservationError::Inconsistent => SettlementStopReason::RpcInconsistent,
        evm_rpc::ObservationError::InvalidResponse | evm_rpc::ObservationError::Overflow => {
            SettlementStopReason::InvalidBaseResponse
        }
        evm_rpc::ObservationError::BaseStateMismatch => SettlementStopReason::BaseStateMismatch,
        evm_rpc::ObservationError::ChainIdMismatch => SettlementStopReason::BaseStateMismatch,
    }
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

fn persist_nonce_conflict_pause(
    operation: &str,
    rpc_audit: &evm_rpc::RpcAuditEvidence,
) -> Result<(), SettlementActionError> {
    let decision = evm_rpc::nonce_conflict_decision(
        operation,
        rpc_audit
            .transaction_hash
            .ok_or(SettlementActionError::StorageFailure)?,
    );
    STORE.with(|store| {
        store
            .borrow_mut()
            .pause_deposits_with_rpc_audit(
                ic_cdk::api::canister_self(),
                ic_cdk::api::time(),
                vec![
                    crate::rpc_audit_event_kind(rpc_audit),
                    crate::rpc_decision_event_kind(&decision),
                ],
            )
            .map_err(|_| SettlementActionError::StorageFailure)
    })
}

fn select_submitted_transaction_hash(
    operation_id: EvmOperationId,
    expected_current_hash: [u8; 32],
    observed_hash: [u8; 32],
) -> Result<bridge_core::EvmOperationRecord, SettlementActionError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut operation = store
            .evm_operation(operation_id.get())
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)?;
        if operation.state
            != (EvmOperationState::Submitted {
                transaction_hash: expected_current_hash,
            })
        {
            return Err(SettlementActionError::Busy);
        }
        if observed_hash != expected_current_hash {
            operation.state = EvmOperationState::Submitted {
                transaction_hash: observed_hash,
            };
            let envelope = store
                .evm_envelope(operation_id.get())
                .map_err(|_| SettlementActionError::StorageFailure)?
                .ok_or(SettlementActionError::StorageFailure)?;
            store
                .promote_submitted_evm_replacement_with_rpc_audit(
                    &operation,
                    &envelope,
                    ic_cdk::api::canister_self(),
                    ic_cdk::api::time(),
                    Vec::new(),
                )
                .map_err(|_| SettlementActionError::StorageFailure)?;
        }
        Ok(operation)
    })
}

fn assign_evm_nonce_for(
    operation_id: bridge_core::EvmOperationId,
) -> Result<bool, SettlementActionError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut operation = store
            .evm_operation(operation_id.get())
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)?;
        if !matches!(operation.state, EvmOperationState::Queued) {
            return Ok(true);
        }
        let has_other_prepared = if let Some((prepared, _)) = store
            .first_prepared_evm()
            .map_err(|_| SettlementActionError::StorageFailure)?
        {
            prepared.id != operation_id
        } else {
            false
        };
        if !bridge_core::can_assign_nonce(true, has_other_prepared) {
            return Ok(false);
        }
        let intent = store
            .evm_call_intent(operation_id.get())
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::StorageFailure)?;
        let mut progress = store
            .external_progress()
            .map_err(|_| SettlementActionError::StorageFailure)?;
        let nonce = progress.next_evm_nonce;
        let next = bridge_core::nonce_next(nonce).ok_or(SettlementActionError::StorageFailure)?;
        let envelope = single_envelope(operation, intent, nonce)
            .ok_or(SettlementActionError::StorageFailure)?;
        operation
            .apply(EvmOperationEvent::Prepared)
            .map_err(|_| SettlementActionError::StorageFailure)?;
        progress.next_evm_nonce = next;
        store
            .prepare_evm_operation(&operation, &envelope, &progress)
            .map_err(|_| SettlementActionError::StorageFailure)?;
        Ok(true)
    })
}

async fn advance_evm_operation(
    config: &crate::config::BridgeInitArgs,
    operation_id: bridge_core::EvmOperationId,
    lease: &mut crate::scheduler::SettlementLease,
) -> Result<EvmAdvance, SettlementActionError> {
    let operation = STORE.with(|store| {
        store
            .borrow()
            .evm_operation(operation_id.get())
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)
    })?;
    match operation.state {
        EvmOperationState::Queued => {
            match ensure_nonce_initialized_inner(config, Some(lease)).await {
                Ok(()) => {}
                Err(NonceInitializationError::Observation) => {
                    return Ok(EvmAdvance::Stopped(SettlementStopReason::NonceUnavailable));
                }
                Err(NonceInitializationError::Storage) => {
                    return Err(SettlementActionError::StorageFailure);
                }
            }
            if !assign_evm_nonce_for(operation_id)? {
                return Ok(EvmAdvance::Stopped(SettlementStopReason::NonceBlocked));
            }
            Box::pin(advance_evm_operation(config, operation_id, lease)).await
        }
        EvmOperationState::Prepared => {
            let mut envelope = STORE.with(|store| {
                store
                    .borrow()
                    .evm_envelope(operation_id.get())
                    .map_err(|_| SettlementActionError::StorageFailure)?
                    .ok_or(SettlementActionError::StorageFailure)
            })?;
            let raw = match envelope.signed_transaction.clone() {
                Some(raw) => raw,
                None => {
                    lease.renew_before_external_call()?;
                    match signer::sign(&envelope, config).await {
                        Ok(raw) => {
                            lease.ensure_current()?;
                            envelope.signed_transaction = Some(raw.clone());
                            STORE.with(|store| {
                                store
                                    .borrow_mut()
                                    .put_evm_envelope(&envelope)
                                    .map_err(|_| SettlementActionError::StorageFailure)
                            })?;
                            raw
                        }
                        Err(_) => {
                            return Ok(EvmAdvance::Stopped(
                                SettlementStopReason::SigningUnavailable,
                            ));
                        }
                    }
                }
            };
            if !mint_transaction_fits_reservation(config, &envelope, &raw)
                .await
                .unwrap_or(false)
            {
                return Ok(EvmAdvance::Stopped(
                    SettlementStopReason::BaseStateMismatch,
                ));
            }
            lease.renew_before_external_call()?;
            let broadcast = match evm_rpc::broadcast(config, &raw).await {
                Ok(outcome) => outcome,
                Err(error) => return Ok(EvmAdvance::Stopped(map_observation_stop(error))),
            };
            lease.ensure_current()?;
            if let evm_rpc::BroadcastOutcome::NonceConflict(rpc_audit) = &broadcast {
                persist_nonce_conflict_pause("broadcast_evm_operation", rpc_audit)?;
                return Ok(EvmAdvance::Stopped(SettlementStopReason::NonceConflict));
            }
            let rpc_audit = match &broadcast {
                evm_rpc::BroadcastOutcome::Submitted(evidence) => evidence,
                evm_rpc::BroadcastOutcome::NonceConflict(_) => unreachable!(),
            };
            let transaction_hash = signer::transaction_hash(&raw);
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let mut current = store
                    .evm_operation(operation_id.get())
                    .map_err(|_| SettlementActionError::StorageFailure)?
                    .ok_or(SettlementActionError::NotFound)?;
                if !matches!(current.state, EvmOperationState::Prepared) {
                    return Err(SettlementActionError::Busy);
                }
                let submitted_at_ns = ic_cdk::api::time();
                envelope.first_broadcast_at_ns = submitted_at_ns;
                envelope.last_broadcast_at_ns = submitted_at_ns;
                store
                    .record_evm_broadcast(&envelope)
                    .map_err(|_| SettlementActionError::StorageFailure)?;
                current
                    .apply(EvmOperationEvent::Submitted { transaction_hash })
                    .map_err(|_| SettlementActionError::StorageFailure)?;
                store
                    .put_submitted_evm_operation_with_rpc_audit(
                        &current,
                        submitted_at_ns,
                        ic_cdk::api::canister_self(),
                        vec![
                            crate::rpc_audit_event_kind(rpc_audit),
                            crate::rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                                "broadcast_evm_operation",
                                Some(transaction_hash),
                                false,
                            )),
                        ],
                    )
                    .map_err(|_| SettlementActionError::StorageFailure)
            })?;
            Ok(EvmAdvance::Submitted(transaction_hash))
        }
        EvmOperationState::Submitted { transaction_hash } => {
            lease.renew_before_external_call()?;
            let mut outcome =
                match evm_rpc::confirmed_receipt_outcome(config, transaction_hash).await {
                    Ok(outcome) => outcome,
                    Err(evm_rpc::ObservationError::Inconsistent) => {
                        let decision = evm_rpc::quorum_loss_decision(
                            "confirm_evm_operation",
                            Some(transaction_hash),
                        );
                        STORE
                            .with(|store| {
                                store.borrow_mut().append_audit_events_atomically(
                                    ic_cdk::api::canister_self(),
                                    vec![crate::rpc_decision_event_kind(&decision)],
                                )
                            })
                            .map_err(|_| SettlementActionError::StorageFailure)?;
                        return Ok(EvmAdvance::Stopped(SettlementStopReason::RpcInconsistent));
                    }
                    Err(error) => return Ok(EvmAdvance::Stopped(map_observation_stop(error))),
                };
            lease.ensure_current()?;
            let mut observed_transaction_hash = transaction_hash;
            if matches!(outcome, evm_rpc::ConfirmedReceiptOutcome::Missing) {
                let prior_transactions = STORE.with(|store| {
                    store
                        .borrow()
                        .evm_envelope(operation_id.get())
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::StorageFailure)
                        .map(|envelope| envelope.prior_signed_transactions)
                })?;
                let mut pending_receipt_block = None;
                for raw in prior_transactions.iter().rev() {
                    let prior_hash = signer::transaction_hash(raw);
                    lease.renew_before_external_call()?;
                    let prior_outcome = match evm_rpc::confirmed_receipt_outcome(config, prior_hash)
                        .await
                    {
                        Ok(outcome) => outcome,
                        Err(evm_rpc::ObservationError::Inconsistent) => {
                            let decision = evm_rpc::quorum_loss_decision(
                                "confirm_prior_evm_operation",
                                Some(prior_hash),
                            );
                            STORE
                                .with(|store| {
                                    store.borrow_mut().append_audit_events_atomically(
                                        ic_cdk::api::canister_self(),
                                        vec![crate::rpc_decision_event_kind(&decision)],
                                    )
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            return Ok(EvmAdvance::Stopped(SettlementStopReason::RpcInconsistent));
                        }
                        Err(error) => return Ok(EvmAdvance::Stopped(map_observation_stop(error))),
                    };
                    lease.ensure_current()?;
                    match prior_outcome {
                        evm_rpc::ConfirmedReceiptOutcome::Missing => {}
                        evm_rpc::ConfirmedReceiptOutcome::Pending {
                            receipt_block_number,
                        } => pending_receipt_block = Some(receipt_block_number),
                        terminal => {
                            observed_transaction_hash = prior_hash;
                            outcome = terminal;
                            break;
                        }
                    }
                }
                if matches!(outcome, evm_rpc::ConfirmedReceiptOutcome::Missing) {
                    if let Some(receipt_block_number) = pending_receipt_block {
                        outcome = evm_rpc::ConfirmedReceiptOutcome::Pending {
                            receipt_block_number,
                        };
                    }
                }
            }
            match outcome {
                evm_rpc::ConfirmedReceiptOutcome::Missing => {
                    if let Some(outcome) = maintain_missing_evm_transaction(
                        config,
                        operation_id,
                        transaction_hash,
                        lease,
                    )
                    .await?
                    {
                        return Ok(outcome);
                    }
                    Ok(EvmAdvance::Waiting(transaction_hash))
                }
                evm_rpc::ConfirmedReceiptOutcome::Pending { .. } => {
                    Ok(EvmAdvance::Waiting(transaction_hash))
                }
                evm_rpc::ConfirmedReceiptOutcome::Succeeded {
                    receipt_block_number,
                    finalized_head_block_number,
                    rpc_audit,
                } => {
                    let visible_hash =
                        visible_confirmation_hash(transaction_hash, observed_transaction_hash);
                    let terminal_operation = select_submitted_transaction_hash(
                        operation_id,
                        transaction_hash,
                        visible_hash,
                    )?;
                    match confirmation_evidence(
                        lease.expected_receipt_block_number(),
                        lease.expected_finalized_block_number(),
                        receipt_block_number,
                        finalized_head_block_number,
                    ) {
                        ConfirmationEvidence::ReceiptMismatch => {
                            return Ok(EvmAdvance::Stopped(
                                SettlementStopReason::BaseStateMismatch,
                            ));
                        }
                        ConfirmationEvidence::WaitForFinalized => {
                            return Ok(EvmAdvance::Waiting(visible_hash));
                        }
                        ConfirmationEvidence::Verified => {}
                    }
                    STORE.with(|store| {
                        let mut store = store.borrow_mut();
                        confirm_evm_member(
                            &mut store,
                            terminal_operation,
                            visible_hash,
                            receipt_block_number,
                            finalized_head_block_number,
                            vec![
                                crate::rpc_audit_event_kind(&rpc_audit),
                                crate::rpc_decision_event_kind(
                                    &evm_rpc::quorum_continued_decision(
                                        "confirm_evm_operation",
                                        Some(visible_hash),
                                        false,
                                    ),
                                ),
                            ],
                        )
                        .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(EvmAdvance::Complete)
                }
                evm_rpc::ConfirmedReceiptOutcome::Reverted {
                    receipt_block_number,
                    finalized_head_block_number,
                    rpc_audit,
                } => {
                    let visible_hash =
                        visible_confirmation_hash(transaction_hash, observed_transaction_hash);
                    let terminal_operation = select_submitted_transaction_hash(
                        operation_id,
                        transaction_hash,
                        visible_hash,
                    )?;
                    match confirmation_evidence(
                        lease.expected_receipt_block_number(),
                        lease.expected_finalized_block_number(),
                        receipt_block_number,
                        finalized_head_block_number,
                    ) {
                        ConfirmationEvidence::ReceiptMismatch => {
                            return Ok(EvmAdvance::Stopped(
                                SettlementStopReason::BaseStateMismatch,
                            ));
                        }
                        ConfirmationEvidence::WaitForFinalized => {
                            return Ok(EvmAdvance::Waiting(visible_hash));
                        }
                        ConfirmationEvidence::Verified => {}
                    }
                    mark_evm_reverted(
                        terminal_operation,
                        visible_hash,
                        receipt_block_number,
                        finalized_head_block_number,
                        vec![
                            crate::rpc_audit_event_kind(&rpc_audit),
                            crate::rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                                "confirm_evm_operation",
                                Some(visible_hash),
                                false,
                            )),
                        ],
                    );
                    Ok(EvmAdvance::Complete)
                }
            }
        }
        EvmOperationState::Confirmed { .. } => Ok(EvmAdvance::Complete),
        EvmOperationState::Reverted { .. } => Ok(EvmAdvance::Stopped(
            SettlementStopReason::TransactionReverted,
        )),
        EvmOperationState::RecoveryPending { .. } | EvmOperationState::Recovered { .. } => Ok(
            EvmAdvance::Stopped(SettlementStopReason::TransactionReverted),
        ),
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
                        ledger_block_index: block_index,
                    },
                    None,
                ),
                RequestReference::DepositRefund(id) => advance_deposit_hold(
                    &mut store,
                    id,
                    hold.id,
                    DepositHoldResolution::RefundSucceeded {
                        ledger_block_index: block_index,
                    },
                    None,
                ),
                RequestReference::Withdrawal(id) => advance_withdrawal_hold(
                    &mut store,
                    config,
                    id,
                    hold.id,
                    WithdrawalHoldResolution::Succeeded {
                        ledger_block_index: block_index,
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
    Quote {
        quote: DepositQuote,
        admission: Box<crate::storage::DepositReserveAdmission>,
    },
    Refund(DepositRefundReason),
    Stopped(SettlementStopReason),
}

fn start_deposit_refund(
    deposit_id: [u8; 32],
    reason: DepositRefundReason,
) -> Result<(), SettlementActionError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut deposit = store
            .deposit(deposit_id)
            .map_err(|_| SettlementActionError::StorageFailure)?
            .ok_or(SettlementActionError::NotFound)?;
        let fee = ledger::KINIC_LEDGER_FEE;
        let amount = bridge_core::deposit_refund_amount(deposit.gross_amount.get(), fee.get())
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
        deposit
            .apply(DepositEvent::StartRefund {
                reason,
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity,
                }),
            })
            .map_err(|_| SettlementActionError::StorageFailure)?;
        store
            .put_deposit(&deposit)
            .map_err(|_| SettlementActionError::StorageFailure)
    })
}

async fn prepare_escrowed_deposit(
    config: &crate::config::BridgeInitArgs,
    deposit: &bridge_core::DepositRecord,
) -> Result<EscrowPreparation, SettlementActionError> {
    let (snapshot, snapshot_generation) =
        match crate::api::base_mint_snapshot(config, ic_cdk::api::time()).await {
            Ok(snapshot) => snapshot,
            Err(crate::api::DepositError::DepositsPaused) => {
                return Ok(EscrowPreparation::Refund(DepositRefundReason::BasePaused));
            }
            Err(crate::api::DepositError::BaseObservationUnavailable) => {
                return Ok(EscrowPreparation::Stopped(
                    SettlementStopReason::RpcUnavailable,
                ));
            }
            Err(crate::api::DepositError::StorageFailure) => {
                return Err(SettlementActionError::StorageFailure);
            }
            Err(_) => {
                return Ok(EscrowPreparation::Stopped(
                    SettlementStopReason::InvalidBaseResponse,
                ));
            }
        };
    let net_amount = match snapshot.quote(deposit.gross_amount, deposit.max_service_fee) {
        Ok(amount) => amount,
        Err(
            bridge_core::CoreError::ServiceFeeAboveMaximum
            | bridge_core::CoreError::ServiceFeeAboveUserMaximum
            | bridge_core::CoreError::InvalidAmount
            | bridge_core::CoreError::ArithmeticUnderflow,
        ) => {
            return Ok(EscrowPreparation::Refund(
                DepositRefundReason::ServiceFeeRejected,
            ));
        }
        Err(bridge_core::CoreError::PerDepositLimitExceeded) => {
            return Ok(EscrowPreparation::Refund(
                DepositRefundReason::PerDepositLimitExceeded,
            ));
        }
        Err(bridge_core::CoreError::MintWindowLimitExceeded) => {
            return Ok(EscrowPreparation::Refund(
                DepositRefundReason::MintWindowLimitExceeded,
            ));
        }
        Err(_) => return Err(SettlementActionError::StorageFailure),
    };
    match ensure_nonce_initialized(config).await {
        Ok(()) => {}
        Err(NonceInitializationError::Observation) => {
            return Ok(EscrowPreparation::Stopped(
                SettlementStopReason::NonceUnavailable,
            ));
        }
        Err(NonceInitializationError::Storage) => {
            return Err(SettlementActionError::StorageFailure);
        }
    }
    let signer_address = match crate::api::cached_signer_address(config).await {
        Ok(address) => address,
        Err(_) => {
            return Ok(EscrowPreparation::Stopped(
                SettlementStopReason::BridgeSignerMismatch,
            ));
        }
    };
    let (expected_token, finalized_observation) = STORE.with(|store| {
        let store = store.borrow();
        let progress = store
            .external_progress()
            .map_err(|_| SettlementActionError::StorageFailure)?;
        Ok::<_, SettlementActionError>((
            store
                .deposit_reserve_token()
                .map_err(|_| SettlementActionError::StorageFailure)?,
            progress.finalized_observation,
        ))
    })?;
    let Some(finalized_observation) = finalized_observation else {
        return Ok(EscrowPreparation::Stopped(
            SettlementStopReason::RpcUnavailable,
        ));
    };
    let finalized_eth = evm_rpc::signer_eth_balance_at(
        config,
        signer_address,
        evm_rpc::FinalizedObservation {
            chain_id: finalized_observation.chain_id,
            block_number: finalized_observation.block_number,
            block_hash: finalized_observation.block_hash,
            observed_at_ns: finalized_observation.observed_at_ns,
        },
    )
    .await;
    let Ok(finalized_eth) = finalized_eth else {
        return Ok(EscrowPreparation::Stopped(
            SettlementStopReason::RpcUnavailable,
        ));
    };
    let safe_eth = evm_rpc::signer_eth_balance(config, signer_address).await;
    let Ok(safe_eth) = safe_eth else {
        return Ok(EscrowPreparation::Stopped(
            SettlementStopReason::RpcUnavailable,
        ));
    };
    let admission = crate::storage::DepositReserveAdmission {
        audit_caller: ic_cdk::api::canister_self(),
        expected_token,
        observed_at_ns: ic_cdk::api::time(),
        eth_balance_wei: finalized_eth.min(safe_eth),
        cycles_balance: ic_cdk::api::canister_liquid_cycle_balance(),
        reserve_policy: config.reserve_policy(),
        mint_snapshot: snapshot,
        snapshot_generation,
    };
    let quote = DepositQuote {
        service_fee: snapshot.service_fee,
        net_amount,
    };
    Ok(EscrowPreparation::Quote {
        quote,
        admission: Box::new(admission),
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
                            current
                                .apply(DepositEvent::FundingSucceeded {
                                    ledger_block_index: block_index,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_deposit_funding_callback(&current, &callback_token)
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
                            current
                                .apply(DepositEvent::FundingAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
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
                let recipient = STORE.with(|store| {
                    store
                        .borrow()
                        .deposit_intent(deposit_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .map(|intent| intent.base_recipient)
                        .ok_or(SettlementActionError::StorageFailure)
                })?;
                lease.renew_before_external_call()?;
                let prepared = prepare_escrowed_deposit(&config, &deposit).await?;
                lease.ensure_current()?;
                match prepared {
                    EscrowPreparation::Quote { quote, admission } => {
                        let result = STORE.with(|store| {
                            crate::api::commit_deposit_quote(
                                &mut store.borrow_mut(),
                                deposit_id,
                                recipient,
                                &config,
                                quote,
                                *admission,
                            )
                        });
                        match result {
                            Ok(()) => {}
                            Err(crate::api::DepositError::ReserveUnavailable) => {
                                start_deposit_refund(
                                    deposit_id,
                                    DepositRefundReason::ReserveInsufficient,
                                )?;
                            }
                            Err(crate::api::DepositError::Rejected(message))
                                if message == "MintWindowLimitExceeded" =>
                            {
                                start_deposit_refund(
                                    deposit_id,
                                    DepositRefundReason::MintWindowLimitExceeded,
                                )?;
                            }
                            Err(crate::api::DepositError::BaseObservationUnavailable) => {
                                return Ok(SettlementActionResult::Stopped {
                                    state,
                                    reason: SettlementStopReason::RpcUnavailable,
                                });
                            }
                            Err(_) => return Err(SettlementActionError::StorageFailure),
                        }
                    }
                    EscrowPreparation::Refund(reason) => {
                        start_deposit_refund(deposit_id, reason)?;
                    }
                    EscrowPreparation::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason });
                    }
                }
            }
            bridge_core::DepositState::MintPending { operation_id, .. } => {
                return match advance_evm_operation(&config, operation_id, lease).await? {
                    EvmAdvance::Complete => continue,
                    EvmAdvance::Submitted(hash) => Ok(SettlementActionResult::Submitted {
                        state,
                        transaction_hash: hash.to_vec(),
                    }),
                    EvmAdvance::Waiting(hash) => {
                        Ok(SettlementActionResult::WaitingForConfirmation {
                            state,
                            transaction_hash: hash.to_vec(),
                        })
                    }
                    EvmAdvance::Stopped(reason) => {
                        Ok(SettlementActionResult::Stopped { state, reason })
                    }
                };
            }
            bridge_core::DepositState::FundingReconciliationHold { hold_id }
            | bridge_core::DepositState::RefundReconciliationHold { hold_id, .. } => {
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
                            current
                                .apply(DepositEvent::RefundSucceeded {
                                    ledger_block_index: block_index,
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_deposit(&current)
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
                            current
                                .apply(DepositEvent::RefundAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let hold = ReconciliationHoldRecord::open(
                                hold_id,
                                RequestReference::DepositRefund(current.id),
                                attempt.identity.clone(),
                            );
                            store
                                .commit_deposit_hold_bundle(&current, &hold)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: SettlementState::Deposit(DepositPhase::RefundReconciliationHold),
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
            bridge_core::DepositState::Minted { .. }
            | bridge_core::DepositState::MintReverted { .. }
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
                                    ledger_block_index: block_index,
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
                    HoldAdvance::Continue => continue,
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
    use bridge_core::{EvmCallIntent, EvmOperationId, EvmOperationRecord};

    #[test]
    fn frontend_confirmation_requires_matching_receipt_and_reached_finalized_head() {
        assert_eq!(
            confirmation_evidence(Some(10), Some(12), 10, 11),
            ConfirmationEvidence::WaitForFinalized
        );
        assert_eq!(
            confirmation_evidence(Some(10), Some(12), 10, 12),
            ConfirmationEvidence::Verified
        );
        assert_eq!(
            confirmation_evidence(Some(10), Some(12), 10, 13),
            ConfirmationEvidence::Verified
        );
        assert_eq!(
            confirmation_evidence(Some(10), Some(12), 11, 12),
            ConfirmationEvidence::ReceiptMismatch
        );
        assert_eq!(
            confirmation_evidence(None, None, 10, 12),
            ConfirmationEvidence::WaitForFinalized
        );
    }

    #[test]
    fn finalized_prior_replacement_hash_becomes_the_wallet_visible_hash() {
        let current = [1; 32];
        let finalized_prior = [2; 32];
        assert_eq!(
            visible_confirmation_hash(current, finalized_prior),
            finalized_prior
        );
        assert_eq!(visible_confirmation_hash(current, current), current);
    }

    #[test]
    fn evm_envelope_contains_exactly_one_operation_and_original_calldata() {
        let operation_id = EvmOperationId::new(1);
        let operation =
            EvmOperationRecord::queued(operation_id, [1; 32], EvmOperationKind::MintDeposit);
        let calldata = vec![1, 2, 3, 4, 5];
        let intent = EvmCallIntent {
            operation_id,
            payload_hash: operation.payload_hash,
            chain_id: 8453,
            contract: [7; 20],
            calldata: calldata.clone(),
            gas_limit: 100_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
            fee_quote: None,
        };
        let envelope = single_envelope(operation, intent, 9).expect("single envelope");
        assert_eq!(envelope.operation_id, operation_id);
        assert_eq!(envelope.operation_id, EvmOperationId::new(1));
        assert_eq!(envelope.nonce, 9);
        assert_eq!(envelope.gas_limit, 100_000);
        assert_eq!(envelope.calldata, calldata);
    }

    #[test]
    fn replacement_limit_allows_three_generations_then_stops() {
        assert!(!envelope_replacement_limit_reached(30, 30, 3));
        assert!(!envelope_replacement_limit_reached(90, 30, 3));
        assert!(envelope_replacement_limit_reached(120, 30, 3));
    }

    #[test]
    fn replacement_without_a_fee_increase_keeps_the_current_transaction_for_rebroadcast() {
        let operation_id = EvmOperationId::new(1);
        let operation =
            EvmOperationRecord::queued(operation_id, [1; 32], EvmOperationKind::MintDeposit);
        let intent = EvmCallIntent {
            operation_id,
            payload_hash: operation.payload_hash,
            chain_id: 8453,
            contract: [7; 20],
            calldata: vec![1, 2, 3],
            gas_limit: 100_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 0,
            fee_quote: None,
        };
        let mut envelope = single_envelope(operation, intent, 9).expect("single envelope");
        let raw = vec![2, 3, 4];
        envelope.signed_transaction = Some(raw.clone());
        envelope.max_fee_per_gas = 2;
        envelope.initial_max_fee_per_gas = 1;
        envelope.replacement_generation = 1;
        let original = envelope.clone();
        let original_hash = signer::transaction_hash(&raw);
        let policy = crate::config::EvmLivenessPolicy {
            max_replacements: 2,
            fee_bump_bps: 5_000,
            ..crate::config::EvmLivenessPolicy::default()
        };
        let fee_policy = crate::config::EvmFeePolicy {
            gas_limit_ceiling: 100_000,
            max_fee_per_gas_ceiling: 2,
            max_priority_fee_per_gas_ceiling: 0,
            l1_fee_per_transaction_ceiling_wei: 1,
            quote_validity_seconds: 90,
            gas_limit_multiplier_bps: 13_000,
            base_fee_multiplier_bps: 60_000,
            l1_fee_multiplier_bps: 15_000,
        };

        let ReplacementPreparation::Rebroadcast(rebroadcast) =
            prepare_evm_replacement(envelope, &raw, fee_policy, policy)
        else {
            panic!("fee ceiling must select rebroadcast");
        };
        assert_eq!(rebroadcast, original);
        assert_eq!(rebroadcast.replacement_generation, 1);
        assert_eq!(rebroadcast.operation_id, operation_id);
        assert_eq!(
            signer::transaction_hash(
                rebroadcast
                    .signed_transaction
                    .as_deref()
                    .expect("signed transaction remains stored")
            ),
            original_hash
        );
    }
}
