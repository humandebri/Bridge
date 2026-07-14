use crate::{evm_rpc, ledger, signer, storage_or_trap, STORE};
use bridge_core::{
    Account, Amount, DepositEvent, DepositHoldResolution, EvmOperationEvent, EvmOperationKind,
    EvmOperationState, FeeKind, LedgerCallOutcome, LedgerOperation, LedgerTransferIdentity,
    ReconciliationHoldRecord, RefundEligibility, RefundReason, RequestReference, Settlement,
    TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId, WithdrawalRecord,
    WithdrawalState,
};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

fn retry_memo(hold_id: u64, identity: &LedgerTransferIdentity) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"KINIC-WITHDRAWAL-RETRY");
    digest.update(hold_id.to_be_bytes());
    digest.update(identity.created_at_time_ns.to_be_bytes());
    digest.finalize().into()
}

pub async fn tick() {
    confirm_one_evm_operation().await;
    reconcile_one_hold().await;
    reconcile_fee_payout().await;
    process_one_withdrawal_notification().await;
    process_one_release().await;
    match ensure_nonce_initialized_from_store().await {
        Ok(()) => {}
        Err(NonceInitializationError::Observation) => return,
        Err(NonceInitializationError::Storage) => {
            ic_cdk::trap("nonce initialization storage failure")
        }
    }
    assign_one_evm_nonce();
    process_one_evm_operation().await;
    process_one_deposit_pull().await;
}

async fn ensure_nonce_initialized_from_store() -> Result<(), NonceInitializationError> {
    let config = STORE
        .with(|store| store.borrow().config())
        .map_err(|_| NonceInitializationError::Storage)?
        .ok_or(NonceInitializationError::Storage)?;
    ensure_nonce_initialized(&config).await
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

async fn reconcile_fee_payout() {
    const DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let payout = storage_or_trap(
            "reconcilable fee payout read",
            store.first_reconcilable_fee_payout(ic_cdk::api::time(), DEDUP_NS),
        )?;
        Some((config, payout))
    });
    let Some((config, mut payout)) = candidate else {
        return;
    };
    let resolution = ledger::reconcile_history(config.ledger_canister_id, &payout.transfer).await;
    match resolution {
        ledger::HistoryResolution::Succeeded { block_index } => {
            payout.state = crate::admin::FeePayoutState::Succeeded { block_index };
            STORE.with(|store| {
                storage_or_trap(
                    "fee payout completion",
                    store
                        .borrow_mut()
                        .complete_fee_payout_success(payout.id, block_index),
                );
            });
        }
        ledger::HistoryResolution::Absent { watermark } => {
            let index = ledger::reconcile_index(
                config.index_canister_id,
                config.ledger_canister_id,
                &payout.transfer,
                watermark,
            )
            .await;
            if matches!(index,ledger::HistoryResolution::Absent{watermark:index_watermark} if index_watermark>=watermark && watermark>0)
            {
                payout.state = crate::admin::FeePayoutState::Failed;
                STORE.with(|store| {
                    storage_or_trap(
                        "failed fee payout persistence",
                        store.borrow_mut().put_fee_payout(&payout),
                    );
                });
            }
        }
        ledger::HistoryResolution::Incomplete => {}
    }
}

fn assign_one_evm_nonce() {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let has_prepared =
            storage_or_trap("prepared EVM operation read", store.first_prepared_evm()).is_some();
        let mut progress = storage_or_trap("external progress read", store.external_progress());
        if !bridge_core::can_assign_nonce(progress.nonce_initialized, has_prepared) {
            return;
        }
        let batch = storage_or_trap("queued EVM batch read", store.first_queued_evm_batch());
        if batch.is_empty() {
            return;
        }
        let nonce = progress.next_evm_nonce;
        let Some(next) = bridge_core::nonce_next(nonce) else {
            return;
        };
        let Some(envelope) = batch_envelope(&batch, nonce) else {
            return;
        };
        let mut operations = batch
            .into_iter()
            .map(|(operation, _)| operation)
            .collect::<Vec<_>>();
        if operations
            .iter_mut()
            .any(|operation| operation.apply(EvmOperationEvent::Prepared).is_err())
        {
            ic_cdk::trap("invalid EVM prepared transition");
        }
        storage_or_trap(
            "EVM envelope persistence",
            store.put_evm_envelope(&envelope),
        );
        for operation in &operations {
            storage_or_trap(
                "prepared EVM operation persistence",
                store.put_evm_operation(operation),
            );
        }
        progress.next_evm_nonce = next;
        store
            .set_external_progress(&progress)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("nonce assignment persistence failed: {error}"))
            });
    });
}

