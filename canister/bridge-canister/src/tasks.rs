use crate::{evm_rpc, ledger, signer, STORE};
use bridge_core::{
    Account, Amount, DepositEvent, DepositHoldResolution, EvmOperationEvent, EvmOperationKind,
    EvmOperationState, EvmSafeObservation, FeeKind, LedgerCallOutcome, LedgerOperation,
    LedgerTransferIdentity, ReconciliationHoldRecord, RefundEligibility, RefundReason,
    RequestReference, SafeReceiptOutcome, Settlement, TransferAttempt, WithdrawalEvent,
    WithdrawalHoldResolution, WithdrawalId, WithdrawalRecord, WithdrawalState,
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
    observe_service_fee().await;
    observe_one_safe_evm_operation().await;
    confirm_one_evm_operation().await;
    rebroadcast_one_unconfirmed_evm_operation().await;
    reconcile_one_hold().await;
    reconcile_fee_payout().await;
    discover_withdrawals().await;
    process_one_release().await;
    if ensure_nonce_initialized_from_store().await.is_err() {
        return;
    }
    assign_one_evm_nonce();
    process_one_evm_operation().await;
    process_one_deposit_pull().await;
}

async fn rebroadcast_one_unconfirmed_evm_operation() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        Some((
            store.config().ok().flatten()?,
            store
                .first_submitted_without_safe_observation()
                .ok()
                .flatten()?,
        ))
    });
    let Some((config, (operation, envelope))) = candidate else {
        return;
    };
    let Some(raw) = envelope.signed_transaction else {
        return;
    };
    let transaction_hash = match operation.state {
        EvmOperationState::Submitted { transaction_hash } => transaction_hash,
        _ => return,
    };
    if signer::transaction_hash(&raw) != transaction_hash {
        return;
    }
    let _ = evm_rpc::broadcast(&config, &raw).await;
}

async fn observe_one_safe_evm_operation() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let progress = store.external_progress().ok()?;
        Some((
            store.config().ok().flatten()?,
            store
                .first_submitted_for_safe_observation(progress.safe_observation_cursor)
                .ok()
                .flatten()?,
        ))
    });
    let Some((config, operation)) = candidate else {
        return;
    };
    let transaction_hash = match operation.state {
        EvmOperationState::Submitted { transaction_hash } => transaction_hash,
        _ => return,
    };
    let outcome = match evm_rpc::safe_receipt_outcome(&config, transaction_hash).await {
        Ok(outcome) => outcome,
        Err(_) => return,
    };
    let safe_block_number = match outcome {
        evm_rpc::SafeReceiptOutcome::Missing { safe_block_number }
        | evm_rpc::SafeReceiptOutcome::Succeeded {
            safe_block_number, ..
        }
        | evm_rpc::SafeReceiptOutcome::Reverted {
            safe_block_number, ..
        } => safe_block_number,
    };
    let observation = match outcome {
        evm_rpc::SafeReceiptOutcome::Missing { .. } => None,
        evm_rpc::SafeReceiptOutcome::Succeeded {
            receipt_block_number,
            safe_block_number,
        } => {
            let contract_matches = if operation.kind == EvmOperationKind::MintDeposit {
                let deposit_id = STORE.with(|store| {
                    store
                        .borrow()
                        .deposit_for_operation(operation.id)
                        .ok()
                        .flatten()
                        .map(|record| record.id)
                });
                let Some(deposit_id) = deposit_id else {
                    return;
                };
                let Ok(matches) = evm_rpc::is_deposit_processed_at_block(
                    &config,
                    deposit_id.bytes(),
                    safe_block_number,
                )
                .await
                else {
                    return;
                };
                matches
            } else {
                let withdrawal_id = STORE.with(|store| {
                    store
                        .borrow()
                        .withdrawal_for_operation(operation.id)
                        .ok()
                        .flatten()
                        .map(|record| record.id)
                });
                let Some(withdrawal_id) = withdrawal_id else {
                    return;
                };
                let expected = match operation.kind {
                    EvmOperationKind::AcknowledgeRelease => 2,
                    EvmOperationKind::RefundWithdrawal => 3,
                    EvmOperationKind::MintDeposit => unreachable!(),
                };
                let Ok(status) = evm_rpc::withdrawal_status_at_block(
                    &config,
                    withdrawal_id.bytes(),
                    safe_block_number,
                )
                .await
                else {
                    return;
                };
                status == expected
            };
            contract_matches.then_some(EvmSafeObservation {
                operation_id: operation.id,
                transaction_hash,
                receipt_block_number,
                safe_block_number,
                observed_at_ns: ic_cdk::api::time(),
                outcome: SafeReceiptOutcome::Succeeded,
            })
        }
        evm_rpc::SafeReceiptOutcome::Reverted {
            receipt_block_number,
            safe_block_number,
        } => Some(EvmSafeObservation {
            operation_id: operation.id,
            transaction_hash,
            receipt_block_number,
            safe_block_number,
            observed_at_ns: ic_cdk::api::time(),
            outcome: SafeReceiptOutcome::Reverted,
        }),
    };
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let Ok(Some(current)) = store.evm_operation(operation.id.get()) else {
            return;
        };
        if current.state != operation.state {
            return;
        }
        match observation {
            Some(value) => {
                if store.put_evm_safe_observation(&value).is_err() {
                    return;
                }
            }
            None => store.remove_evm_safe_observation(operation.id.get()),
        }
        let Ok(mut progress) = store.external_progress() else {
            return;
        };
        progress.last_safe_base_block = safe_block_number;
        progress.last_safe_observation_ns = ic_cdk::api::time();
        progress.safe_observation_cursor = operation.id.get();
        let _ = store.set_external_progress(&progress);
    });
}

