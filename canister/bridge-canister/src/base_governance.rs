use crate::{admin, evm_rpc, signer, storage, STORE};
use bridge_core::{EvmOperationId, EvmTransactionEnvelope};
use candid::{CandidType, Deserialize, Nat, Principal};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseGovernanceAction {
    PauseDepositMints,
    PauseWithdrawals,
    SetServiceFee { value: Nat },
    CancelPendingTimelock,
    ScheduleActivation,
    ExecuteActivation,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseGovernanceError {
    Unauthorized,
    InvalidArgument,
    Busy { operation_id: u64 },
    StorageFailure,
    ObservationUnavailable,
    SigningUnavailable,
    BroadcastAmbiguous { operation_id: u64 },
    NonceConflict { operation_id: u64 },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BaseGovernanceReceipt {
    pub operation_id: u64,
    pub nonce: u64,
    pub transaction_hash: Option<Vec<u8>>,
}

pub async fn submit(
    caller: Principal,
    action: BaseGovernanceAction,
) -> Result<BaseGovernanceReceipt, BaseGovernanceError> {
    let governance =
        admin::is_governance(caller).map_err(|_| BaseGovernanceError::StorageFailure)?;
    let pause = is_pause_principal(caller)?;
    if !authorized_action(governance, pause, &action) {
        return Err(BaseGovernanceError::Unauthorized);
    }
    let activation = matches!(
        action,
        BaseGovernanceAction::ScheduleActivation | BaseGovernanceAction::ExecuteActivation
    );
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::StorageFailure)
    })?;
    let operator = crate::api::cached_governance_operator_address(&config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    require_current_authorization(caller, &action)?;
    let (initialized, _, _, pending) = STORE.with(|store| {
        store
            .borrow()
            .governance_lane()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    if let Some(pending) = pending {
        if !action_matches_pending(&action, &pending.kind) {
            return Err(BaseGovernanceError::Busy {
                operation_id: pending.id,
            });
        }
        return continue_pending(caller, &config, pending).await;
    }
    if !initialized {
        let nonce = evm_rpc::transaction_count(&config, operator)
            .await
            .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
        require_current_authorization(caller, &action)?;
        STORE.with(|store| {
            store
                .borrow_mut()
                .initialize_governance_nonce(nonce)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
    }
    let (_, nonce, id, pending) = STORE.with(|store| {
        store
            .borrow()
            .governance_lane()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    if let Some(pending) = pending {
        if !action_matches_pending(&action, &pending.kind) {
            return Err(BaseGovernanceError::Busy {
                operation_id: pending.id,
            });
        }
        return continue_pending(caller, &config, pending).await;
    }
    if activation {
        activation_preflight(&config).await?;
        require_current_authorization(caller, &action)?;
    }
    let (kind, target, calldata) = encode_action(action, id)?;
    let payload_hash: [u8; 32] = Sha256::digest(&calldata).into();
    let envelope = EvmTransactionEnvelope {
        operation_id: EvmOperationId::new(id),
        payload_hash,
        nonce,
        chain_id: config.base_chain_id,
        contract: target,
        calldata,
        gas_limit: config.transaction_gas_limit,
        max_fee_per_gas: config.max_fee_per_gas,
        max_priority_fee_per_gas: config.max_priority_fee_per_gas,
        signed_transaction: None,
        initial_max_fee_per_gas: config.max_fee_per_gas,
        initial_max_priority_fee_per_gas: config.max_priority_fee_per_gas,
        replacement_generation: 0,
        prior_signed_transactions: Vec::new(),
        first_broadcast_at_ns: 0,
        last_broadcast_at_ns: 0,
        rebroadcast_count: 0,
    };
    let transaction = storage::GovernanceTransaction {
        id,
        kind,
        envelope,
        state: storage::GovernanceTransactionState::Prepared,
    };
    STORE.with(|store| {
        store
            .borrow_mut()
            .prepare_governance_transaction(transaction.clone())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    continue_pending(caller, &config, transaction).await
}

async fn activation_preflight(
    config: &crate::config::BridgeInitArgs,
) -> Result<(), BaseGovernanceError> {
    let expected_signer = crate::api::cached_signer_address(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let observed = evm_rpc::bridge_snapshot(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let (locally_paused, nonterminal_withdrawals, reserved_mint_operations) =
        STORE.with(|store| {
            let store = store.borrow();
            let paused = store
                .admin_state()
                .map(|state| state.deposits_paused)
                .unwrap_or(false);
            let counters = store
                .counters()
                .map_err(|_| BaseGovernanceError::StorageFailure)?;
            Ok::<_, BaseGovernanceError>((
                paused,
                counters.nonterminal_withdrawals,
                counters.reserved_deposit_mint_operations,
            ))
        })?;
    let finalized_eth = evm_rpc::signer_eth_balance_at(config, expected_signer, observed.finalized)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let safe_eth = evm_rpc::signer_eth_balance(config, expected_signer)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let reserve_sufficient = config
        .reserve_policy()
        .snapshot(
            nonterminal_withdrawals,
            reserved_mint_operations,
            0,
            finalized_eth.min(safe_eth),
            ic_cdk::api::canister_liquid_cycle_balance(),
        )
        .map(|snapshot| snapshot.sufficient)
        .unwrap_or(false);
    if !locally_paused
        || !reserve_sufficient
        || observed.snapshot.bridge_signer != expected_signer
        || !observed.snapshot.deposits_paused
        || !observed.snapshot.withdrawals_paused
    {
        return Err(BaseGovernanceError::ObservationUnavailable);
    }
    Ok(())
}

pub async fn emergency_pause(
    caller: Principal,
) -> Result<BaseGovernanceReceipt, BaseGovernanceError> {
    crate::admin::pause(caller).map_err(|error| match error {
        crate::admin::AdminError::Unauthorized => BaseGovernanceError::Unauthorized,
        _ => BaseGovernanceError::StorageFailure,
    })?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .enqueue_emergency_base_actions()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    let result = process_emergency(caller).await;
    crate::scheduler::arm_base_governance(caller);
    result
}

pub(crate) async fn process_emergency(
    caller: Principal,
) -> Result<BaseGovernanceReceipt, BaseGovernanceError> {
    let pending = loop {
        let pending = STORE.with(|store| {
            store
                .borrow()
                .governance_lane()
                .map(|(_, _, _, pending)| pending)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
        let Some(transaction) = pending.as_ref() else {
            break None;
        };
        if abort_if_emergency_unbroadcast(transaction)? {
            continue;
        }
        break pending;
    };
    let action = if let Some(transaction) = pending {
        match transaction.kind {
            storage::GovernanceTransactionKind::PauseDepositMints => {
                BaseGovernanceAction::PauseDepositMints
            }
            storage::GovernanceTransactionKind::PauseWithdrawals => {
                BaseGovernanceAction::PauseWithdrawals
            }
            storage::GovernanceTransactionKind::CancelTimelock { .. } => {
                BaseGovernanceAction::CancelPendingTimelock
            }
            storage::GovernanceTransactionKind::SetServiceFee { value } => {
                BaseGovernanceAction::SetServiceFee {
                    value: value.into(),
                }
            }
            storage::GovernanceTransactionKind::ScheduleActivation { .. } => {
                BaseGovernanceAction::ScheduleActivation
            }
            storage::GovernanceTransactionKind::ExecuteActivation { .. } => {
                BaseGovernanceAction::ExecuteActivation
            }
        }
    } else {
        match STORE
            .with(|store| store.borrow().next_emergency_base_action())
            .map_err(|_| BaseGovernanceError::StorageFailure)?
        {
            Some(storage::GovernanceTransactionKind::PauseDepositMints) => {
                BaseGovernanceAction::PauseDepositMints
            }
            Some(storage::GovernanceTransactionKind::PauseWithdrawals) => {
                BaseGovernanceAction::PauseWithdrawals
            }
            Some(storage::GovernanceTransactionKind::CancelTimelock { .. }) => {
                BaseGovernanceAction::CancelPendingTimelock
            }
            Some(storage::GovernanceTransactionKind::SetServiceFee { .. }) | None => {
                return Err(BaseGovernanceError::InvalidArgument)
            }
            Some(storage::GovernanceTransactionKind::ScheduleActivation { .. })
            | Some(storage::GovernanceTransactionKind::ExecuteActivation { .. }) => {
                return Err(BaseGovernanceError::InvalidArgument)
            }
        }
    };
    submit(caller, action).await
}

fn authorized_action(governance: bool, pause: bool, action: &BaseGovernanceAction) -> bool {
    let safe_action = matches!(
        action,
        BaseGovernanceAction::PauseDepositMints
            | BaseGovernanceAction::PauseWithdrawals
            | BaseGovernanceAction::CancelPendingTimelock
    );
    governance || (pause && safe_action)
}

fn require_current_authorization(
    caller: Principal,
    action: &BaseGovernanceAction,
) -> Result<(), BaseGovernanceError> {
    let governance =
        admin::is_governance(caller).map_err(|_| BaseGovernanceError::StorageFailure)?;
    let pause = is_pause_principal(caller)?;
    if authorized_action(governance, pause, action) {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

fn require_current_transaction_authorization(
    caller: Principal,
    kind: &storage::GovernanceTransactionKind,
) -> Result<(), BaseGovernanceError> {
    let governance =
        admin::is_governance(caller).map_err(|_| BaseGovernanceError::StorageFailure)?;
    let pause = is_pause_principal(caller)?;
    let safe = matches!(
        kind,
        storage::GovernanceTransactionKind::PauseDepositMints
            | storage::GovernanceTransactionKind::PauseWithdrawals
            | storage::GovernanceTransactionKind::CancelTimelock { .. }
    );
    if governance || (pause && safe) {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

fn action_matches_pending(
    action: &BaseGovernanceAction,
    kind: &storage::GovernanceTransactionKind,
) -> bool {
    match (action, kind) {
        (
            BaseGovernanceAction::PauseDepositMints,
            storage::GovernanceTransactionKind::PauseDepositMints,
        )
        | (
            BaseGovernanceAction::PauseWithdrawals,
            storage::GovernanceTransactionKind::PauseWithdrawals,
        )
        | (
            BaseGovernanceAction::CancelPendingTimelock,
            storage::GovernanceTransactionKind::CancelTimelock { .. },
        )
        | (
            BaseGovernanceAction::ScheduleActivation,
            storage::GovernanceTransactionKind::ScheduleActivation { .. },
        )
        | (
            BaseGovernanceAction::ExecuteActivation,
            storage::GovernanceTransactionKind::ExecuteActivation { .. },
        ) => true,
        (
            BaseGovernanceAction::SetServiceFee { value },
            storage::GovernanceTransactionKind::SetServiceFee { value: pending },
        ) => nat_u128(value).is_some_and(|value| value == *pending),
        _ => false,
    }
}

fn dangerous_governance_kind(kind: &storage::GovernanceTransactionKind) -> bool {
    matches!(
        kind,
        storage::GovernanceTransactionKind::SetServiceFee { .. }
            | storage::GovernanceTransactionKind::ScheduleActivation { .. }
            | storage::GovernanceTransactionKind::ExecuteActivation { .. }
    )
}

fn emergency_base_actions_pending() -> Result<bool, BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow()
            .emergency_base_actions_pending()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn should_resume_activation(activates: bool, emergency_pending: bool) -> bool {
    activates && !emergency_pending
}

fn abort_if_emergency_unbroadcast(
    transaction: &storage::GovernanceTransaction,
) -> Result<bool, BaseGovernanceError> {
    if !dangerous_governance_kind(&transaction.kind)
        || !matches!(
            transaction.state,
            storage::GovernanceTransactionState::Prepared
                | storage::GovernanceTransactionState::Signed
        )
        || !emergency_base_actions_pending()?
    {
        return Ok(false);
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .abort_unbroadcast_governance_transaction_for_emergency(transaction)
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    Ok(true)
}

async fn continue_pending(
    caller: Principal,
    config: &crate::config::BridgeInitArgs,
    mut transaction: storage::GovernanceTransaction,
) -> Result<BaseGovernanceReceipt, BaseGovernanceError> {
    if abort_if_emergency_unbroadcast(&transaction)? {
        return Err(BaseGovernanceError::Busy {
            operation_id: transaction.id,
        });
    }
    if matches!(
        transaction.state,
        storage::GovernanceTransactionState::NonceConflict { .. }
    ) {
        return recover_nonce_conflict(caller, config, transaction).await;
    }
    if let storage::GovernanceTransactionState::Broadcasting { transaction_hash } =
        transaction.state
    {
        let known = evm_rpc::transaction_known(config, transaction_hash)
            .await
            .map_err(|_| BaseGovernanceError::BroadcastAmbiguous {
                operation_id: transaction.id,
            })?;
        require_current_transaction_authorization(caller, &transaction.kind)?;
        if known {
            transaction.state = storage::GovernanceTransactionState::Submitted { transaction_hash };
            persist(&transaction)?;
            return Ok(receipt(&transaction, Some(transaction_hash)));
        }
        let operator = crate::api::cached_governance_operator_address(config)
            .await
            .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
        let observed_nonce = evm_rpc::transaction_count(config, operator)
            .await
            .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
        require_current_transaction_authorization(caller, &transaction.kind)?;
        if observed_nonce > transaction.envelope.nonce {
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .resolve_governance_nonce_conflict(&transaction, observed_nonce)
                    .map_err(|_| BaseGovernanceError::StorageFailure)
            })?;
            return Err(BaseGovernanceError::NonceConflict {
                operation_id: transaction.id,
            });
        }
        if dangerous_governance_kind(&transaction.kind) && emergency_base_actions_pending()? {
            return Err(BaseGovernanceError::BroadcastAmbiguous {
                operation_id: transaction.id,
            });
        }
    }
    if let storage::GovernanceTransactionState::Submitted { transaction_hash } = transaction.state {
        match evm_rpc::confirmed_receipt_outcome(config, transaction_hash).await {
            Ok(evm_rpc::ConfirmedReceiptOutcome::Succeeded {
                receipt_block_number,
                ..
            }) => {
                require_current_transaction_authorization(caller, &transaction.kind)?;
                let activates = matches!(
                    transaction.kind,
                    storage::GovernanceTransactionKind::ExecuteActivation { .. }
                );
                if activates {
                    let observed = evm_rpc::bridge_snapshot(config)
                        .await
                        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
                    require_current_transaction_authorization(caller, &transaction.kind)?;
                    if observed.snapshot.deposits_paused || observed.snapshot.withdrawals_paused {
                        return Err(BaseGovernanceError::ObservationUnavailable);
                    }
                }
                transaction.state = storage::GovernanceTransactionState::Confirmed {
                    transaction_hash,
                    receipt_block_number,
                };
                complete(&transaction)?;
                if should_resume_activation(activates, emergency_base_actions_pending()?) {
                    crate::admin::resume(caller)
                        .map_err(|_| BaseGovernanceError::StorageFailure)?;
                }
                return Ok(receipt(&transaction, Some(transaction_hash)));
            }
            Ok(evm_rpc::ConfirmedReceiptOutcome::Reverted {
                receipt_block_number,
                ..
            }) => {
                require_current_transaction_authorization(caller, &transaction.kind)?;
                transaction.state = storage::GovernanceTransactionState::Reverted {
                    transaction_hash,
                    receipt_block_number,
                };
                complete(&transaction)?;
                return Err(BaseGovernanceError::BroadcastAmbiguous {
                    operation_id: transaction.id,
                });
            }
            Ok(evm_rpc::ConfirmedReceiptOutcome::Missing)
            | Ok(evm_rpc::ConfirmedReceiptOutcome::Pending { .. })
            | Err(_) => {
                return Err(BaseGovernanceError::BroadcastAmbiguous {
                    operation_id: transaction.id,
                });
            }
        }
    }
    if transaction.envelope.signed_transaction.is_none() {
        let signed = signer::sign_governance(&transaction.envelope, config)
            .await
            .map_err(|_| BaseGovernanceError::SigningUnavailable)?;
        require_current_transaction_authorization(caller, &transaction.kind)?;
        transaction.envelope.signed_transaction = Some(signed);
        transaction.state = storage::GovernanceTransactionState::Signed;
        persist(&transaction)?;
    }
    if abort_if_emergency_unbroadcast(&transaction)? {
        return Err(BaseGovernanceError::Busy {
            operation_id: transaction.id,
        });
    }
    let raw = transaction
        .envelope
        .signed_transaction
        .as_deref()
        .ok_or(BaseGovernanceError::StorageFailure)?
        .to_vec();
    let transaction_hash = evm_rpc::signed_transaction_hash(&raw);
    if !matches!(
        transaction.state,
        storage::GovernanceTransactionState::Broadcasting { .. }
    ) {
        transaction.state = storage::GovernanceTransactionState::Broadcasting { transaction_hash };
        persist(&transaction)?;
    }
    match evm_rpc::broadcast(config, &raw).await {
        Ok(evm_rpc::BroadcastOutcome::Submitted(evidence)) => {
            require_current_transaction_authorization(caller, &transaction.kind)?;
            let hash = evidence
                .transaction_hash
                .ok_or(BaseGovernanceError::StorageFailure)?;
            if hash != transaction_hash {
                return Err(BaseGovernanceError::StorageFailure);
            }
            transaction.state = storage::GovernanceTransactionState::Submitted {
                transaction_hash: hash,
            };
            persist(&transaction)?;
            Ok(receipt(&transaction, Some(hash)))
        }
        Ok(evm_rpc::BroadcastOutcome::NonceConflict(evidence)) => {
            require_current_transaction_authorization(caller, &transaction.kind)?;
            let hash = evidence
                .transaction_hash
                .ok_or(BaseGovernanceError::StorageFailure)?;
            if hash != transaction_hash {
                return Err(BaseGovernanceError::StorageFailure);
            }
            transaction.state = storage::GovernanceTransactionState::NonceConflict {
                transaction_hash: hash,
            };
            persist(&transaction)?;
            recover_nonce_conflict(caller, config, transaction).await
        }
        Err(_) => Err(BaseGovernanceError::BroadcastAmbiguous {
            operation_id: transaction.id,
        }),
    }
}

async fn recover_nonce_conflict(
    caller: Principal,
    config: &crate::config::BridgeInitArgs,
    mut transaction: storage::GovernanceTransaction,
) -> Result<BaseGovernanceReceipt, BaseGovernanceError> {
    let storage::GovernanceTransactionState::NonceConflict { transaction_hash } = transaction.state
    else {
        return Err(BaseGovernanceError::StorageFailure);
    };
    let known = match evm_rpc::transaction_known(config, transaction_hash).await {
        Ok(known) => known,
        Err(_) => {
            return Err(BaseGovernanceError::NonceConflict {
                operation_id: transaction.id,
            })
        }
    };
    if known {
        match nonce_conflict_recovery(true, transaction.envelope.nonce, None) {
            NonceConflictRecovery::Submitted => {}
            NonceConflictRecovery::Resync(_) | NonceConflictRecovery::Retry => {
                return Err(BaseGovernanceError::StorageFailure)
            }
        }
        require_current_transaction_authorization(caller, &transaction.kind)?;
        transaction.state = storage::GovernanceTransactionState::Submitted { transaction_hash };
        persist(&transaction)?;
        return Ok(receipt(&transaction, Some(transaction_hash)));
    }
    let operator = crate::api::cached_governance_operator_address(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let observed_nonce = evm_rpc::transaction_count(config, operator)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    if let NonceConflictRecovery::Resync(observed_nonce) =
        nonce_conflict_recovery(false, transaction.envelope.nonce, Some(observed_nonce))
    {
        require_current_transaction_authorization(caller, &transaction.kind)?;
        STORE.with(|store| {
            store
                .borrow_mut()
                .resolve_governance_nonce_conflict(&transaction, observed_nonce)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
    }
    Err(BaseGovernanceError::NonceConflict {
        operation_id: transaction.id,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NonceConflictRecovery {
    Submitted,
    Resync(u64),
    Retry,
}

fn nonce_conflict_recovery(
    transaction_known: bool,
    transaction_nonce: u64,
    observed_nonce: Option<u64>,
) -> NonceConflictRecovery {
    if transaction_known {
        NonceConflictRecovery::Submitted
    } else {
        match observed_nonce {
            Some(nonce) if nonce > transaction_nonce => NonceConflictRecovery::Resync(nonce),
            _ => NonceConflictRecovery::Retry,
        }
    }
}

fn persist(transaction: &storage::GovernanceTransaction) -> Result<(), BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow_mut()
            .update_governance_transaction(transaction.clone())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn complete(transaction: &storage::GovernanceTransaction) -> Result<(), BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow_mut()
            .complete_governance_transaction(transaction.clone())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn receipt(
    transaction: &storage::GovernanceTransaction,
    hash: Option<[u8; 32]>,
) -> BaseGovernanceReceipt {
    BaseGovernanceReceipt {
        operation_id: transaction.id,
        nonce: transaction.envelope.nonce,
        transaction_hash: hash.map(Vec::from),
    }
}

fn is_pause_principal(caller: Principal) -> Result<bool, BaseGovernanceError> {
    if caller == Principal::anonymous() {
        return Ok(false);
    }
    STORE.with(|store| {
        store
            .borrow()
            .admin_state()
            .map(|state| state.pause_principal == caller)
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn encode_action(
    action: BaseGovernanceAction,
    governance_operation_id: u64,
) -> Result<(storage::GovernanceTransactionKind, [u8; 20], Vec<u8>), BaseGovernanceError> {
    let (bridge, timelock) = STORE.with(|store| {
        let config = store
            .borrow()
            .config()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::StorageFailure)?;
        Ok::<_, BaseGovernanceError>((config.contract_array(), config.timelock_array()))
    })?;
    match action {
        BaseGovernanceAction::PauseDepositMints => Ok((
            storage::GovernanceTransactionKind::PauseDepositMints,
            bridge,
            selector("pauseDepositMints()"),
        )),
        BaseGovernanceAction::PauseWithdrawals => Ok((
            storage::GovernanceTransactionKind::PauseWithdrawals,
            bridge,
            selector("pauseWithdrawals()"),
        )),
        BaseGovernanceAction::SetServiceFee { value } => {
            let value = nat_u128(&value).ok_or(BaseGovernanceError::InvalidArgument)?;
            let mut calldata = selector("setServiceFee(uint256)");
            calldata.extend_from_slice(&word_u128(value));
            Ok((
                storage::GovernanceTransactionKind::SetServiceFee { value },
                bridge,
                calldata,
            ))
        }
        BaseGovernanceAction::CancelPendingTimelock => {
            let pending = STORE.with(|store| {
                store
                    .borrow()
                    .pending_timelock_operation()
                    .map_err(|_| BaseGovernanceError::StorageFailure)?
                    .ok_or(BaseGovernanceError::InvalidArgument)
            })?;
            let mut calldata = selector("cancel(bytes32)");
            calldata.extend_from_slice(&pending.operation_id);
            Ok((
                storage::GovernanceTransactionKind::CancelTimelock {
                    operation_id: pending.operation_id,
                },
                timelock,
                calldata,
            ))
        }
        BaseGovernanceAction::ScheduleActivation => {
            let salt = activation_salt(governance_operation_id);
            let operation_id = activation_operation_id(bridge, salt);
            Ok((
                storage::GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
                timelock,
                schedule_activation_calldata(bridge, salt),
            ))
        }
        BaseGovernanceAction::ExecuteActivation => {
            let pending = STORE
                .with(|store| store.borrow().pending_timelock_operation())
                .map_err(|_| BaseGovernanceError::StorageFailure)?
                .ok_or(BaseGovernanceError::InvalidArgument)?;
            Ok((
                storage::GovernanceTransactionKind::ExecuteActivation {
                    operation_id: pending.operation_id,
                    salt: pending.salt,
                },
                timelock,
                execute_activation_calldata(bridge, pending.salt),
            ))
        }
    }
}

fn selector(signature: &str) -> Vec<u8> {
    let mut hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut hash);
    hash[..4].to_vec()
}

fn word_u128(value: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn activation_payloads() -> [Vec<u8>; 2] {
    [
        selector("unpauseDepositMints()"),
        selector("unpauseWithdrawals()"),
    ]
}

fn activation_salt(governance_operation_id: u64) -> [u8; 32] {
    let mut input = b"KINIC_BRIDGE_ACTIVATION_V1".to_vec();
    input.extend_from_slice(&governance_operation_id.to_be_bytes());
    keccak(&input)
}

fn nat_u128(value: &Nat) -> Option<u128> {
    value.0.to_string().parse().ok()
}

fn activation_operation_id(bridge: [u8; 20], salt: [u8; 32]) -> [u8; 32] {
    keccak(&activation_arguments(bridge, salt, false))
}

fn schedule_activation_calldata(bridge: [u8; 20], salt: [u8; 32]) -> Vec<u8> {
    let mut calldata =
        selector("scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)");
    calldata.extend_from_slice(&activation_arguments(bridge, salt, true));
    calldata
}

fn execute_activation_calldata(bridge: [u8; 20], salt: [u8; 32]) -> Vec<u8> {
    let mut calldata = selector("executeBatch(address[],uint256[],bytes[],bytes32,bytes32)");
    calldata.extend_from_slice(&activation_arguments(bridge, salt, false));
    calldata
}

fn activation_arguments(bridge: [u8; 20], salt: [u8; 32], include_delay: bool) -> Vec<u8> {
    let targets = encode_address_array([bridge, bridge]);
    let values = encode_u128_array([0, 0]);
    let payloads = encode_bytes_array(activation_payloads());
    let head_words = if include_delay { 6u128 } else { 5u128 };
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&word_u128(head_words * 32));
    encoded.extend_from_slice(&word_u128(head_words * 32 + targets.len() as u128));
    encoded.extend_from_slice(&word_u128(
        head_words * 32 + targets.len() as u128 + values.len() as u128,
    ));
    encoded.extend_from_slice(&[0; 32]);
    encoded.extend_from_slice(&salt);
    if include_delay {
        encoded.extend_from_slice(&word_u128(72 * 60 * 60));
    }
    encoded.extend_from_slice(&targets);
    encoded.extend_from_slice(&values);
    encoded.extend_from_slice(&payloads);
    encoded
}

fn encode_address_array(values: [[u8; 20]; 2]) -> Vec<u8> {
    let mut encoded = word_u128(2).to_vec();
    for value in values {
        encoded.extend_from_slice(&[0; 12]);
        encoded.extend_from_slice(&value);
    }
    encoded
}

fn encode_u128_array(values: [u128; 2]) -> Vec<u8> {
    let mut encoded = word_u128(2).to_vec();
    for value in values {
        encoded.extend_from_slice(&word_u128(value));
    }
    encoded
}

fn encode_bytes_array(values: [Vec<u8>; 2]) -> Vec<u8> {
    let first = encode_bytes(&values[0]);
    let second = encode_bytes(&values[1]);
    let mut encoded = word_u128(2).to_vec();
    encoded.extend_from_slice(&word_u128(64));
    encoded.extend_from_slice(&word_u128(64 + first.len() as u128));
    encoded.extend_from_slice(&first);
    encoded.extend_from_slice(&second);
    encoded
}

fn encode_bytes(value: &[u8]) -> Vec<u8> {
    let mut encoded = word_u128(value.len() as u128).to_vec();
    encoded.extend_from_slice(value);
    encoded.resize(encoded.len().next_multiple_of(32), 0);
    encoded
}

fn keccak(value: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(value);
    hasher.finalize(&mut hash);
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        action_matches_pending, activation_operation_id, activation_salt, authorized_action,
        execute_activation_calldata, nonce_conflict_recovery, selector, should_resume_activation,
        word_u128, BaseGovernanceAction, NonceConflictRecovery,
    };
    use crate::storage::GovernanceTransactionKind;

    #[test]
    fn closed_action_encoding_uses_frozen_selectors_and_uint_word() {
        assert_eq!(selector("pauseDepositMints()"), [0x15, 0x41, 0x5f, 0x22]);
        assert_eq!(selector("pauseWithdrawals()"), [0x56, 0xbb, 0x54, 0xa7]);
        assert_eq!(&word_u128(7)[16..], &7u128.to_be_bytes());
    }

    #[test]
    fn pause_principal_is_limited_to_safety_actions() {
        assert!(authorized_action(
            false,
            true,
            &BaseGovernanceAction::PauseDepositMints
        ));
        assert!(authorized_action(
            false,
            true,
            &BaseGovernanceAction::PauseWithdrawals
        ));
        assert!(authorized_action(
            false,
            true,
            &BaseGovernanceAction::CancelPendingTimelock
        ));
        assert!(!authorized_action(
            false,
            true,
            &BaseGovernanceAction::SetServiceFee { value: 1u8.into() },
        ));
        assert!(authorized_action(
            true,
            false,
            &BaseGovernanceAction::SetServiceFee { value: 1u8.into() },
        ));
        assert!(!authorized_action(
            false,
            false,
            &BaseGovernanceAction::PauseWithdrawals
        ));
    }

    #[test]
    fn pending_action_matching_rejects_safe_action_disguises_and_changed_arguments() {
        assert!(!action_matches_pending(
            &BaseGovernanceAction::PauseDepositMints,
            &GovernanceTransactionKind::ExecuteActivation {
                operation_id: [1; 32],
                salt: [2; 32],
            },
        ));
        assert!(!action_matches_pending(
            &BaseGovernanceAction::PauseWithdrawals,
            &GovernanceTransactionKind::SetServiceFee { value: 7 },
        ));
        assert!(!action_matches_pending(
            &BaseGovernanceAction::SetServiceFee { value: 8u8.into() },
            &GovernanceTransactionKind::SetServiceFee { value: 7 },
        ));
        assert!(action_matches_pending(
            &BaseGovernanceAction::SetServiceFee { value: 7u8.into() },
            &GovernanceTransactionKind::SetServiceFee { value: 7 },
        ));
    }

    #[test]
    fn activation_salt_is_domain_separated_by_governance_operation_id() {
        assert_ne!(activation_salt(0), activation_salt(1));
        assert_eq!(activation_salt(7), activation_salt(7));
    }

    #[test]
    fn nonce_conflict_recovery_requires_known_hash_or_advanced_nonce() {
        assert_eq!(
            nonce_conflict_recovery(true, 7, None),
            NonceConflictRecovery::Submitted
        );
        assert_eq!(
            nonce_conflict_recovery(false, 7, Some(8)),
            NonceConflictRecovery::Resync(8)
        );
        assert_eq!(
            nonce_conflict_recovery(false, 7, Some(7)),
            NonceConflictRecovery::Retry
        );
        assert_eq!(
            nonce_conflict_recovery(false, 7, None),
            NonceConflictRecovery::Retry
        );
    }

    #[test]
    fn activation_resume_is_suppressed_while_emergency_actions_remain() {
        assert!(should_resume_activation(true, false));
        assert!(!should_resume_activation(true, true));
        assert!(!should_resume_activation(false, false));
    }

    #[test]
    fn activation_abi_and_operation_id_match_openzeppelin_encoding() {
        let bridge = [0x11; 20];
        let salt = [0x22; 32];
        assert_eq!(
            activation_operation_id(bridge, salt),
            [
                0x05, 0x8d, 0x6b, 0xaf, 0xaa, 0x5c, 0xbb, 0x93, 0x29, 0xfd, 0x94, 0x37, 0xab, 0xad,
                0x40, 0x35, 0x3f, 0x96, 0xaa, 0x29, 0x5c, 0xbe, 0x50, 0x7b, 0x2f, 0x72, 0xeb, 0xf1,
                0x87, 0x3c, 0xe2, 0xa6,
            ]
        );
        assert_eq!(
            &execute_activation_calldata(bridge, salt)[..4],
            &[0xe3, 0x83, 0x35, 0xe5]
        );
    }
}