fn abi_word(value: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn batch_envelope(
    batch: &[(bridge_core::EvmOperationRecord, bridge_core::EvmCallIntent)],
    nonce: u64,
) -> Option<bridge_core::EvmTransactionEnvelope> {
    let (first_operation, first_intent) = batch.first()?;
    if batch.len() > 4
        || batch.iter().any(|(operation, intent)| {
            operation.kind != first_operation.kind
                || intent.chain_id != first_intent.chain_id
                || intent.contract != first_intent.contract
                || intent.max_fee_per_gas != first_intent.max_fee_per_gas
                || intent.max_priority_fee_per_gas != first_intent.max_priority_fee_per_gas
        })
    {
        return None;
    }
    let (signature, single_words) = match first_operation.kind {
        EvmOperationKind::MintDeposit => (
            "mintDeposits((bytes32,address,uint256,uint256,uint256)[])",
            5,
        ),
        EvmOperationKind::AcknowledgeRelease => (
            "acknowledgeReleases((uint256,uint256,uint256,uint256,uint256)[])",
            5,
        ),
        EvmOperationKind::RefundWithdrawal => ("refundWithdrawals(uint256[])", 1),
    };
    let expected_len = 4 + single_words * 32;
    if batch
        .iter()
        .any(|(_, intent)| intent.calldata.len() != expected_len)
    {
        return None;
    }
    let mut selector_hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut selector_hash);
    let mut calldata = selector_hash[..4].to_vec();
    calldata.extend_from_slice(&abi_word(32));
    calldata.extend_from_slice(&abi_word(batch.len() as u128));
    for (_, intent) in batch {
        calldata.extend_from_slice(&intent.calldata[4..]);
    }
    let mut digest = Sha256::new();
    for (operation, _) in batch {
        digest.update(operation.id.get().to_be_bytes());
        digest.update(operation.payload_hash);
    }
    Some(bridge_core::EvmTransactionEnvelope {
        operation_id: first_operation.id,
        operation_ids: batch.iter().map(|(operation, _)| operation.id).collect(),
        payload_hash: digest.finalize().into(),
        nonce,
        chain_id: first_intent.chain_id,
        contract: first_intent.contract,
        calldata,
        gas_limit: batch
            .iter()
            .try_fold(0u128, |sum, (_, intent)| sum.checked_add(intent.gas_limit))?,
        max_fee_per_gas: first_intent.max_fee_per_gas,
        max_priority_fee_per_gas: first_intent.max_priority_fee_per_gas,
        signed_transaction: None,
    })
}

async fn process_one_deposit_pull() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let deposit = storage_or_trap("pull-pending deposit read", store.first_pull_pending())?;
        let intent = storage_or_trap(
            "deposit intent read",
            store.deposit_intent(deposit.id.bytes()),
        )
        .unwrap_or_else(|| ic_cdk::trap("missing deposit intent"));
        Some((config, deposit, intent.base_recipient))
    });
    let Some((config, mut deposit, recipient)) = candidate else {
        return;
    };
    match ledger::pull(config.ledger_canister_id, &deposit.transfer).await {
        LedgerCallOutcome::Succeeded { block_index }
        | LedgerCallOutcome::Duplicate { block_index } => {
            crate::api::prepare_mint(deposit.id.bytes(), block_index, recipient, &config)
                .unwrap_or_else(|error| {
                    ic_cdk::trap(format!("mint preparation failed: {error:?}"))
                });
        }
        LedgerCallOutcome::Ambiguous => {
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let current = storage_or_trap("deposit read", store.deposit(deposit.id.bytes()))
                    .unwrap_or_else(|| ic_cdk::trap("missing pull-pending deposit"));
                if !matches!(current.state, bridge_core::DepositState::PullPending) {
                    return;
                }
                let hold_id = storage_or_trap("hold ID allocation", store.allocate_hold_id());
                deposit
                    .apply(DepositEvent::PullAmbiguous { hold_id })
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("deposit hold transition failed: {error}"))
                    });
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Deposit(deposit.id),
                    deposit.transfer.clone(),
                );
                storage_or_trap("held deposit persistence", store.put_deposit(&deposit));
                storage_or_trap(
                    "deposit hold persistence",
                    store.put_open_reconciliation_hold(&hold),
                );
            });
        }
        LedgerCallOutcome::DefinitiveFailure { code } => {
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                crate::api::cancel_deposit_in_store(&mut store, deposit.id.bytes(), code)
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("deposit cancellation failed: {error:?}"))
                    });
            });
        }
        LedgerCallOutcome::RetryableFailure { .. } => {}
    }
}