async fn ensure_nonce_initialized_from_store() -> Result<(), ()> {
    let config = STORE.with(|store| store.borrow().config().ok().flatten());
    let Some(config) = config else {
        return Err(());
    };
    ensure_nonce_initialized(&config).await.map_err(|_| ())
}

pub(crate) enum NonceInitializationError {
    Observation,
    Storage,
}

pub(crate) async fn ensure_nonce_initialized(
    config: &crate::config::BridgeInitArgs,
) -> Result<(), NonceInitializationError> {
    let initialized = STORE.with(|store| {
        store
            .borrow()
            .external_progress()
            .map(|progress| progress.nonce_initialized)
            .unwrap_or(false)
    });
    if initialized {
        return Ok(());
    }
    let address = signer::ethereum_address(config).await.map_err(|error| {
        ic_cdk::println!("failed to derive bridge signer address: {error:?}");
        NonceInitializationError::Observation
    })?;
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
        Some((
            store.config().ok().flatten()?,
            store.first_held_fee_payout().ok().flatten()?,
        ))
    });
    let Some((config, mut payout)) = candidate else {
        return;
    };
    if ic_cdk::api::time().saturating_sub(payout.transfer.created_at_time_ns) <= DEDUP_NS {
        return;
    }
    let resolution = ledger::reconcile_history(config.ledger_canister_id, &payout.transfer).await;
    match resolution {
        ledger::HistoryResolution::Succeeded { block_index } => {
            payout.state = crate::admin::FeePayoutState::Succeeded { block_index };
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let Ok(mut accounting) = store.accounting() else {
                    return;
                };
                let Some(debit) =
                    bridge_core::payout_debit(true, payout.amount, payout.transfer.fee.get())
                else {
                    return;
                };
                if accounting.spend_fee_reserve(Amount::new(debit)).is_ok()
                    && store.set_accounting(&accounting).is_ok()
                {
                    let _ = store.put_fee_payout(&payout);
                }
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
                    let _ = store.borrow_mut().put_fee_payout(&payout);
                });
            }
        }
        ledger::HistoryResolution::Incomplete => {}
    }
}

fn assign_one_evm_nonce() {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let has_prepared = match store.first_prepared_evm() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => return,
        };
        let Ok(mut progress) = store.external_progress() else {
            return;
        };
        if !bridge_core::can_assign_nonce(progress.nonce_initialized, has_prepared) {
            return;
        }
        let Ok(Some((mut operation, intent))) = store.first_queued_evm() else {
            return;
        };
        let nonce = progress.next_evm_nonce;
        let Some(next) = bridge_core::nonce_next(nonce) else {
            return;
        };
        let envelope = intent.assign_nonce(nonce);
        if operation.apply(EvmOperationEvent::Prepared).is_err() {
            return;
        }
        if store.put_evm_envelope(&envelope).is_err()
            || store.put_evm_operation(&operation).is_err()
        {
            return;
        }
        progress.next_evm_nonce = next;
        store
            .set_external_progress(&progress)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("nonce assignment persistence failed: {error}"))
            });
    });
}

