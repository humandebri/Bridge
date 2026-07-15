use crate::{evm_rpc, ledger, signer, storage_or_trap, STORE};
use bridge_core::{
    DepositEvent, DepositHoldResolution, EvmOperationEvent, EvmOperationKind, EvmOperationState,
    LedgerCallOutcome, LedgerOperation, LedgerTransferIdentity, ReconciliationHoldRecord,
    ReconciliationScanProgress, ReconciliationTarget, RequestReference, WithdrawalEvent,
    WithdrawalHoldResolution, WithdrawalId, WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Deserialize};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

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
    ConfirmationCheckExhausted,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SettlementActionResult {
    Complete {
        state: String,
    },
    Submitted {
        state: String,
        transaction_hash: Vec<u8>,
    },
    WaitingForConfirmation {
        state: String,
        transaction_hash: Vec<u8>,
    },
    ReconciliationProgress {
        state: String,
    },
    Stopped {
        state: String,
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
            SettlementStopReason::ConfirmationCheckExhausted => {
                "Automatic Base confirmation checks were exhausted".into()
            }
        }),
        _ => None,
    }
}

pub(crate) enum NonceInitializationError {
    Observation,
    Storage,
}

pub(crate) async fn ensure_nonce_initialized(
    config: &crate::config::BridgeInitArgs,
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
            let derived = signer::ethereum_address(config).await.map_err(|error| {
                ic_cdk::println!("failed to derive bridge signer address: {error:?}");
                NonceInitializationError::Observation
            })?;
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .set_signer_address_if_absent(derived)
                    .map_err(|_| NonceInitializationError::Storage)
            })?
        }
    };
    let nonce = evm_rpc::transaction_count(config, address)
        .await
        .map_err(|error| {
            ic_cdk::println!("failed to observe bridge signer nonce: {error:?}");
            NonceInitializationError::Observation
        })?;
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
    config: &crate::config::BridgeInitArgs,
    withdrawal_id: WithdrawalId,
    hold_id: bridge_core::HoldId,
    resolution: WithdrawalHoldResolution,
    scan_target: Option<&ReconciliationTarget>,
) {
    let WithdrawalHoldResolution::Succeeded { ledger_block_index } = resolution else {
        store
            .resolve_withdrawal_hold_and_scan(withdrawal_id, hold_id, resolution, scan_target)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!(
                    "withdrawal reconciliation persistence failed: {error}"
                ))
            });
        return;
    };
    let mut withdrawal = store
        .withdrawal(withdrawal_id.bytes())
        .unwrap_or_else(|error| ic_cdk::trap(format!("withdrawal read failed: {error}")))
        .unwrap_or_else(|| ic_cdk::trap("missing withdrawal"));
    let mut hold = store
        .reconciliation_hold(hold_id.get())
        .unwrap_or_else(|error| ic_cdk::trap(format!("withdrawal hold read failed: {error}")))
        .unwrap_or_else(|| ic_cdk::trap("missing withdrawal hold"));
    bridge_core::resolve_withdrawal_hold(
        &mut withdrawal,
        &mut hold,
        WithdrawalHoldResolution::Succeeded { ledger_block_index },
    )
    .unwrap_or_else(|error| ic_cdk::trap(format!("withdrawal reconciliation failed: {error}")));
    prepare_acknowledgement_in_store_and_scan(store, config, &mut withdrawal, scan_target)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!("acknowledgement preparation failed: {error}"))
        });
}

fn confirm_evm_member(
    store: &mut crate::storage::StableStore,
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    confirmed_block_number: u64,
) -> Result<(), ()> {
    operation
        .apply(EvmOperationEvent::Confirmed {
            transaction_hash,
            receipt_block_number,
            confirmed_block_number,
        })
        .map_err(|_| ())?;
    let mut progress = store.external_progress().map_err(|_| ())?;
    progress.last_safe_base_block = progress.last_safe_base_block.max(confirmed_block_number);
    if operation.kind == EvmOperationKind::MintDeposit {
        progress.last_safe_mint_block = progress.last_safe_mint_block.max(confirmed_block_number);
    }
    progress.last_safe_observation_ns = ic_cdk::api::time();
    store
        .commit_evm_terminal_bundle(&operation, &progress, None)
        .map_err(|_| ())
}