async fn reconcile_one_hold() {
    const DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let hold = storage_or_trap("open hold read", store.first_open_hold())?;
        Some((config, hold))
    });
    let Some((config, hold)) = candidate else {
        return;
    };
    if ic_cdk::api::time().saturating_sub(hold.transfer.created_at_time_ns) > DEDUP_NS {
        let resolution =
            match ledger::reconcile_history(config.ledger_canister_id, &hold.transfer).await {
                ledger::HistoryResolution::Succeeded { block_index } => Ok(block_index),
                ledger::HistoryResolution::Absent { watermark } => {
                    match ledger::reconcile_index(
                        config.index_canister_id,
                        config.ledger_canister_id,
                        &hold.transfer,
                        watermark,
                    )
                    .await
                    {
                        ledger::HistoryResolution::Succeeded { block_index } => Ok(block_index),
                        ledger::HistoryResolution::Absent {
                            watermark: index_watermark,
                        } if index_watermark >= watermark && watermark > 0 => {
                            Err((watermark, index_watermark))
                        }
                        _ => return,
                    }
                }
                ledger::HistoryResolution::Incomplete => return,
            };
        STORE.with(|store| {
            let mut store = store.borrow_mut();
            match hold.request {
                RequestReference::Deposit(id) => {
                    let resolution = match resolution {
                        Ok(ledger_block_index) => {
                            DepositHoldResolution::Succeeded { ledger_block_index }
                        }
                        Err((ledger_watermark, history_watermark)) => {
                            let scan = bridge_core::ReconciliationScanProgress {
                                hold_id: hold.id,
                                next_block: ledger_watermark,
                                ledger_tip: ledger_watermark - 1,
                                index_watermark: history_watermark.saturating_sub(1),
                                archives_complete: true,
                                matched_block: None,
                                transfer: hold.transfer.clone(),
                            };
                            if !scan.can_prove_absent() {
                                return;
                            }
                            storage_or_trap(
                                "deposit reconciliation scan persistence",
                                store.put_reconciliation_scan(&scan),
                            );
                            DepositHoldResolution::Absent { history_watermark }
                        }
                    };
                    advance_deposit_hold(&mut store, &config, id, hold.id, resolution);
                }
                RequestReference::Withdrawal(id) => {
                    let resolution = match resolution {
                        Ok(ledger_block_index) => {
                            WithdrawalHoldResolution::Succeeded { ledger_block_index }
                        }
                        Err((ledger_watermark, history_watermark)) => {
                            let scan = bridge_core::ReconciliationScanProgress {
                                hold_id: hold.id,
                                next_block: ledger_watermark,
                                ledger_tip: ledger_watermark - 1,
                                index_watermark: history_watermark.saturating_sub(1),
                                archives_complete: true,
                                matched_block: None,
                                transfer: hold.transfer.clone(),
                            };
                            if !scan.can_prove_absent() {
                                return;
                            }
                            storage_or_trap(
                                "withdrawal reconciliation scan persistence",
                                store.put_reconciliation_scan(&scan),
                            );
                            let mut next_identity = hold.transfer.clone();
                            next_identity.created_at_time_ns = ic_cdk::api::time()
                                .max(hold.transfer.created_at_time_ns.saturating_add(1));
                            next_identity.memo = retry_memo(hold.id.get(), &next_identity);
                            WithdrawalHoldResolution::Absent {
                                history_watermark,
                                next_identity: Box::new(next_identity),
                            }
                        }
                    };
                    advance_withdrawal_hold(&mut store, &config, id, hold.id, resolution);
                }
            }
        });
        return;
    }
    let outcome = match hold.transfer.operation {
        LedgerOperation::PullDeposit => {
            ledger::pull(config.ledger_canister_id, &hold.transfer).await
        }
        LedgerOperation::ReleaseWithdrawal => {
            ledger::release(config.ledger_canister_id, &hold.transfer).await
        }
        LedgerOperation::FeePayout => return,
    };
    let Some(block_index) = outcome.confirmed_block() else {
        return;
    };
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        match hold.request {
            RequestReference::Deposit(id) => {
                advance_deposit_hold(
                    &mut store,
                    &config,
                    id,
                    hold.id,
                    DepositHoldResolution::Succeeded {
                        ledger_block_index: block_index,
                    },
                );
            }
            RequestReference::Withdrawal(id) => {
                advance_withdrawal_hold(
                    &mut store,
                    &config,
                    id,
                    hold.id,
                    WithdrawalHoldResolution::Succeeded {
                        ledger_block_index: block_index,
                    },
                );
            }
        }
    });
}

fn advance_deposit_hold(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    config: &crate::config::BridgeInitArgs,
    deposit_id: bridge_core::DepositId,
    hold_id: bridge_core::HoldId,
    resolution: DepositHoldResolution,
) {
    let succeeded = match resolution {
        DepositHoldResolution::Succeeded { ledger_block_index } => Some(ledger_block_index),
        DepositHoldResolution::Absent { .. } => None,
    };
    store
        .resolve_deposit_hold(deposit_id, hold_id, resolution)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!(
                "deposit reconciliation persistence failed: {error}"
            ))
        });
    let Some(block_index) = succeeded else {
        return;
    };
    let recipient = store
        .deposit_intent(deposit_id.bytes())
        .unwrap_or_else(|error| ic_cdk::trap(format!("deposit intent read failed: {error}")))
        .unwrap_or_else(|| ic_cdk::trap("missing deposit intent"))
        .base_recipient;
    crate::api::prepare_mint_in_store(store, deposit_id.bytes(), block_index, recipient, config)
        .unwrap_or_else(|error| ic_cdk::trap(format!("mint preparation failed: {error:?}")));
}