async fn observe_service_fee() {
    let config = STORE.with(|store| store.borrow().config().ok().flatten());
    let Some(config) = config else {
        return;
    };
    let Ok((current, _)) = evm_rpc::service_fee(&config).await else {
        return;
    };
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let Ok(mut progress) = store.external_progress() else {
            return;
        };
        if progress.last_observed_service_fee == Some(current) {
            return;
        }
        let previous = progress.last_observed_service_fee;
        progress.last_observed_service_fee = Some(current);
        if store.set_external_progress(&progress).is_ok() {
            let _ = store.append_audit_event(
                ic_cdk::api::canister_self(),
                crate::storage::AuditEventKind::BaseServiceFeeChanged { previous, current },
            );
        }
    });
}

async fn process_one_deposit_pull() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = store.config().ok().flatten()?;
        let deposit = store.first_pull_pending().ok().flatten()?;
        let intent = store.deposit_intent(deposit.id.bytes()).ok().flatten()?;
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
                let Ok(Some(current)) = store.deposit(deposit.id.bytes()) else {
                    return;
                };
                if !matches!(current.state, bridge_core::DepositState::PullPending) {
                    return;
                }
                let Ok(hold_id) = store.allocate_hold_id() else {
                    return;
                };
                if deposit
                    .apply(DepositEvent::PullAmbiguous { hold_id })
                    .is_err()
                {
                    return;
                }
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Deposit(deposit.id),
                    deposit.transfer.clone(),
                );
                if store.put_deposit(&deposit).is_ok() {
                    store
                        .put_open_reconciliation_hold(&hold)
                        .unwrap_or_else(|error| {
                            ic_cdk::trap(format!("deposit hold persistence failed: {error}"))
                        });
                }
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
        Some((
            store.config().ok().flatten()?,
            store.first_open_hold().ok().flatten()?,
        ))
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
                            if !scan.can_prove_absent()
                                || store.put_reconciliation_scan(&scan).is_err()
                            {
                                return;
                            }
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
                            if !scan.can_prove_absent()
                                || store.put_reconciliation_scan(&scan).is_err()
                            {
                                return;
                            }
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

async fn discover_withdrawals() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        Some((
            store.config().ok().flatten()?,
            store.external_progress().ok()?,
        ))
    });
    let Some((config, mut progress)) = candidate else {
        return;
    };
    let Ok((next_cursor, ids)) =
        evm_rpc::discover_withdrawals(&config, progress.withdrawal_log_cursor).await
    else {
        return;
    };
    progress.last_finalized_base_block = progress
        .last_finalized_base_block
        .max(next_cursor.saturating_sub(1));
    progress.last_finalized_observation_ns = ic_cdk::api::time();
    for id in ids {
        let already_known =
            STORE.with(|store| store.borrow().withdrawal(id).ok().flatten().is_some());
        if already_known {
            continue;
        }
        let Ok(Some(observed)) = evm_rpc::finalized_withdrawal(&config, id).await else {
            return;
        };
        let Ok((service_fee, max_service_fee)) = evm_rpc::service_fee(&config).await else {
            return;
        };
        let Ok(ledger_fee) = ledger::ledger_fee(config.ledger_canister_id).await else {
            return;
        };
        let mut digest = Sha256::new();
        digest.update(id);
        digest.update(&observed.owner);
        digest.update(observed.subaccount);
        digest.update(observed.amount.to_be_bytes());
        digest.update(observed.min_amount_out.to_be_bytes());
        let payload_hash: [u8; 32] = digest.finalize().into();
        let Ok(mut withdrawal) = WithdrawalRecord::observed(
            WithdrawalId::new(id),
            payload_hash,
            Amount::new(observed.amount),
            Amount::new(observed.min_amount_out),
            Amount::new(max_service_fee),
        ) else {
            return;
        };
        let Some(amount_out) = observed
            .amount
            .checked_sub(service_fee)
            .and_then(|value| value.checked_sub(ledger_fee.get()))
        else {
            // An uneconomic withdrawal remains Observed until the refund transaction path is
            // prepared; no ICP transfer is attempted.
            if prepare_refund(
                &config,
                &mut withdrawal,
                RefundEligibility {
                    finalized_base_block: next_cursor.saturating_sub(1),
                    base_status_pending: true,
                    release_attempt_created: false,
                    reason: RefundReason::AmountBelowMinimum,
                },
            )
            .is_err()
            {
                return;
            }
            continue;
        };
        if amount_out < observed.min_amount_out {
            if prepare_refund(
                &config,
                &mut withdrawal,
                RefundEligibility {
                    finalized_base_block: next_cursor.saturating_sub(1),
                    base_status_pending: true,
                    release_attempt_created: false,
                    reason: RefundReason::AmountBelowMinimum,
                },
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        let canister = ic_cdk::api::canister_self();
        let transfer = LedgerTransferIdentity {
            operation: LedgerOperation::ReleaseWithdrawal,
            created_at_time_ns: ic_cdk::api::time(),
            memo: payload_hash,
            amount: Amount::new(amount_out),
            fee: ledger_fee,
            from: match Account::new(canister.as_slice().to_vec(), [0; 32]) {
                Ok(account) => account,
                Err(_) => return,
            },
            to: match Account::new(observed.owner, observed.subaccount) {
                Ok(account) => account,
                Err(_) => return,
            },
            spender: None,
        };
        if withdrawal
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
            .is_err()
        {
            return;
        }
        if STORE
            .with(|store| store.borrow_mut().put_withdrawal(&withdrawal))
            .is_err()
        {
            return;
        }
    }
    progress.withdrawal_log_cursor = next_cursor;
    STORE.with(|store| {
        store
            .borrow_mut()
            .set_external_progress(&progress)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("withdrawal cursor persistence failed: {error}"))
            })
    });
}