fn mark_evm_reverted(
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    confirmed_block_number: u64,
) {
    operation
        .apply(EvmOperationEvent::Reverted {
            transaction_hash,
            receipt_block_number,
            confirmed_block_number,
        })
        .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert transition failed: {error}")));
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut progress = store.external_progress().unwrap_or_else(|error| {
            ic_cdk::trap(format!("external progress read failed: {error}"))
        });
        progress.last_safe_base_block = progress.last_safe_base_block.max(confirmed_block_number);
        progress.last_safe_observation_ns = ic_cdk::api::time();
        store
            .commit_evm_terminal_bundle(
                &operation,
                &progress,
                Some((
                    ic_cdk::api::canister_self(),
                    ic_cdk::api::time(),
                    confirmed_block_number,
                )),
            )
            .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert bundle failed: {error}")));
    });
}

pub(crate) fn prepare_acknowledgement_in_store(
    store: &mut crate::storage::StableStore,
    config: &crate::config::BridgeInitArgs,
    withdrawal: &mut WithdrawalRecord,
) -> Result<(), crate::storage::StorageError> {
    prepare_acknowledgement_in_store_and_scan(store, config, withdrawal, None)
}

fn prepare_acknowledgement_in_store_and_scan(
    store: &mut crate::storage::StableStore,
    config: &crate::config::BridgeInitArgs,
    withdrawal: &mut WithdrawalRecord,
    scan_target: Option<&ReconciliationTarget>,
) -> Result<(), crate::storage::StorageError> {
    let (settlement, ledger_block_index) = match &withdrawal.state {
        WithdrawalState::ReleaseTransferred {
            settlement,
            ledger_block_index,
            ..
        } => (*settlement, *ledger_block_index),
        WithdrawalState::AcknowledgePending { .. }
        | WithdrawalState::AcknowledgeReverted { .. }
        | WithdrawalState::Released { .. } => return Ok(()),
        _ => {
            return Err(bridge_core::CoreError::InvalidTransition {
                entity: "withdrawal",
                event: "prepare_acknowledgement",
            }
            .into())
        }
    };
    let operation_id = store.next_evm_operation_id()?;
    withdrawal.apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })?;
    let operation = bridge_core::EvmOperationRecord::queued(
        operation_id,
        withdrawal.payload_hash,
        EvmOperationKind::AcknowledgeRelease,
    );
    let mut selector_hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(b"acknowledgeRelease(uint256,uint256,uint256,uint256,uint256)");
    keccak.finalize(&mut selector_hash);
    let mut calldata = selector_hash[..4].to_vec();
    calldata.extend_from_slice(&withdrawal.id.bytes());
    for value in [
        settlement.amount_out.get(),
        settlement.service_fee.get(),
        settlement.ledger_fee.get(),
        ledger_block_index,
    ] {
        calldata.extend_from_slice(&[0; 16]);
        calldata.extend_from_slice(&value.to_be_bytes());
    }
    let intent = bridge_core::EvmCallIntent {
        operation_id,
        payload_hash: withdrawal.payload_hash,
        chain_id: config.base_chain_id,
        contract: config.contract_array(),
        calldata,
        gas_limit: config.transaction_gas_limit,
        max_fee_per_gas: config.max_fee_per_gas,
        max_priority_fee_per_gas: config.max_priority_fee_per_gas,
    };
    store.commit_acknowledgement_bundle_and_scan(withdrawal, &operation, &intent, scan_target)
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

fn deposit_state_name(state: &bridge_core::DepositState) -> String {
    use bridge_core::DepositState as State;
    match state {
        State::PullPending => "PullPending",
        State::Escrowed { .. } => "Escrowed",
        State::MintPending { .. } => "MintPending",
        State::Minted { .. } => "Minted",
        State::MintReverted { .. } => "MintReverted",
        State::ReconciliationHold { .. } => "ReconciliationHold",
        State::Cancelled { .. } => "Cancelled",
    }
    .into()
}

fn withdrawal_state_name(state: &WithdrawalState) -> String {
    match state {
        WithdrawalState::Observed => "Observed",
        WithdrawalState::ReleasePending { .. } => "ReleasePending",
        WithdrawalState::ReleaseTransferred { .. } => "ReleaseTransferred",
        WithdrawalState::AcknowledgePending { .. } => "AcknowledgePending",
        WithdrawalState::AcknowledgeReverted { .. } => "AcknowledgeReverted",
        WithdrawalState::Released { .. } => "Released",
        WithdrawalState::RefundPending { .. } => "RefundPending",
        WithdrawalState::RefundReverted { .. } => "RefundReverted",
        WithdrawalState::Refunded { .. } => "Refunded",
        WithdrawalState::ReconciliationHold { .. } => "ReconciliationHold",
        WithdrawalState::ReleaseCancellationPending { .. } => "ReleaseCancellationPending",
        WithdrawalState::ReleaseCancelled { .. } => "ReleaseCancelled",
    }
    .into()
}