fn advance_withdrawal_hold(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    config: &crate::config::BridgeInitArgs,
    withdrawal_id: WithdrawalId,
    hold_id: bridge_core::HoldId,
    resolution: WithdrawalHoldResolution,
) {
    let succeeded = matches!(resolution, WithdrawalHoldResolution::Succeeded { .. });
    let result = store
        .resolve_withdrawal_hold(withdrawal_id, hold_id, resolution)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!(
                "withdrawal reconciliation persistence failed: {error}"
            ))
        });
    if !succeeded {
        return;
    }
    let mut accounting = store
        .accounting()
        .unwrap_or_else(|error| ic_cdk::trap(format!("accounting read failed: {error}")));
    accounting
        .confirm_fee(FeeKind::Withdrawal, result.fee_delta)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!("withdrawal fee confirmation failed: {error}"))
        });
    store
        .set_accounting(&accounting)
        .unwrap_or_else(|error| ic_cdk::trap(format!("accounting persistence failed: {error}")));
    let mut withdrawal = store
        .withdrawal(withdrawal_id.bytes())
        .unwrap_or_else(|error| ic_cdk::trap(format!("withdrawal read failed: {error}")))
        .unwrap_or_else(|| ic_cdk::trap("missing withdrawal"));
    prepare_acknowledgement_in_store(store, config, &mut withdrawal).unwrap_or_else(|error| {
        ic_cdk::trap(format!("acknowledgement preparation failed: {error}"))
    });
}

async fn process_one_withdrawal_notification() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let notification = storage_or_trap(
            "due withdrawal notification read",
            store.first_due_withdrawal_notification(ic_cdk::api::time()),
        )?;
        Some((config, notification))
    });
    let Some((config, notification)) = candidate else {
        return;
    };
    let outcome = match evm_rpc::notified_withdrawal_outcome(&config, notification.transaction_hash)
        .await
    {
        Ok(outcome) => outcome,
        Err(evm_rpc::ObservationError::InvalidResponse | evm_rpc::ObservationError::Overflow) => {
            discard_withdrawal_notification(notification.transaction_hash);
            return;
        }
        Err(evm_rpc::ObservationError::Rpc | evm_rpc::ObservationError::Inconsistent) => {
            retry_withdrawal_notification(notification);
            return;
        }
    };
    let (observed, snapshot, finalized_block_number) = match outcome {
        evm_rpc::NotifiedWithdrawalOutcome::Missing
        | evm_rpc::NotifiedWithdrawalOutcome::Pending { .. } => {
            retry_withdrawal_notification(notification);
            return;
        }
        evm_rpc::NotifiedWithdrawalOutcome::Reverted { .. } => {
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .remove_withdrawal_notification(notification.transaction_hash)
            });
            return;
        }
        evm_rpc::NotifiedWithdrawalOutcome::Finalized {
            withdrawal,
            snapshot,
            finalized_block_number,
            ..
        } => (withdrawal, snapshot, finalized_block_number),
    };
    let Ok(owner) = candid::Principal::try_from_slice(&observed.owner) else {
        discard_withdrawal_notification(notification.transaction_hash);
        return;
    };
    if owner != notification.caller {
        discard_withdrawal_notification(notification.transaction_hash);
        return;
    }
    let already_known = STORE.with(|store| {
        storage_or_trap("withdrawal read", store.borrow().withdrawal(observed.id)).is_some()
    });
    if already_known {
        discard_withdrawal_notification(notification.transaction_hash);
        return;
    }
    let Ok(ledger_fee) = ledger::ledger_fee(config.ledger_canister_id).await else {
        retry_withdrawal_notification(notification);
        return;
    };
    if ingest_notified_withdrawal(
        &config,
        observed,
        snapshot.mint.service_fee.get(),
        snapshot.mint.max_service_fee.get(),
        ledger_fee,
        finalized_block_number,
    )
    .is_err()
    {
        retry_withdrawal_notification(notification);
        return;
    }
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.remove_withdrawal_notification(notification.transaction_hash);
        let mut progress = storage_or_trap("external progress read", store.external_progress());
        progress.last_finalized_base_block = progress
            .last_finalized_base_block
            .max(finalized_block_number);
        progress.last_finalized_observation_ns = ic_cdk::api::time();
        storage_or_trap(
            "external progress persistence",
            store.set_external_progress(&progress),
        );
    });
}