fn prepare_refund(
    config: &crate::config::BridgeInitArgs,
    withdrawal: &mut WithdrawalRecord,
    eligibility: RefundEligibility,
) -> Result<(), ()> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if store
            .withdrawal(withdrawal.id.bytes())
            .map_err(|_| ())?
            .is_some()
        {
            return Ok(());
        }
        let operation_id = store.allocate_evm_operation_id().map_err(|_| ())?;
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
        Some((
            store.config().ok().flatten()?,
            store.first_submitted_evm().ok().flatten()?,
        ))
    });
    let Some((config, mut operation)) = candidate else {
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
            mark_evm_reverted(
                operation,
                transaction_hash,
                receipt_block_number,
                finalized_block_number,
            );
            return;
        }
    };
    if operation.kind == EvmOperationKind::MintDeposit {
        let deposit_id = STORE.with(|store| {
            store
                .borrow()
                .deposit_for_operation(operation.id)
                .ok()
                .flatten()
                .map(|record| record.id)
        });
        let Some(deposit_id) = deposit_id else {
            return;
        };
        if evm_rpc::is_deposit_processed(&config, deposit_id.bytes()).await != Ok(true) {
            return;
        }
    } else {
        let withdrawal_id = STORE.with(|store| {
            store
                .borrow()
                .withdrawal_for_operation(operation.id)
                .ok()
                .flatten()
                .map(|record| record.id)
        });
        let Some(withdrawal_id) = withdrawal_id else {
            return;
        };
        let expected = match operation.kind {
            EvmOperationKind::AcknowledgeRelease => 2,
            EvmOperationKind::RefundWithdrawal => 3,
            EvmOperationKind::MintDeposit => unreachable!(),
        };
        if evm_rpc::finalized_withdrawal_status(&config, withdrawal_id.bytes()).await
            != Ok(expected)
        {
            return;
        }
    }
    if operation
        .apply(EvmOperationEvent::Finalized {
            transaction_hash,
            finalized_block_number: receipt_block_number,
        })
        .is_err()
    {
        return;
    }
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if operation.kind == EvmOperationKind::MintDeposit {
            let Ok(Some(mut deposit)) = store.deposit_for_operation(operation.id) else {
                return;
            };
            let Ok(result) = deposit.apply(DepositEvent::MintFinalized {
                operation_id: operation.id,
            }) else {
                return;
            };
            let Ok(mut accounting) = store.accounting() else {
                return;
            };
            if accounting
                .confirm_fee(FeeKind::Deposit, result.fee_delta)
                .is_err()
            {
                return;
            }
            if store.set_accounting(&accounting).is_err() || store.put_deposit(&deposit).is_err() {
                return;
            }
        } else {
            let Ok(Some(mut withdrawal)) = store.withdrawal_for_operation(operation.id) else {
                return;
            };
            let event = match operation.kind {
                EvmOperationKind::AcknowledgeRelease => WithdrawalEvent::AcknowledgementFinalized {
                    operation_id: operation.id,
                },
                EvmOperationKind::RefundWithdrawal => WithdrawalEvent::RefundFinalized {
                    operation_id: operation.id,
                },
                EvmOperationKind::MintDeposit => unreachable!(),
            };
            if withdrawal.apply(event).is_err() || store.put_withdrawal(&withdrawal).is_err() {
                return;
            }
        }
        if store.put_evm_operation(&operation).is_err() {
            return;
        }
        store.remove_evm_safe_observation(operation.id.get());
        if let Ok(mut progress) = store.external_progress() {
            progress.last_finalized_base_block = progress
                .last_finalized_base_block
                .max(finalized_block_number);
            if operation.kind == EvmOperationKind::MintDeposit {
                progress.last_finalized_mint_block = progress
                    .last_finalized_mint_block
                    .max(finalized_block_number);
            }
            progress.last_finalized_observation_ns = ic_cdk::api::time();
            store
                .set_external_progress(&progress)
                .unwrap_or_else(|error| {
                    ic_cdk::trap(format!("finalized block persistence failed: {error}"))
                });
        }
    });
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
            finalized_block_number: receipt_block_number,
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
        store.remove_evm_safe_observation(operation.id.get());
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
                    finalized_block_number: receipt_block_number,
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
        Some((
            store.config().ok().flatten()?,
            store.first_prepared_evm().ok().flatten()?,
        ))
    });
    let Some((config, (mut operation, mut envelope))) = candidate else {
        return;
    };
    let raw = match envelope.signed_transaction.clone() {
        Some(raw) => raw,
        None => match signer::sign(&envelope, &config).await {
            Ok(raw) => {
                envelope.signed_transaction = Some(raw.clone());
                let persisted = STORE.with(|store| store.borrow_mut().put_evm_envelope(&envelope));
                if persisted.is_err() {
                    return;
                }
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
    if operation
        .apply(EvmOperationEvent::Submitted { transaction_hash })
        .is_err()
    {
        return;
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .put_evm_operation(&operation)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!(
                    "submitted EVM operation persistence failed: {error}"
                ))
            });
    });
}

