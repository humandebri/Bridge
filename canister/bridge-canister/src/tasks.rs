use crate::{
    evm_rpc, ledger,
    phases::{DepositPhase, SettlementState, WithdrawalPhase},
    signer, storage_or_trap, STORE,
};
use bridge_core::{
    Account, DepositEvent, DepositHoldResolution, EvmOperationEvent, EvmOperationKind,
    EvmOperationState, LedgerCallOutcome, LedgerOperation, LedgerTransferIdentity,
    ReconciliationHoldRecord, ReconciliationScanProgress, ReconciliationTarget, RequestReference,
    Settlement, TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId,
    WithdrawalState,
};
use candid::{CandidType, Deserialize};
use sha2::{Digest, Sha256};

fn retry_memo(hold_id: u64, identity: &LedgerTransferIdentity) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"KINIC-WITHDRAWAL-RETRY");
    digest.update(hold_id.to_be_bytes());
    digest.update(identity.created_at_time_ns.to_be_bytes());
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
    LedgerFeeChanged,
    NonceConflict,
    BaseStateMismatch,
    BridgeSignerMismatch,
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
            SettlementStopReason::LedgerFeeChanged => {
                "Ledger fee changed; settlement identity was updated without sending".into()
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
    })
}

const LEDGER_DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

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
                    RequestReference::Deposit(id) => advance_deposit_hold(
                        &mut store,
                        config,
                        id,
                        hold_id,
                        DepositHoldResolution::Succeeded {
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
                    RequestReference::Deposit(id) => advance_deposit_hold(
                        &mut store,
                        config,
                        id,
                        hold_id,
                        DepositHoldResolution::Absent { history_watermark },
                        Some(&scan_target),
                    ),
                    RequestReference::Withdrawal(id) => {
                        let mut next_identity = transfer;
                        next_identity.created_at_time_ns = ic_cdk::api::time()
                            .max(next_identity.created_at_time_ns.saturating_add(1));
                        next_identity.memo = retry_memo(hold_id.get(), &next_identity);
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
        }
    });
}