fn discard_withdrawal_notification(transaction_hash: [u8; 32]) {
    STORE.with(|store| {
        store
            .borrow_mut()
            .remove_withdrawal_notification(transaction_hash)
    });
}

fn retry_withdrawal_notification(mut notification: crate::storage::WithdrawalNotification) {
    notification.attempts = notification.attempts.saturating_add(1);
    if notification.attempts >= 12 {
        discard_withdrawal_notification(notification.transaction_hash);
        return;
    }
    let delay_seconds = 60u64.saturating_mul(1u64 << notification.attempts.min(4));
    notification.next_attempt_at_ns =
        ic_cdk::api::time().saturating_add(delay_seconds * 1_000_000_000);
    STORE.with(|store| {
        storage_or_trap(
            "withdrawal notification retry persistence",
            store
                .borrow_mut()
                .put_withdrawal_notification(&notification),
        );
    });
}

fn ingest_notified_withdrawal(
    config: &crate::config::BridgeInitArgs,
    observed: evm_rpc::ObservedWithdrawal,
    service_fee: u128,
    max_service_fee: u128,
    ledger_fee: Amount,
    finalized_block_number: u64,
) -> Result<(), ()> {
    let mut digest = Sha256::new();
    digest.update(observed.id);
    digest.update(&observed.owner);
    digest.update(observed.subaccount);
    digest.update(observed.amount.to_be_bytes());
    digest.update(observed.min_amount_out.to_be_bytes());
    let payload_hash: [u8; 32] = digest.finalize().into();
    let mut withdrawal = WithdrawalRecord::observed(
        WithdrawalId::new(observed.id),
        payload_hash,
        Amount::new(observed.amount),
        Amount::new(observed.min_amount_out),
        Amount::new(max_service_fee),
    )
    .map_err(|_| ())?;
    let amount_out = observed
        .amount
        .checked_sub(service_fee)
        .and_then(|value| value.checked_sub(ledger_fee.get()));
    if amount_out.is_none_or(|amount| amount < observed.min_amount_out) {
        return prepare_refund(
            config,
            &mut withdrawal,
            RefundEligibility {
                finalized_base_block: finalized_block_number,
                base_status_pending: true,
                release_attempt_created: false,
                reason: RefundReason::AmountBelowMinimum,
            },
        );
    }
    let amount_out = amount_out.expect("checked economic withdrawal");
    let canister = ic_cdk::api::canister_self();
    let transfer = LedgerTransferIdentity {
        operation: LedgerOperation::ReleaseWithdrawal,
        created_at_time_ns: ic_cdk::api::time(),
        memo: payload_hash,
        amount: Amount::new(amount_out),
        fee: ledger_fee,
        from: Account::new(canister.as_slice().to_vec(), [0; 32]).map_err(|_| ())?,
        to: Account::new(observed.owner, observed.subaccount).map_err(|_| ())?,
        spender: None,
    };
    withdrawal
        .apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(TransferAttempt {
                attempt_no: 0,
                identity: transfer,
            }),
            settlement: Settlement {
                amount_out: Amount::new(amount_out),
                service_fee: Amount::new(service_fee),
                ledger_fee,
            },
        })
        .map_err(|_| ())?;
    STORE.with(|store| {
        storage_or_trap(
            "notified withdrawal persistence",
            store.borrow_mut().put_withdrawal(&withdrawal),
        );
    });
    Ok(())
}

fn prepare_refund(
    config: &crate::config::BridgeInitArgs,
    withdrawal: &mut WithdrawalRecord,
    eligibility: RefundEligibility,
) -> Result<(), ()> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if storage_or_trap("withdrawal read", store.withdrawal(withdrawal.id.bytes())).is_some() {
            return Ok(());
        }
        let operation_id = storage_or_trap(
            "EVM operation ID allocation",
            store.allocate_evm_operation_id(),
        );
        if withdrawal
            .apply(WithdrawalEvent::StartRefund {
                operation_id,
                eligibility,
            })
            .is_err()
        {
            return Err(());
        }
        let operation = bridge_core::EvmOperationRecord::queued(
            operation_id,
            withdrawal.payload_hash,
            EvmOperationKind::RefundWithdrawal,
        );
        let mut selector_hash = [0u8; 32];
        let mut keccak = Keccak::v256();
        keccak.update(b"refundWithdrawal(uint256)");
        keccak.finalize(&mut selector_hash);
        let mut calldata = selector_hash[..4].to_vec();
        calldata.extend_from_slice(&withdrawal.id.bytes());
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
        store
            .put_refund_if_absent(withdrawal, &operation, &intent)
            .unwrap_or_else(|error| ic_cdk::trap(format!("refund persistence failed: {error}")));
        Ok(())
    })
}