async fn process_one_release() {
    let candidate = STORE.with(|store| {
        let store = store.borrow();
        let config = store.config().ok().flatten()?;
        let withdrawal = store.first_release_pending().ok().flatten()?;
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
        let Ok(Some(mut withdrawal)) = store.withdrawal(withdrawal_id.bytes()) else {
            return;
        };
        match outcome {
            LedgerCallOutcome::Succeeded { block_index }
            | LedgerCallOutcome::Duplicate { block_index } => {
                if let Ok(result) = withdrawal.apply(WithdrawalEvent::ReleaseSucceeded {
                    ledger_block_index: block_index,
                }) {
                    let Ok(mut accounting) = store.accounting() else {
                        return;
                    };
                    if accounting
                        .confirm_fee(FeeKind::Withdrawal, result.fee_delta)
                        .is_err()
                        || store.set_accounting(&accounting).is_err()
                    {
                        return;
                    }
                    prepare_acknowledgement_in_store(&mut store, &config, &mut withdrawal)
                        .unwrap_or_else(|error| {
                            ic_cdk::trap(format!("acknowledgement preparation failed: {error}"))
                        });
                }
            }
            LedgerCallOutcome::Ambiguous => {
                let Ok(hold_id) = store.allocate_hold_id() else {
                    return;
                };
                if withdrawal
                    .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                    .is_err()
                {
                    return;
                }
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Withdrawal(withdrawal.id),
                    transfer,
                );
                if store.put_withdrawal(&withdrawal).is_ok() {
                    store
                        .put_open_reconciliation_hold(&hold)
                        .unwrap_or_else(|error| {
                            ic_cdk::trap(format!("withdrawal hold persistence failed: {error}"))
                        });
                }
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