fn advance_deposit_hold(
    store: &mut crate::storage::StableStore,
    config: &crate::config::BridgeInitArgs,
    deposit_id: bridge_core::DepositId,
    hold_id: bridge_core::HoldId,
    resolution: DepositHoldResolution,
    scan_target: Option<&ReconciliationTarget>,
) {
    let DepositHoldResolution::Succeeded {
        ledger_block_index: block_index,
    } = resolution
    else {
        store
            .resolve_deposit_hold_and_scan(deposit_id, hold_id, resolution, scan_target)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!(
                    "deposit reconciliation persistence failed: {error}"
                ))
            });
        return;
    };
    let recipient = store
        .deposit_intent(deposit_id.bytes())
        .unwrap_or_else(|error| ic_cdk::trap(format!("deposit intent read failed: {error}")))
        .unwrap_or_else(|| ic_cdk::trap("missing deposit intent"))
        .base_recipient;
    crate::api::prepare_mint_in_store_and_scan(
        store,
        deposit_id.bytes(),
        block_index,
        recipient,
        config,
        scan_target,
    )
    .unwrap_or_else(|error| ic_cdk::trap(format!("mint preparation failed: {error:?}")));
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
                    return Ok(EvmAdvance::Stopped(SettlementStopReason::NonceUnavailable))
                }
                Err(NonceInitializationError::Storage) => {
                    return Err(SettlementActionError::StorageFailure)
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
                            ))
                        }
                    }
                }
            };
            lease.renew_before_external_call()?;
            let broadcast = match evm_rpc::broadcast(config, &raw).await {
                Ok(outcome) => outcome,
                Err(error) => return Ok(EvmAdvance::Stopped(map_observation_stop(error))),
            };
            lease.ensure_current()?;
            if let evm_rpc::BroadcastOutcome::NonceConflict(rpc_audit) = &broadcast {
                let decision = evm_rpc::nonce_conflict_decision(
                    "broadcast_evm_operation",
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
                })?;
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
                current
                    .apply(EvmOperationEvent::Submitted { transaction_hash })
                    .map_err(|_| SettlementActionError::StorageFailure)?;
                let submitted_at_ns = ic_cdk::api::time();
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
            let outcome = match evm_rpc::confirmed_receipt_outcome(config, transaction_hash).await {
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
            match outcome {
                evm_rpc::ConfirmedReceiptOutcome::Missing => {
                    Ok(EvmAdvance::Waiting(transaction_hash))
                }
                evm_rpc::ConfirmedReceiptOutcome::Succeeded {
                    receipt_block_number,
                    finalized_head_block_number,
                    rpc_audit,
                } => {
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
                            return Ok(EvmAdvance::Waiting(transaction_hash));
                        }
                        ConfirmationEvidence::Verified => {}
                    }
                    STORE.with(|store| {
                        let mut store = store.borrow_mut();
                        let current = store
                            .evm_operation(operation_id.get())
                            .map_err(|_| SettlementActionError::StorageFailure)?
                            .ok_or(SettlementActionError::NotFound)?;
                        confirm_evm_member(
                            &mut store,
                            current,
                            transaction_hash,
                            receipt_block_number,
                            finalized_head_block_number,
                            vec![
                                crate::rpc_audit_event_kind(&rpc_audit),
                                crate::rpc_decision_event_kind(
                                    &evm_rpc::quorum_continued_decision(
                                        "confirm_evm_operation",
                                        Some(transaction_hash),
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
                            return Ok(EvmAdvance::Waiting(transaction_hash));
                        }
                        ConfirmationEvidence::Verified => {}
                    }
                    mark_evm_reverted(
                        operation,
                        transaction_hash,
                        receipt_block_number,
                        finalized_head_block_number,
                        vec![
                            crate::rpc_audit_event_kind(&rpc_audit),
                            crate::rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                                "confirm_evm_operation",
                                Some(transaction_hash),
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
            LedgerOperation::ReleaseWithdrawal => {
                ledger::release(config.ledger_canister_id, &hold.transfer).await
            }
            LedgerOperation::FeePayout => {
                return Ok(HoldAdvance::Stopped(
                    SettlementStopReason::LedgerUnavailable,
                ))
            }
        };
        lease.ensure_current()?;
        let Some(block_index) = outcome.confirmed_block() else {
            return Ok(HoldAdvance::Stopped(ledger_stop(&outcome)));
        };
        STORE.with(|store| {
            let mut store = store.borrow_mut();
            match hold.request {
                RequestReference::Deposit(id) => advance_deposit_hold(
                    &mut store,
                    config,
                    id,
                    hold.id,
                    DepositHoldResolution::Succeeded {
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
            resolve_reconciliation_success(config, target, block_index);
            Ok(HoldAdvance::Continue)
        }
        ledger::ReconciliationOutcome::Absent {
            ledger_watermark,
            index_watermark,
        } => {
            if index_watermark < ledger_watermark {
                return Ok(HoldAdvance::Progress);
            }
            resolve_reconciliation_absence(config, target, hold.transfer, index_watermark);
            Ok(HoldAdvance::Continue)
        }
    }
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
            bridge_core::DepositState::PullPending => {
                lease.renew_before_external_call()?;
                let outcome = ledger::pull(config.ledger_canister_id, &deposit.transfer).await;
                lease.ensure_current()?;
                match outcome {
                    LedgerCallOutcome::Succeeded { block_index }
                    | LedgerCallOutcome::Duplicate { block_index } => {
                        let recipient = STORE.with(|store| {
                            store
                                .borrow()
                                .deposit_intent(deposit_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .map(|intent| intent.base_recipient)
                                .ok_or(SettlementActionError::StorageFailure)
                        })?;
                        crate::api::prepare_mint(deposit_id, block_index, recipient, &config)
                            .map_err(|_| SettlementActionError::StorageFailure)?;
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
                                .apply(DepositEvent::PullAmbiguous { hold_id })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            let hold = ReconciliationHoldRecord::open(
                                hold_id,
                                RequestReference::Deposit(current.id),
                                current.transfer.clone(),
                            );
                            store
                                .commit_deposit_hold_bundle(&current, &hold)
                                .map_err(|_| SettlementActionError::StorageFailure)
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: SettlementState::Deposit(DepositPhase::ReconciliationHold),
                            reason: SettlementStopReason::LedgerAmbiguous,
                        });
                    }
                    LedgerCallOutcome::DefinitiveFailure { code } => {
                        STORE.with(|store| {
                            crate::api::cancel_deposit_in_store(
                                &mut store.borrow_mut(),
                                deposit_id,
                                code,
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
                        })
                    }
                }
            }
            bridge_core::DepositState::Escrowed { ledger_block_index } => {
                let recipient = STORE.with(|store| {
                    store
                        .borrow()
                        .deposit_intent(deposit_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .map(|intent| intent.base_recipient)
                        .ok_or(SettlementActionError::StorageFailure)
                })?;
                crate::api::prepare_mint(deposit_id, ledger_block_index, recipient, &config)
                    .map_err(|_| SettlementActionError::StorageFailure)?;
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
            bridge_core::DepositState::ReconciliationHold { hold_id } => {
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
                        return Ok(SettlementActionResult::ReconciliationProgress { state })
                    }
                    HoldAdvance::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason })
                    }
                }
            }
            bridge_core::DepositState::Minted { .. }
            | bridge_core::DepositState::MintReverted { .. }
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
                    LedgerCallOutcome::DefinitiveFailure {
                        code: bridge_core::LedgerFailure::BadFee { expected_fee },
                    } => {
                        let next_state = STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut current = store
                                .withdrawal(withdrawal_id)
                                .map_err(|_| SettlementActionError::StorageFailure)?
                                .ok_or(SettlementActionError::NotFound)?;
                            let (current_attempt, current_settlement) = match &current.state {
                                WithdrawalState::ReleasePending {
                                    attempt,
                                    settlement,
                                } => (attempt.clone(), *settlement),
                                _ => return Err(SettlementActionError::Busy),
                            };
                            if expected_fee > current.charged_service_fee {
                                return Ok::<_, SettlementActionError>(
                                    SettlementState::Withdrawal(WithdrawalPhase::ReleasePending),
                                );
                            }
                            let created_at_time_ns = ic_cdk::api::time().max(
                                current_attempt
                                    .identity
                                    .created_at_time_ns
                                    .saturating_add(1),
                            );
                            let mut digest = Sha256::new();
                            digest.update(b"KINIC-WITHDRAWAL-BAD-FEE");
                            digest.update(current.payload_hash);
                            digest.update(current_attempt.attempt_no.to_be_bytes());
                            digest.update(expected_fee.get().to_be_bytes());
                            let mut identity = current_attempt.identity.clone();
                            identity.created_at_time_ns = created_at_time_ns;
                            identity.memo = digest.finalize().into();
                            identity.amount = current.amount_out;
                            identity.fee = expected_fee;
                            let next_attempt = bridge_core::TransferAttempt {
                                attempt_no: current_attempt
                                    .attempt_no
                                    .checked_add(1)
                                    .ok_or(SettlementActionError::StorageFailure)?,
                                identity,
                            };
                            current
                                .apply(WithdrawalEvent::RepriceRelease {
                                    attempt: Box::new(next_attempt),
                                    settlement: bridge_core::Settlement {
                                        amount_out: current.amount_out,
                                        service_fee: current_settlement.service_fee,
                                        ledger_fee: expected_fee,
                                    },
                                })
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            store
                                .put_withdrawal(&current)
                                .map_err(|_| SettlementActionError::StorageFailure)?;
                            Ok::<_, SettlementActionError>(SettlementState::Withdrawal(
                                WithdrawalPhase::ReleasePending,
                            ))
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: next_state,
                            reason: SettlementStopReason::LedgerFeeChanged,
                        });
                    }
                    other => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: ledger_stop(&other),
                        })
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
                        return Ok(SettlementActionResult::ReconciliationProgress { state })
                    }
                    HoldAdvance::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason })
                    }
                }
            }
            WithdrawalState::Paid { .. } => return Ok(SettlementActionResult::Complete { state }),
            WithdrawalState::Observed => {
                lease.renew_before_external_call()?;
                let ledger_fee = match ledger::ledger_fee(config.ledger_canister_id).await {
                    Ok(fee) => fee,
                    Err(()) => {
                        return Ok(SettlementActionResult::Stopped {
                            state,
                            reason: SettlementStopReason::LedgerUnavailable,
                        })
                    }
                };
                lease.ensure_current()?;
                if ledger_fee > withdrawal.charged_service_fee {
                    return Ok(SettlementActionResult::Stopped {
                        state,
                        reason: SettlementStopReason::LedgerFeeChanged,
                    });
                }
                let identity = LedgerTransferIdentity {
                    operation: LedgerOperation::ReleaseWithdrawal,
                    created_at_time_ns: ic_cdk::api::time(),
                    memo: withdrawal.payload_hash,
                    amount: withdrawal.amount_out,
                    fee: ledger_fee,
                    from: Account::new(ic_cdk::api::canister_self().as_slice().to_vec(), [0; 32])
                        .map_err(|_| SettlementActionError::StorageFailure)?,
                    to: Account::new(withdrawal.owner.clone(), withdrawal.subaccount)
                        .map_err(|_| SettlementActionError::StorageFailure)?,
                    spender: None,
                };
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .withdrawal(withdrawal_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    current
                        .apply(WithdrawalEvent::StartRelease {
                            attempt: Box::new(TransferAttempt {
                                attempt_no: 0,
                                identity,
                            }),
                            settlement: Settlement {
                                amount_out: current.amount_out,
                                service_fee: current.charged_service_fee,
                                ledger_fee,
                            },
                        })
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    current.last_settlement_stop_reason = None;
                    store
                        .put_withdrawal(&current)
                        .map_err(|_| SettlementActionError::StorageFailure)
                })?;
            }
        }
    }
}

pub(crate) async fn advance_fee_payout(
    payout_id: u64,
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
            let outcome = ledger::release(config.ledger_canister_id, &payout.transfer).await;
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
                let outcome = ledger::release(config.ledger_canister_id, &payout.transfer).await;
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
            match ledger::reconcile_step(
                config.ledger_canister_id,
                config.index_canister_id,
                progress,
            )
            .await
            {
                ledger::ReconciliationOutcome::Progress(progress) => {
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
                    resolve_reconciliation_success(&config, target, block_index);
                    Ok(FeePayoutActionResult::Complete {
                        state: crate::admin::FeePayoutState::Succeeded { block_index },
                    })
                }
                ledger::ReconciliationOutcome::Absent {
                    ledger_watermark,
                    index_watermark,
                } => {
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
        };
        let envelope = single_envelope(operation, intent, 9).expect("single envelope");
        assert_eq!(envelope.operation_id, operation_id);
        assert_eq!(envelope.operation_id, EvmOperationId::new(1));
        assert_eq!(envelope.nonce, 9);
        assert_eq!(envelope.gas_limit, 100_000);
        assert_eq!(envelope.calldata, calldata);
    }
}