async fn confirm_one_evm_operation() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let operation =
            storage_or_trap("submitted EVM operation read", store.first_submitted_evm())?;
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let envelope = storage_or_trap("EVM envelope read", store.evm_envelope(operation.id.get()))
            .unwrap_or_else(|| ic_cdk::trap("missing submitted EVM envelope"));
        Some((config, operation, envelope))
    });
    let Some((config, operation, envelope)) = candidate else {
        return;
    };
    let transaction_hash = match operation.state {
        EvmOperationState::Submitted { transaction_hash } => transaction_hash,
        _ => return,
    };
    let outcome = match evm_rpc::finalized_receipt_outcome(&config, transaction_hash).await {
        Ok(outcome) => outcome,
        Err(_) => return,
    };
    let (receipt_block_number, finalized_block_number) = match outcome {
        evm_rpc::FinalizedReceiptOutcome::Missing => return,
        evm_rpc::FinalizedReceiptOutcome::Succeeded {
            receipt_block_number,
            finalized_block_number,
        } => (receipt_block_number, finalized_block_number),
        evm_rpc::FinalizedReceiptOutcome::Reverted {
            receipt_block_number,
            finalized_block_number,
        } => {
            let members = STORE.with(|store| {
                let store = store.borrow();
                envelope
                    .operation_ids
                    .iter()
                    .map(|id| {
                        storage_or_trap("batched EVM operation read", store.evm_operation(id.get()))
                    })
                    .collect::<Option<Vec<_>>>()
            });
            let Some(members) = members else {
                ic_cdk::trap("missing batched EVM operation");
            };
            for member in members {
                mark_evm_reverted(
                    member,
                    transaction_hash,
                    receipt_block_number,
                    finalized_block_number,
                );
            }
            return;
        }
    };
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut finalized_mint = false;
        for operation_id in &envelope.operation_ids {
            let member = storage_or_trap(
                "batched EVM operation read",
                store.evm_operation(operation_id.get()),
            )
            .unwrap_or_else(|| ic_cdk::trap("missing batched EVM operation"));
            finalized_mint |= member.kind == EvmOperationKind::MintDeposit;
            finalize_evm_member(
                &mut store,
                member,
                transaction_hash,
                receipt_block_number,
                finalized_block_number,
            )
            .unwrap_or_else(|_| ic_cdk::trap("invalid finalized EVM transition"));
        }
        let mut progress = storage_or_trap("external progress read", store.external_progress());
        progress.last_finalized_base_block = progress
            .last_finalized_base_block
            .max(finalized_block_number);
        if finalized_mint {
            progress.last_finalized_mint_block = progress
                .last_finalized_mint_block
                .max(finalized_block_number);
        }
        progress.last_finalized_observation_ns = ic_cdk::api::time();
        storage_or_trap(
            "finalized block persistence",
            store.set_external_progress(&progress),
        );
    });
}

fn finalize_evm_member<M: ic_stable_structures::Memory>(
    store: &mut crate::storage::StableStore<M>,
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    finalized_block_number: u64,
) -> Result<(), ()> {
    operation
        .apply(EvmOperationEvent::Finalized {
            transaction_hash,
            receipt_block_number,
            finalized_block_number,
        })
        .map_err(|_| ())?;
    match operation.kind {
        EvmOperationKind::MintDeposit => {
            let mut deposit = storage_or_trap(
                "deposit by EVM operation read",
                store.deposit_for_operation(operation.id),
            )
            .ok_or(())?;
            let result = deposit
                .apply(DepositEvent::MintFinalized {
                    operation_id: operation.id,
                })
                .map_err(|_| ())?;
            let mut accounting = storage_or_trap("accounting read", store.accounting());
            accounting
                .confirm_fee(FeeKind::Deposit, result.fee_delta)
                .map_err(|_| ())?;
            storage_or_trap("accounting persistence", store.set_accounting(&accounting));
            storage_or_trap("finalized deposit persistence", store.put_deposit(&deposit));
        }
        EvmOperationKind::AcknowledgeRelease | EvmOperationKind::RefundWithdrawal => {
            let mut withdrawal = storage_or_trap(
                "withdrawal by EVM operation read",
                store.withdrawal_for_operation(operation.id),
            )
            .ok_or(())?;
            let event = match operation.kind {
                EvmOperationKind::AcknowledgeRelease => WithdrawalEvent::AcknowledgementFinalized {
                    operation_id: operation.id,
                },
                EvmOperationKind::RefundWithdrawal => WithdrawalEvent::RefundFinalized {
                    operation_id: operation.id,
                },
                EvmOperationKind::MintDeposit => unreachable!(),
            };
            withdrawal.apply(event).map_err(|_| ())?;
            storage_or_trap(
                "finalized withdrawal persistence",
                store.put_withdrawal(&withdrawal),
            );
        }
    }
    storage_or_trap(
        "finalized EVM operation persistence",
        store.put_evm_operation(&operation),
    );
    Ok(())
}