fn map_observation_stop(error: evm_rpc::ObservationError) -> SettlementStopReason {
    match error {
        evm_rpc::ObservationError::Rpc => SettlementStopReason::RpcUnavailable,
        evm_rpc::ObservationError::Inconsistent => SettlementStopReason::RpcInconsistent,
        evm_rpc::ObservationError::InvalidResponse | evm_rpc::ObservationError::Overflow => {
            SettlementStopReason::InvalidBaseResponse
        }
        evm_rpc::ObservationError::BaseStateMismatch => SettlementStopReason::BaseStateMismatch,
        evm_rpc::ObservationError::NonceConflict => SettlementStopReason::NonceConflict,
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
            match ensure_nonce_initialized(config).await {
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
            Box::pin(advance_evm_operation(config, operation_id)).await
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
                None => match signer::sign(&envelope, config).await {
                    Ok(raw) => {
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
                },
            };
            if let Err(error) = evm_rpc::broadcast(config, &raw).await {
                if matches!(error, evm_rpc::ObservationError::NonceConflict) {
                    STORE.with(|store| {
                        let mut store = store.borrow_mut();
                        let mut admin = store
                            .admin_state()
                            .map_err(|_| SettlementActionError::StorageFailure)?;
                        admin.deposits_paused = true;
                        store
                            .set_admin_state(&admin)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    return Ok(EvmAdvance::Stopped(SettlementStopReason::NonceConflict));
                }
                return Ok(EvmAdvance::Stopped(map_observation_stop(error)));
            }
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
                    .put_submitted_evm_operation(
                        &current,
                        submitted_at_ns,
                        submitted_at_ns.saturating_add(
                            crate::scheduler::confirmation_delay_ns(current.kind, 0)
                                .expect("every EVM operation has a confirmation schedule"),
                        ),
                    )
                    .map_err(|_| SettlementActionError::StorageFailure)
            })?;
            Ok(EvmAdvance::Submitted(transaction_hash))
        }
        EvmOperationState::Submitted { transaction_hash } => {
            let outcome = match evm_rpc::confirmed_receipt_outcome(config, transaction_hash).await {
                Ok(outcome) => outcome,
                Err(error) => return Ok(EvmAdvance::Stopped(map_observation_stop(error))),
            };
            match outcome {
                evm_rpc::ConfirmedReceiptOutcome::Missing => {
                    Ok(EvmAdvance::Waiting(transaction_hash))
                }
                evm_rpc::ConfirmedReceiptOutcome::Succeeded {
                    receipt_block_number,
                    confirmed_block_number,
                } => {
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
                            confirmed_block_number,
                        )
                        .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(EvmAdvance::Complete)
                }
                evm_rpc::ConfirmedReceiptOutcome::Reverted {
                    receipt_block_number,
                    confirmed_block_number,
                } => {
                    mark_evm_reverted(
                        operation,
                        transaction_hash,
                        receipt_block_number,
                        confirmed_block_number,
                    );
                    Ok(EvmAdvance::Complete)
                }
            }
        }
        EvmOperationState::Confirmed { .. } => Ok(EvmAdvance::Complete),
        EvmOperationState::Reverted { .. } => Ok(EvmAdvance::Stopped(
            SettlementStopReason::TransactionReverted,
        )),
    }
}

async fn advance_hold(
    config: &crate::config::BridgeInitArgs,
    hold: ReconciliationHoldRecord,
) -> Result<HoldAdvance, SettlementActionError> {
    if ic_cdk::api::time().saturating_sub(hold.transfer.created_at_time_ns) <= LEDGER_DEDUP_NS {
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
        let state = deposit_state_name(&deposit.state);
        match deposit.state {
            bridge_core::DepositState::PullPending => {
                let outcome = ledger::pull(config.ledger_canister_id, &deposit.transfer).await;
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
                            state: "ReconciliationHold".into(),
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
                            state: "Cancelled".into(),
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
                return match advance_evm_operation(&config, operation_id).await? {
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
                match advance_hold(&config, hold).await? {
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
        let state = withdrawal_state_name(&withdrawal.state);
        match withdrawal.state {
            WithdrawalState::ReleaseCancellationPending { operation_id, .. } => {
                return match advance_evm_operation(&config, operation_id).await? {
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
            WithdrawalState::ReleasePending { attempt, .. } => {
                let outcome = ledger::release(config.ledger_canister_id, &attempt.identity).await;
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
                            prepare_acknowledgement_in_store(&mut store, &config, &mut current)
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
                            state: "ReconciliationHold".into(),
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
                            let repriced_amount = current
                                .amount
                                .get()
                                .checked_sub(current_settlement.service_fee.get())
                                .and_then(|value| value.checked_sub(expected_fee.get()));
                            if repriced_amount
                                .is_some_and(|value| value >= current.min_amount_out.get())
                            {
                                let amount_out =
                                    bridge_core::Amount::new(repriced_amount.expect("checked"));
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
                                identity.amount = amount_out;
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
                                            amount_out,
                                            service_fee: current_settlement.service_fee,
                                            ledger_fee: expected_fee,
                                        },
                                    })
                                    .map_err(|_| SettlementActionError::StorageFailure)?;
                                store
                                    .put_withdrawal(&current)
                                    .map_err(|_| SettlementActionError::StorageFailure)?;
                                Ok::<_, SettlementActionError>("ReleasePending")
                            } else {
                                let operation_id = store
                                    .next_evm_operation_id()
                                    .map_err(|_| SettlementActionError::StorageFailure)?;
                                current
                                    .apply(WithdrawalEvent::PrepareReleaseCancellation {
                                        operation_id,
                                        expected_ledger_fee: expected_fee,
                                    })
                                    .map_err(|_| SettlementActionError::StorageFailure)?;
                                let operation = bridge_core::EvmOperationRecord::queued(
                                    operation_id,
                                    current.payload_hash,
                                    EvmOperationKind::CancelRelease,
                                );
                                let mut selector_hash = [0u8; 32];
                                let mut keccak = Keccak::v256();
                                keccak.update(b"cancelRelease(uint256)");
                                keccak.finalize(&mut selector_hash);
                                let mut calldata = selector_hash[..4].to_vec();
                                calldata.extend_from_slice(&current.id.bytes());
                                let intent = bridge_core::EvmCallIntent {
                                    operation_id,
                                    payload_hash: current.payload_hash,
                                    chain_id: config.base_chain_id,
                                    contract: config.contract_array(),
                                    calldata,
                                    gas_limit: config.transaction_gas_limit,
                                    max_fee_per_gas: config.max_fee_per_gas,
                                    max_priority_fee_per_gas: config.max_priority_fee_per_gas,
                                };
                                store
                                    .commit_withdrawal_operation_bundle(
                                        &current, &operation, &intent,
                                    )
                                    .map_err(|_| SettlementActionError::StorageFailure)?;
                                Ok::<_, SettlementActionError>("ReleaseCancellationPending")
                            }
                        })?;
                        return Ok(SettlementActionResult::Stopped {
                            state: next_state.into(),
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
            WithdrawalState::ReleaseTransferred { .. } => {
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .withdrawal(withdrawal_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    prepare_acknowledgement_in_store(&mut store, &config, &mut current)
                        .map_err(|_| SettlementActionError::StorageFailure)
                })?;
            }
            WithdrawalState::AcknowledgePending { operation_id, .. }
            | WithdrawalState::RefundPending { operation_id, .. } => {
                return match advance_evm_operation(&config, operation_id).await? {
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
            WithdrawalState::ReconciliationHold { hold_id, .. } => {
                let hold = STORE.with(|store| {
                    store
                        .borrow()
                        .reconciliation_hold(hold_id.get())
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)
                })?;
                match advance_hold(&config, hold).await? {
                    HoldAdvance::Continue => continue,
                    HoldAdvance::Progress => {
                        return Ok(SettlementActionResult::ReconciliationProgress { state })
                    }
                    HoldAdvance::Stopped(reason) => {
                        return Ok(SettlementActionResult::Stopped { state, reason })
                    }
                }
            }
            WithdrawalState::Released { .. }
            | WithdrawalState::Refunded { .. }
            | WithdrawalState::AcknowledgeReverted { .. }
            | WithdrawalState::RefundReverted { .. } => {
                return Ok(SettlementActionResult::Complete { state })
            }
            WithdrawalState::Observed => {
                return Ok(SettlementActionResult::Stopped {
                    state,
                    reason: SettlementStopReason::InvalidBaseResponse,
                })
            }
            WithdrawalState::ReleaseCancelled { .. } => {
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut current = store
                        .withdrawal(withdrawal_id)
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .ok_or(SettlementActionError::NotFound)?;
                    let operation_id = store
                        .next_evm_operation_id()
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    let confirmed_base_block = store
                        .external_progress()
                        .map_err(|_| SettlementActionError::StorageFailure)?
                        .last_safe_base_block;
                    current
                        .apply(WithdrawalEvent::StartRefund {
                            operation_id,
                            eligibility: bridge_core::RefundEligibility {
                                confirmed_base_block,
                                base_status_pending: true,
                                release_transfer_proven_absent: true,
                                reason: bridge_core::RefundReason::AmountBelowMinimum,
                            },
                        })
                        .map_err(|_| SettlementActionError::StorageFailure)?;
                    let operation = bridge_core::EvmOperationRecord::queued(
                        operation_id,
                        current.payload_hash,
                        EvmOperationKind::RefundWithdrawal,
                    );
                    let mut selector_hash = [0u8; 32];
                    let mut keccak = Keccak::v256();
                    keccak.update(b"refundWithdrawal(uint256)");
                    keccak.finalize(&mut selector_hash);
                    let mut calldata = selector_hash[..4].to_vec();
                    calldata.extend_from_slice(&current.id.bytes());
                    let intent = bridge_core::EvmCallIntent {
                        operation_id,
                        payload_hash: current.payload_hash,
                        chain_id: config.base_chain_id,
                        contract: config.contract_array(),
                        calldata,
                        gas_limit: config.transaction_gas_limit,
                        max_fee_per_gas: config.max_fee_per_gas,
                        max_priority_fee_per_gas: config.max_priority_fee_per_gas,
                    };
                    store
                        .commit_withdrawal_operation_bundle(&current, &operation, &intent)
                        .map_err(|_| SettlementActionError::StorageFailure)
                })?;
            }
        }
    }
}

pub(crate) async fn advance_fee_payout(
    payout_id: u64,
) -> Result<SettlementActionResult, SettlementActionError> {
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
        crate::admin::FeePayoutState::Succeeded { .. } => Ok(SettlementActionResult::Complete {
            state: "Succeeded".into(),
        }),
        crate::admin::FeePayoutState::Failed => Ok(SettlementActionResult::Complete {
            state: "Failed".into(),
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
                    Ok(SettlementActionResult::Complete {
                        state: "Succeeded".into(),
                    })
                }
                LedgerCallOutcome::Ambiguous => {
                    STORE.with(|store| {
                        store
                            .borrow_mut()
                            .hold_fee_payout(payout_id)
                            .map_err(|_| SettlementActionError::StorageFailure)
                    })?;
                    Ok(SettlementActionResult::Stopped {
                        state: "ReconciliationHold".into(),
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
                    Ok(SettlementActionResult::Stopped {
                        state: "Failed".into(),
                        reason: SettlementStopReason::LedgerRejected(format!("{code:?}")),
                    })
                }
                other => Ok(SettlementActionResult::Stopped {
                    state: "Pending".into(),
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
                    return Ok(SettlementActionResult::Complete {
                        state: "Succeeded".into(),
                    });
                }
                return Ok(SettlementActionResult::Stopped {
                    state: "ReconciliationHold".into(),
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
                    Ok(SettlementActionResult::ReconciliationProgress {
                        state: "ReconciliationHold".into(),
                    })
                }
                ledger::ReconciliationOutcome::Succeeded { block_index } => {
                    resolve_reconciliation_success(&config, target, block_index);
                    Ok(SettlementActionResult::Complete {
                        state: "Succeeded".into(),
                    })
                }
                ledger::ReconciliationOutcome::Absent {
                    ledger_watermark,
                    index_watermark,
                } => {
                    if index_watermark < ledger_watermark {
                        return Ok(SettlementActionResult::ReconciliationProgress {
                            state: "ReconciliationHold".into(),
                        });
                    }
                    resolve_reconciliation_absence(
                        &config,
                        target,
                        payout.transfer,
                        index_watermark,
                    );
                    Ok(SettlementActionResult::Complete {
                        state: "Failed".into(),
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
    fn evm_envelope_contains_exactly_one_operation_and_original_calldata() {
        let operation_id = EvmOperationId::new(1);
        let operation =
            EvmOperationRecord::queued(operation_id, [1; 32], EvmOperationKind::RefundWithdrawal);
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