fn mark_evm_reverted(
    mut operation: bridge_core::EvmOperationRecord,
    transaction_hash: [u8; 32],
    receipt_block_number: u64,
    finalized_block_number: u64,
) {
    operation
        .apply(EvmOperationEvent::Reverted {
            transaction_hash,
            receipt_block_number,
            finalized_block_number,
        })
        .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert transition failed: {error}")));
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        match operation.kind {
            EvmOperationKind::MintDeposit => {
                let mut deposit = store
                    .deposit_for_operation(operation.id)
                    .unwrap_or_else(|error| ic_cdk::trap(format!("deposit read failed: {error}")))
                    .unwrap_or_else(|| ic_cdk::trap("missing deposit for reverted operation"));
                deposit
                    .apply(DepositEvent::MintReverted {
                        operation_id: operation.id,
                    })
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("deposit revert transition failed: {error}"))
                    });
                store.put_deposit(&deposit).unwrap_or_else(|error| {
                    ic_cdk::trap(format!("deposit revert persistence failed: {error}"))
                });
            }
            EvmOperationKind::AcknowledgeRelease | EvmOperationKind::RefundWithdrawal => {
                let mut withdrawal = store
                    .withdrawal_for_operation(operation.id)
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("withdrawal read failed: {error}"))
                    })
                    .unwrap_or_else(|| ic_cdk::trap("missing withdrawal for reverted operation"));
                let event = match operation.kind {
                    EvmOperationKind::AcknowledgeRelease => {
                        WithdrawalEvent::AcknowledgementReverted {
                            operation_id: operation.id,
                        }
                    }
                    EvmOperationKind::RefundWithdrawal => WithdrawalEvent::RefundReverted {
                        operation_id: operation.id,
                    },
                    EvmOperationKind::MintDeposit => unreachable!(),
                };
                withdrawal.apply(event).unwrap_or_else(|error| {
                    ic_cdk::trap(format!("withdrawal revert transition failed: {error}"))
                });
                store.put_withdrawal(&withdrawal).unwrap_or_else(|error| {
                    ic_cdk::trap(format!("withdrawal revert persistence failed: {error}"))
                });
            }
        }
        store.put_evm_operation(&operation).unwrap_or_else(|error| {
            ic_cdk::trap(format!("EVM revert persistence failed: {error}"))
        });
        let mut admin = store
            .admin_state()
            .unwrap_or_else(|error| ic_cdk::trap(format!("administrator read failed: {error}")));
        admin.deposits_paused = true;
        store
            .set_admin_state(&admin)
            .unwrap_or_else(|error| ic_cdk::trap(format!("automatic pause failed: {error}")));
        store
            .append_audit_event(
                ic_cdk::api::canister_self(),
                crate::storage::AuditEventKind::EvmOperationReverted {
                    operation_id: operation.id.get(),
                    kind: operation.kind.into(),
                    transaction_hash: transaction_hash.to_vec(),
                    finalized_block_number,
                },
            )
            .unwrap_or_else(|error| ic_cdk::trap(format!("EVM revert audit failed: {error}")));
        let mut progress = store.external_progress().unwrap_or_else(|error| {
            ic_cdk::trap(format!("external progress read failed: {error}"))
        });
        progress.last_finalized_base_block = progress
            .last_finalized_base_block
            .max(finalized_block_number);
        progress.last_finalized_observation_ns = ic_cdk::api::time();
        store
            .set_external_progress(&progress)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("external progress persistence failed: {error}"))
            });
    });
}

async fn process_one_evm_operation() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let operation = storage_or_trap("prepared EVM operation read", store.first_prepared_evm())?;
        Some((config, operation))
    });
    let Some((config, (operation, mut envelope))) = candidate else {
        return;
    };
    let raw = match envelope.signed_transaction.clone() {
        Some(raw) => raw,
        None => match signer::sign(&envelope, &config).await {
            Ok(raw) => {
                envelope.signed_transaction = Some(raw.clone());
                STORE.with(|store| {
                    storage_or_trap(
                        "signed EVM envelope persistence",
                        store.borrow_mut().put_evm_envelope(&envelope),
                    );
                });
                raw
            }
            Err(error) => {
                ic_cdk::println!(
                    "failed to sign EVM operation {}: {error}",
                    operation.id.get()
                );
                return;
            }
        },
    };
    if evm_rpc::broadcast(&config, &raw).await.is_err() {
        return;
    }
    let transaction_hash = signer::transaction_hash(&raw);
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        for operation_id in &envelope.operation_ids {
            let mut member = store
                .evm_operation(operation_id.get())
                .unwrap_or_else(|error| ic_cdk::trap(format!("EVM operation read failed: {error}")))
                .unwrap_or_else(|| ic_cdk::trap("missing batched EVM operation"));
            member
                .apply(EvmOperationEvent::Submitted { transaction_hash })
                .unwrap_or_else(|error| {
                    ic_cdk::trap(format!("EVM submit transition failed: {error}"))
                });
            store.put_evm_operation(&member).unwrap_or_else(|error| {
                ic_cdk::trap(format!(
                    "submitted EVM operation persistence failed: {error}"
                ))
            });
        }
    });
}

async fn process_one_release() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let withdrawal = storage_or_trap(
            "release-pending withdrawal read",
            store.first_release_pending(),
        )?;
        let transfer = match &withdrawal.state {
            WithdrawalState::ReleasePending { attempt, .. } => attempt.identity.clone(),
            _ => return None,
        };
        Some((config, withdrawal.id, transfer))
    });
    let Some((config, withdrawal_id, transfer)) = candidate else {
        return;
    };
    let outcome = ledger::release(config.ledger_canister_id, &transfer).await;
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut withdrawal =
            storage_or_trap("withdrawal read", store.withdrawal(withdrawal_id.bytes()))
                .unwrap_or_else(|| ic_cdk::trap("missing release-pending withdrawal"));
        match outcome {
            LedgerCallOutcome::Succeeded { block_index }
            | LedgerCallOutcome::Duplicate { block_index } => {
                let result = withdrawal
                    .apply(WithdrawalEvent::ReleaseSucceeded {
                        ledger_block_index: block_index,
                    })
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("withdrawal release transition failed: {error}"))
                    });
                let mut accounting = storage_or_trap("accounting read", store.accounting());
                accounting
                    .confirm_fee(FeeKind::Withdrawal, result.fee_delta)
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("withdrawal fee confirmation failed: {error}"))
                    });
                storage_or_trap("accounting persistence", store.set_accounting(&accounting));
                prepare_acknowledgement_in_store(&mut store, &config, &mut withdrawal)
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("acknowledgement preparation failed: {error}"))
                    });
            }
            LedgerCallOutcome::Ambiguous => {
                let hold_id = storage_or_trap("hold ID allocation", store.allocate_hold_id());
                withdrawal
                    .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                    .unwrap_or_else(|error| {
                        ic_cdk::trap(format!("withdrawal hold transition failed: {error}"))
                    });
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Withdrawal(withdrawal.id),
                    transfer,
                );
                storage_or_trap(
                    "held withdrawal persistence",
                    store.put_withdrawal(&withdrawal),
                );
                storage_or_trap(
                    "withdrawal hold persistence",
                    store.put_open_reconciliation_hold(&hold),
                );
            }
            LedgerCallOutcome::DefinitiveFailure { .. }
            | LedgerCallOutcome::RetryableFailure { .. } => {}
        }
    });
}

pub(crate) fn prepare_acknowledgement_in_store<M: ic_stable_structures::Memory>(
    store: &mut crate::storage::StableStore<M>,
    config: &crate::config::BridgeInitArgs,
    withdrawal: &mut WithdrawalRecord,
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
    let operation_id = store.allocate_evm_operation_id()?;
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
    store.put_evm_call_intent(&intent)?;
    store.put_evm_operation(&operation)?;
    store.put_withdrawal(withdrawal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{EvmCallIntent, EvmOperationId, EvmOperationRecord};

    #[test]
    fn refund_batch_envelope_uses_one_nonce_and_bounds_members() {
        let batch = (1..=4)
            .map(|id| {
                let operation_id = EvmOperationId::new(id);
                let operation = EvmOperationRecord::queued(
                    operation_id,
                    [id as u8; 32],
                    EvmOperationKind::RefundWithdrawal,
                );
                let mut calldata = vec![0; 4];
                calldata.extend_from_slice(&abi_word(id as u128));
                let intent = EvmCallIntent {
                    operation_id,
                    payload_hash: operation.payload_hash,
                    chain_id: 8453,
                    contract: [7; 20],
                    calldata,
                    gas_limit: 100_000,
                    max_fee_per_gas: 10,
                    max_priority_fee_per_gas: 1,
                };
                (operation, intent)
            })
            .collect::<Vec<_>>();
        let envelope = batch_envelope(&batch, 9).expect("batch envelope");
        assert_eq!(envelope.operation_ids.len(), 4);
        assert_eq!(envelope.operation_id, EvmOperationId::new(1));
        assert_eq!(envelope.nonce, 9);
        assert_eq!(envelope.gas_limit, 400_000);
        assert_eq!(envelope.calldata.len(), 4 + 32 + 32 + 4 * 32);
        assert_eq!(
            &envelope.calldata[..4],
            &selector_for_test("refundWithdrawals(uint256[])")
        );
    }

    fn selector_for_test(signature: &str) -> [u8; 4] {
        let mut hash = [0; 32];
        let mut keccak = Keccak::v256();
        keccak.update(signature.as_bytes());
        keccak.finalize(&mut hash);
        hash[..4].try_into().expect("selector")
    }
}
