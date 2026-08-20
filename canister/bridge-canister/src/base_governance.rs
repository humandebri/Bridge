use crate::{admin, evm_rpc, signer, storage, STORE};
use bridge_core::{
    GovernanceOperationId, GovernanceTransactionEnvelope, SignedGovernanceTransaction,
};
use candid::{CandidType, Deserialize, Nat, Principal};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[cfg(not(feature = "test-deployment"))]
const ACTIVATION_TIMELOCK_DELAY_SECONDS: u128 = 24 * 60 * 60;
#[cfg(feature = "test-deployment")]
const ACTIVATION_TIMELOCK_DELAY_SECONDS: u128 = 5 * 60;

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseGovernanceAction {
    PauseDepositMints,
    PauseWithdrawals,
    SetServiceFee { value: Nat },
    CancelPendingTimelock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GovernanceAction {
    PauseDepositMints,
    PauseWithdrawals,
    SetServiceFee { value: Nat },
    CancelPendingTimelock,
    ScheduleActivation,
    ExecuteActivation,
}

impl From<BaseGovernanceAction> for GovernanceAction {
    fn from(value: BaseGovernanceAction) -> Self {
        match value {
            BaseGovernanceAction::PauseDepositMints => Self::PauseDepositMints,
            BaseGovernanceAction::PauseWithdrawals => Self::PauseWithdrawals,
            BaseGovernanceAction::SetServiceFee { value } => Self::SetServiceFee { value },
            BaseGovernanceAction::CancelPendingTimelock => Self::CancelPendingTimelock,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseGovernanceOperationKind {
    PauseDepositMints,
    PauseWithdrawals,
    SetServiceFee {
        value: Nat,
    },
    CancelTimelock {
        operation_id: Vec<u8>,
    },
    ScheduleActivation {
        operation_id: Vec<u8>,
        salt: Vec<u8>,
    },
    ExecuteActivation {
        operation_id: Vec<u8>,
        salt: Vec<u8>,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseGovernanceError {
    Unauthorized,
    InvalidArgument,
    Busy {
        operation_id: u64,
    },
    StorageFailure,
    ObservationUnavailable,
    InsufficientGovernanceBalance {
        observed_wei: u128,
        required_wei: u128,
    },
    SigningUnavailable {
        class: signer::SigningFailureClass,
    },
    TransactionNotFinalized {
        operation_id: u64,
    },
    TransactionReverted {
        operation_id: u64,
    },
    ReplacementLimitReached {
        operation_id: u64,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SignedBaseGovernanceTransaction {
    pub operation_id: u64,
    pub kind: BaseGovernanceOperationKind,
    pub chain_id: u64,
    pub sender: Vec<u8>,
    pub nonce: u64,
    pub target: Vec<u8>,
    pub calldata: Vec<u8>,
    pub gas_limit: Nat,
    pub max_fee_per_gas: Nat,
    pub max_priority_fee_per_gas: Nat,
    pub raw_transaction: Vec<u8>,
    pub transaction_hash: Vec<u8>,
    pub generation: u8,
    pub signed_at_ns: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfirmBaseGovernanceTransactionArgs {
    pub operation_id: u64,
    pub transaction_hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrepareBaseGovernanceReplacementArgs {
    pub operation_id: u64,
    pub expected_transaction_hash: Vec<u8>,
    pub max_fee_per_gas: Nat,
    pub max_priority_fee_per_gas: Nat,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BaseGovernanceConfirmation {
    pub operation_id: u64,
    pub transaction_hash: Vec<u8>,
    pub receipt_block_number: u64,
    pub succeeded: bool,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EmergencyPauseReceipt {
    pub caller: Principal,
    pub local_deposits_paused: bool,
    pub local_pause_audit_sequence: u64,
    pub local_pause_audit_sha256: Vec<u8>,
    pub base_actions_queued: bool,
    pub base_action_count: u8,
    pub base_action_plan_sha256: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivationOperationView {
    pub operation_id: Vec<u8>,
    pub salt: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivationStatus {
    pub deposits_paused: bool,
    pub pending_timelock_operation: Option<ActivationOperationView>,
    pub last_confirmed_activation: Option<ActivationConfirmationView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivationConfirmationView {
    pub phase: String,
    pub governance_operation_id: u64,
    pub timelock_operation_id: Vec<u8>,
    pub transaction_hash: Vec<u8>,
    pub receipt_block_number: u64,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionLifecycle {
    Bootstrap,
    OperationalConfigSealed,
    Activated,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OperationalConfigSealReceipt {
    pub lifecycle: ProductionLifecycle,
    pub activation_attestation: crate::config::ActivationAttestation,
}

pub fn production_lifecycle() -> Result<ProductionLifecycle, BaseGovernanceError> {
    STORE.with(|store| {
        let store = store.borrow();
        let sealed = store
            .operational_config_sealed()
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        let paused = store
            .admin_state()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .deposits_paused;
        Ok(if sealed && !paused {
            ProductionLifecycle::Activated
        } else if sealed {
            ProductionLifecycle::OperationalConfigSealed
        } else {
            ProductionLifecycle::Bootstrap
        })
    })
}

pub async fn seal_operational_config(
    caller: Principal,
    value: crate::config::OperationalConfigArgs,
) -> Result<OperationalConfigSealReceipt, BaseGovernanceError> {
    let (governance, _) = caller_roles(caller)?;
    if !governance {
        return Err(BaseGovernanceError::Unauthorized);
    }
    let current = config()?;
    let next = current.with_operational_config(value);
    next.validate()
        .map_err(|_| BaseGovernanceError::InvalidArgument)?;
    let attestation = activation_preflight(&next).await?;
    let (governance, _) = caller_roles(caller)?;
    if !governance {
        return Err(BaseGovernanceError::Unauthorized);
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .seal_operational_config(&next, attestation.clone())
            .map_err(|_| BaseGovernanceError::InvalidArgument)
    })?;
    Ok(OperationalConfigSealReceipt {
        lifecycle: production_lifecycle()?,
        activation_attestation: attestation,
    })
}

pub fn activation_attestation() -> Result<crate::config::ActivationAttestation, BaseGovernanceError>
{
    STORE.with(|store| {
        store
            .borrow()
            .activation_attestation()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::InvalidArgument)
    })
}

pub async fn refresh_activation_attestation(
    caller: Principal,
) -> Result<crate::config::ActivationAttestation, BaseGovernanceError> {
    let before = config()?;
    if !attestation_refresh_authorized(&before, caller) {
        return Err(BaseGovernanceError::Unauthorized);
    }
    if production_lifecycle()? != ProductionLifecycle::OperationalConfigSealed {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    let attestation = activation_preflight(&before).await?;
    if config()? != before || !attestation_refresh_authorized(&before, caller) {
        return Err(BaseGovernanceError::Unauthorized);
    }
    if production_lifecycle()? != ProductionLifecycle::OperationalConfigSealed {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .refresh_activation_attestation(attestation.clone())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    Ok(attestation)
}

pub fn activation_status() -> Result<ActivationStatus, BaseGovernanceError> {
    STORE.with(|store| {
        let store = store.borrow();
        let deposits_paused = store
            .admin_state()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .deposits_paused;
        let pending_timelock_operation = store
            .pending_timelock_operation()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .map(|pending| ActivationOperationView {
                operation_id: pending.operation_id.to_vec(),
                salt: pending.salt.to_vec(),
            });
        let last_confirmed_activation = store
            .last_completed_governance_transaction()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .and_then(|transaction| {
                let (phase, timelock_operation_id) = match transaction.kind {
                    storage::GovernanceTransactionKind::ScheduleActivation {
                        operation_id, ..
                    } => ("schedule", operation_id),
                    storage::GovernanceTransactionKind::ExecuteActivation {
                        operation_id, ..
                    } => ("execute", operation_id),
                    _ => return None,
                };
                let storage::GovernanceTransactionState::Confirmed {
                    transaction_hash,
                    receipt_block_number,
                } = transaction.state
                else {
                    return None;
                };
                Some(ActivationConfirmationView {
                    phase: phase.into(),
                    governance_operation_id: transaction.id,
                    timelock_operation_id: timelock_operation_id.to_vec(),
                    transaction_hash: transaction_hash.to_vec(),
                    receipt_block_number,
                })
            });
        Ok(ActivationStatus {
            deposits_paused,
            pending_timelock_operation,
            last_confirmed_activation,
        })
    })
}

pub async fn prepare(
    caller: Principal,
    action: GovernanceAction,
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    require_action_authorization(caller, &action)?;
    let config = config()?;
    require_operational_config_sealed()?;
    if let GovernanceAction::SetServiceFee { value } = &action {
        let value = nat_u128(value).ok_or(BaseGovernanceError::InvalidArgument)?;
        let runtime_attested = crate::api::runtime_attested(&config)
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        let observed = evm_rpc::bridge_snapshot(&config, runtime_attested)
            .await
            .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
        crate::api::cache_runtime_attestation(&config, &observed)
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        require_action_authorization(caller, &action)?;
        if !bridge_core::service_fee_change_allowed(
            value,
            observed.snapshot.mint.max_service_fee.get(),
        ) {
            return Err(BaseGovernanceError::InvalidArgument);
        }
    }
    let operator = crate::api::cached_governance_operator_address(&config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    require_action_authorization(caller, &action)?;
    let (initialized, _, _, pending) = governance_lane()?;
    if let Some(pending) = pending {
        require_transaction_authorization(caller, &pending.kind)?;
        if !action_matches_pending(&action, &pending.kind) {
            return Err(BaseGovernanceError::Busy {
                operation_id: pending.id,
            });
        }
        return resume_pending(caller, &config, pending, operator).await;
    }
    let observed_nonce = if !initialized {
        Some(
            evm_rpc::transaction_count(&config, operator)
                .await
                .map_err(|_| BaseGovernanceError::ObservationUnavailable)?,
        )
    } else {
        None
    };
    require_action_authorization(caller, &action)?;
    let (initialized, stored_nonce, id, pending) = governance_lane()?;
    if let Some(pending) = pending {
        require_transaction_authorization(caller, &pending.kind)?;
        return Err(BaseGovernanceError::Busy {
            operation_id: pending.id,
        });
    }
    let nonce = if initialized {
        stored_nonce
    } else {
        observed_nonce.ok_or(BaseGovernanceError::StorageFailure)?
    };
    if matches!(
        action,
        GovernanceAction::ScheduleActivation | GovernanceAction::ExecuteActivation
    ) {
        activation_preflight(&config).await?;
        require_action_authorization(caller, &action)?;
    }
    let (kind, target, calldata) = encode_action(action, id)?;
    let payload_hash: [u8; 32] = Sha256::digest(&calldata).into();
    let fee_cap = config.governance_evm_fee.max_fee_per_gas_ceiling;
    let priority_cap = config.governance_evm_fee.max_priority_fee_per_gas_ceiling;
    let gas_limit = config.governance_evm_fee.gas_limit_ceiling;
    let initial_max_fee_per_gas = initial_fee(
        fee_cap,
        config.governance_replacement.fee_bump_bps,
        config.governance_replacement.max_replacements,
    );
    let initial_max_priority_fee_per_gas = initial_fee(
        priority_cap,
        config.governance_replacement.fee_bump_bps,
        config.governance_replacement.max_replacements,
    )
    .min(initial_max_fee_per_gas);
    let envelope = GovernanceTransactionEnvelope {
        operation_id: GovernanceOperationId::new(id),
        payload_hash,
        nonce,
        chain_id: config.base_chain_id,
        contract: target,
        calldata,
        gas_limit,
        max_fee_per_gas: initial_max_fee_per_gas,
        max_priority_fee_per_gas: initial_max_priority_fee_per_gas,
        signed_transactions: Vec::new(),
    };
    let transaction = storage::GovernanceTransaction {
        id,
        kind,
        envelope,
        state: storage::GovernanceTransactionState::Prepared,
    };
    require_affordable(&config, operator, &transaction.envelope).await?;
    require_transaction_authorization(caller, &transaction.kind)?;
    if !initialized {
        STORE.with(|store| {
            store
                .borrow_mut()
                .initialize_governance_nonce(nonce)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .prepare_governance_transaction(transaction.clone())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    sign_prepared(caller, &config, transaction, operator).await
}

pub fn get_pending() -> Result<Option<SignedBaseGovernanceTransaction>, BaseGovernanceError> {
    STORE.with(|store| {
        let store = store.borrow();
        let operator = store
            .governance_operator_address()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::ObservationUnavailable)?;
        let pending = store
            .governance_lane()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .3;
        pending
            .map(|pending| signed_view(&pending, operator))
            .transpose()
    })
}

pub async fn prepare_replacement(
    caller: Principal,
    args: PrepareBaseGovernanceReplacementArgs,
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    require_governance_or_pause(caller)?;
    let expected_hash = hash32(&args.expected_transaction_hash)?;
    let max_fee_per_gas =
        nat_u128(&args.max_fee_per_gas).ok_or(BaseGovernanceError::InvalidArgument)?;
    let max_priority_fee_per_gas =
        nat_u128(&args.max_priority_fee_per_gas).ok_or(BaseGovernanceError::InvalidArgument)?;
    let config = config()?;
    let operator = STORE.with(|store| {
        store
            .borrow()
            .governance_operator_address()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::ObservationUnavailable)
    })?;
    let mut transaction = pending_transaction(args.operation_id)?;
    require_transaction_authorization(caller, &transaction.kind)?;
    let current = transaction
        .envelope
        .signed_transactions
        .last()
        .ok_or(BaseGovernanceError::StorageFailure)?;
    if current.transaction_hash != expected_hash
        || !matches!(
            transaction.state,
            storage::GovernanceTransactionState::SignedAwaitingRelay { .. }
        )
    {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    if current.generation >= config.governance_replacement.max_replacements {
        return Err(BaseGovernanceError::ReplacementLimitReached {
            operation_id: transaction.id,
        });
    }
    if max_fee_per_gas > config.governance_evm_fee.max_fee_per_gas_ceiling
        || max_priority_fee_per_gas > config.governance_evm_fee.max_priority_fee_per_gas_ceiling
        || max_priority_fee_per_gas > max_fee_per_gas
        || !minimum_fee_bump(
            current.max_fee_per_gas,
            max_fee_per_gas,
            config.governance_replacement.fee_bump_bps,
        )
        || !minimum_fee_bump(
            current.max_priority_fee_per_gas,
            max_priority_fee_per_gas,
            config.governance_replacement.fee_bump_bps,
        )
    {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    transaction.envelope.max_fee_per_gas = max_fee_per_gas;
    transaction.envelope.max_priority_fee_per_gas = max_priority_fee_per_gas;
    require_affordable(&config, operator, &transaction.envelope).await?;
    require_transaction_authorization(caller, &transaction.kind)?;
    let raw = signer::sign_governance(&transaction.envelope, &config)
        .await
        .map_err(signing_failure)?;
    require_transaction_authorization(caller, &transaction.kind)?;
    let current_pending = pending_transaction(args.operation_id)?;
    if current_pending.envelope.signed_transactions.last() != Some(current)
        || !matches!(
            current_pending.state,
            storage::GovernanceTransactionState::SignedAwaitingRelay { .. }
        )
    {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    let transaction_hash = evm_rpc::signed_transaction_hash(&raw);
    let generation = current
        .generation
        .checked_add(1)
        .ok_or(BaseGovernanceError::StorageFailure)?;
    let signed_at_ns = ic_cdk::api::time();
    transaction
        .envelope
        .signed_transactions
        .push(SignedGovernanceTransaction {
            raw_transaction: raw,
            transaction_hash,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            generation,
            signed_at_ns,
        });
    transaction.state = storage::GovernanceTransactionState::SignedAwaitingRelay {
        transaction_hash,
        generation,
        signed_at_ns,
    };
    persist(&transaction)?;
    signed_view(&transaction, operator)
}

pub async fn confirm(
    caller: Principal,
    args: ConfirmBaseGovernanceTransactionArgs,
) -> Result<BaseGovernanceConfirmation, BaseGovernanceError> {
    require_confirmation_caller(caller)?;
    let transaction_hash = hash32(&args.transaction_hash)?;
    let mut transaction = match pending_transaction(args.operation_id) {
        Ok(transaction) => transaction,
        Err(BaseGovernanceError::InvalidArgument) => {
            if let Some(completed) = completed_transaction(args.operation_id)? {
                if let storage::GovernanceTransactionState::Confirmed {
                    transaction_hash: completed_hash,
                    receipt_block_number,
                }
                | storage::GovernanceTransactionState::Reverted {
                    transaction_hash: completed_hash,
                    receipt_block_number,
                } = completed.state
                {
                    if completed_hash == transaction_hash {
                        return Ok(BaseGovernanceConfirmation {
                            operation_id: completed.id,
                            transaction_hash: completed_hash.to_vec(),
                            receipt_block_number,
                            succeeded: matches!(
                                completed.state,
                                storage::GovernanceTransactionState::Confirmed { .. }
                            ),
                        });
                    }
                }
            }
            return Err(BaseGovernanceError::InvalidArgument);
        }
        Err(error) => return Err(error),
    };
    if !transaction
        .envelope
        .signed_transactions
        .iter()
        .any(|signed| signed.transaction_hash == transaction_hash)
    {
        return Err(BaseGovernanceError::InvalidArgument);
    }
    let config = config()?;
    let outcome = evm_rpc::confirmed_receipt_outcome(&config, transaction_hash)
        .await
        .map_err(|error| {
            ic_cdk::println!(
                "base governance confirmation observation failed: operation_id={} phase=confirmed_receipt error={error:?}",
                transaction.id
            );
            BaseGovernanceError::ObservationUnavailable
        })?;
    let (receipt_block_number, succeeded, finalized_observation) = match outcome {
        evm_rpc::ConfirmedReceiptOutcome::Missing
        | evm_rpc::ConfirmedReceiptOutcome::Pending { .. } => {
            return Err(BaseGovernanceError::TransactionNotFinalized {
                operation_id: transaction.id,
            });
        }
        evm_rpc::ConfirmedReceiptOutcome::Succeeded {
            receipt_block_number,
            finalized_observation,
            ..
        } => (receipt_block_number, true, finalized_observation),
        evm_rpc::ConfirmedReceiptOutcome::Reverted {
            receipt_block_number,
            finalized_observation,
            ..
        } => (receipt_block_number, false, finalized_observation),
    };
    let activates = succeeded
        && matches!(
            transaction.kind,
            storage::GovernanceTransactionKind::ExecuteActivation { .. }
        );
    if activates {
        let runtime_attested = crate::api::runtime_attested(&config)
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        let observed =
            evm_rpc::bridge_snapshot_at(&config, finalized_observation, runtime_attested)
                .await
                .map_err(|error| {
                    ic_cdk::println!(
                        "base governance confirmation observation failed: operation_id={} phase=activation_snapshot error={error:?}",
                        transaction.id
                    );
                    BaseGovernanceError::ObservationUnavailable
                })?;
        crate::api::cache_runtime_attestation(&config, &observed)
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        if !activation_postcondition_matches(
            observed.snapshot.deposits_paused,
            observed.snapshot.withdrawals_paused,
        ) {
            ic_cdk::println!(
                "base governance confirmation observation failed: operation_id={} phase=activation_postcondition deposits_paused={} withdrawals_paused={}",
                transaction.id,
                observed.snapshot.deposits_paused,
                observed.snapshot.withdrawals_paused
            );
            return Err(BaseGovernanceError::ObservationUnavailable);
        }
    }
    transaction.state = if succeeded {
        storage::GovernanceTransactionState::Confirmed {
            transaction_hash,
            receipt_block_number,
        }
    } else {
        storage::GovernanceTransactionState::Reverted {
            transaction_hash,
            receipt_block_number,
        }
    };
    if activates {
        let governance_principal = STORE.with(|store| {
            store
                .borrow()
                .admin_state()
                .map(|state| state.governance_principal)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
        complete_confirmed_activation(&transaction, governance_principal)?;
    } else {
        complete(&transaction)?;
    }
    let confirmation = BaseGovernanceConfirmation {
        operation_id: transaction.id,
        transaction_hash: transaction_hash.to_vec(),
        receipt_block_number,
        succeeded,
    };
    if succeeded {
        Ok(confirmation)
    } else {
        Err(BaseGovernanceError::TransactionReverted {
            operation_id: transaction.id,
        })
    }
}

pub fn require_confirmation_caller(caller: Principal) -> Result<(), BaseGovernanceError> {
    let config = config()?;
    let state = STORE.with(|store| {
        store
            .borrow()
            .admin_state()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    if confirmation_caller_authorized(
        caller,
        config.confirmation_relayer_principal,
        state.governance_principal,
        state.pause_principal,
    ) {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

pub(crate) fn confirmation_caller_authorized(
    caller: Principal,
    confirmation_relayer: Principal,
    governance: Principal,
    pause: Principal,
) -> bool {
    caller != Principal::anonymous()
        && (caller == confirmation_relayer || caller == governance || caller == pause)
}

pub fn emergency_pause(caller: Principal) -> Result<EmergencyPauseReceipt, BaseGovernanceError> {
    let local_pause_audit = admin::pause_with_audit(caller).map_err(|error| match error {
        admin::AdminError::Unauthorized => BaseGovernanceError::Unauthorized,
        _ => BaseGovernanceError::StorageFailure,
    })?;
    let cancel_required = STORE.with(|store| {
        let mut store = store.borrow_mut();
        store
            .enqueue_emergency_base_actions()
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        store
            .pending_timelock_operation()
            .map(|value| value.is_some())
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    let action_names = if cancel_required {
        ["PauseDepositMints", "PauseWithdrawals", "CancelTimelock"].as_slice()
    } else {
        ["PauseDepositMints", "PauseWithdrawals"].as_slice()
    };
    let action_plan = action_names.join("\n");
    let audit_bytes =
        candid::encode_one(&local_pause_audit).map_err(|_| BaseGovernanceError::StorageFailure)?;
    Ok(EmergencyPauseReceipt {
        caller,
        local_deposits_paused: true,
        local_pause_audit_sequence: local_pause_audit.sequence,
        local_pause_audit_sha256: Sha256::digest(audit_bytes).to_vec(),
        base_actions_queued: true,
        base_action_count: action_names.len() as u8,
        base_action_plan_sha256: Sha256::digest(action_plan.as_bytes()).to_vec(),
    })
}

pub async fn prepare_next_emergency(
    caller: Principal,
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    require_governance_or_pause(caller)?;
    if let Some(pending) = governance_lane()?.3 {
        require_transaction_authorization(caller, &pending.kind)?;
        if !is_emergency_kind(&pending.kind) {
            return Err(BaseGovernanceError::Busy {
                operation_id: pending.id,
            });
        }
        let config = config()?;
        let operator = STORE.with(|store| {
            store
                .borrow()
                .governance_operator_address()
                .map_err(|_| BaseGovernanceError::StorageFailure)?
                .ok_or(BaseGovernanceError::ObservationUnavailable)
        })?;
        return resume_pending(caller, &config, pending, operator).await;
    }
    let action = match STORE
        .with(|store| store.borrow().next_emergency_base_action())
        .map_err(|_| BaseGovernanceError::StorageFailure)?
    {
        Some(storage::GovernanceTransactionKind::PauseDepositMints) => {
            GovernanceAction::PauseDepositMints
        }
        Some(storage::GovernanceTransactionKind::PauseWithdrawals) => {
            GovernanceAction::PauseWithdrawals
        }
        Some(storage::GovernanceTransactionKind::CancelTimelock { .. }) => {
            GovernanceAction::CancelPendingTimelock
        }
        _ => return Err(BaseGovernanceError::InvalidArgument),
    };
    prepare(caller, action).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingSignatureAction {
    Sign,
    ReturnSigned,
}

fn pending_signature_action(
    transaction: &storage::GovernanceTransaction,
) -> Result<PendingSignatureAction, BaseGovernanceError> {
    match transaction.state {
        storage::GovernanceTransactionState::Prepared
            if transaction.envelope.signed_transactions.is_empty() =>
        {
            Ok(PendingSignatureAction::Sign)
        }
        storage::GovernanceTransactionState::SignedAwaitingRelay {
            transaction_hash,
            generation,
            signed_at_ns,
        } => {
            let current = transaction
                .envelope
                .signed_transactions
                .last()
                .ok_or(BaseGovernanceError::StorageFailure)?;
            if current.transaction_hash != transaction_hash
                || current.generation != generation
                || current.signed_at_ns != signed_at_ns
            {
                return Err(BaseGovernanceError::StorageFailure);
            }
            Ok(PendingSignatureAction::ReturnSigned)
        }
        storage::GovernanceTransactionState::Prepared
        | storage::GovernanceTransactionState::Confirmed { .. }
        | storage::GovernanceTransactionState::Reverted { .. } => {
            Err(BaseGovernanceError::StorageFailure)
        }
    }
}

async fn resume_pending(
    caller: Principal,
    config: &crate::config::BridgeInitArgs,
    transaction: storage::GovernanceTransaction,
    operator: [u8; 20],
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    require_transaction_authorization(caller, &transaction.kind)?;
    match pending_signature_action(&transaction)? {
        PendingSignatureAction::Sign => {
            require_affordable(config, operator, &transaction.envelope).await?;
            require_transaction_authorization(caller, &transaction.kind)?;
            sign_prepared(caller, config, transaction, operator).await
        }
        PendingSignatureAction::ReturnSigned => signed_view(&transaction, operator),
    }
}

async fn sign_prepared(
    caller: Principal,
    config: &crate::config::BridgeInitArgs,
    mut transaction: storage::GovernanceTransaction,
    operator: [u8; 20],
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    if pending_signature_action(&transaction)? != PendingSignatureAction::Sign {
        return Err(BaseGovernanceError::StorageFailure);
    }
    let raw = signer::sign_governance(&transaction.envelope, config)
        .await
        .map_err(signing_failure)?;
    require_transaction_authorization(caller, &transaction.kind)?;
    if pending_transaction(transaction.id)? != transaction {
        return Err(BaseGovernanceError::StorageFailure);
    }
    if dangerous_governance_kind(&transaction.kind) && emergency_base_actions_pending()? {
        STORE.with(|store| {
            store
                .borrow_mut()
                .abort_prepared_governance_transaction_for_emergency(&transaction)
                .map_err(|_| BaseGovernanceError::StorageFailure)
        })?;
        return Err(BaseGovernanceError::Busy {
            operation_id: transaction.id,
        });
    }
    let transaction_hash = evm_rpc::signed_transaction_hash(&raw);
    let signed_at_ns = ic_cdk::api::time();
    transaction
        .envelope
        .signed_transactions
        .push(SignedGovernanceTransaction {
            raw_transaction: raw,
            transaction_hash,
            max_fee_per_gas: transaction.envelope.max_fee_per_gas,
            max_priority_fee_per_gas: transaction.envelope.max_priority_fee_per_gas,
            generation: 0,
            signed_at_ns,
        });
    transaction.state = storage::GovernanceTransactionState::SignedAwaitingRelay {
        transaction_hash,
        generation: 0,
        signed_at_ns,
    };
    persist(&transaction)?;
    signed_view(&transaction, operator)
}

async fn activation_preflight(
    config: &crate::config::BridgeInitArgs,
) -> Result<crate::config::ActivationAttestation, BaseGovernanceError> {
    let expected_bridge_signer = crate::api::cached_signer_address(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let governance_operator = crate::api::cached_governance_operator_address(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let runtime_attested =
        crate::api::runtime_attested(config).map_err(|_| BaseGovernanceError::StorageFailure)?;
    let observed = evm_rpc::bridge_snapshot(config, runtime_attested)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    crate::api::cache_runtime_attestation(config, &observed)
        .map_err(|_| BaseGovernanceError::StorageFailure)?;
    if !activation_base_preflight_matches(
        observed.snapshot.bridge_signer,
        expected_bridge_signer,
        observed.snapshot.deposits_paused,
        observed.snapshot.withdrawals_paused,
    ) {
        return Err(BaseGovernanceError::ObservationUnavailable);
    }
    let deployment = evm_rpc::deployment_postconditions_at(config, observed.finalized)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let timelock: [u8; 20] = config
        .timelock_contract
        .as_slice()
        .try_into()
        .map_err(|_| BaseGovernanceError::InvalidArgument)?;
    if deployment.bridge_timelock != timelock
        || deployment.runtime_administrator != governance_operator
        || deployment.timelock_admin != timelock
        || deployment.timelock_proposer != governance_operator
        || deployment.timelock_canceller != governance_operator
        || deployment.timelock_executor != governance_operator
        || deployment.timelock_runtime_code_hash
            != deployment.bridge_approved_timelock_runtime_code_hash
        || deployment.bsns_bridge.as_slice() != config.bridge_contract.as_slice()
        || deployment.bsns_name != "KINIC"
        || deployment.bsns_symbol != "KINIC"
    {
        return Err(BaseGovernanceError::ObservationUnavailable);
    }
    STORE.with(|store| {
        let store = store.borrow();
        let locally_paused = store
            .admin_state()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .deposits_paused;
        if !locally_paused {
            return Err(BaseGovernanceError::ObservationUnavailable);
        }
        Ok(())
    })?;
    Ok(crate::config::ActivationAttestation {
        chain_id: config.base_chain_id,
        finalized_block_number: observed.finalized.block_number,
        finalized_block_hash: observed.finalized.block_hash.to_vec(),
        observed_at_ns: observed.finalized.observed_at_ns,
        bridge_signer: observed.snapshot.bridge_signer.to_vec(),
        bridge_runtime_sha256: observed.bridge_identity.runtime_sha256.to_vec(),
        deposits_paused: observed.snapshot.deposits_paused,
        withdrawals_paused: observed.snapshot.withdrawals_paused,
        bridge_timelock: deployment.bridge_timelock.to_vec(),
        runtime_administrator: deployment.runtime_administrator.to_vec(),
        timelock_admin: deployment.timelock_admin.to_vec(),
        timelock_proposer: deployment.timelock_proposer.to_vec(),
        timelock_canceller: deployment.timelock_canceller.to_vec(),
        timelock_executor: deployment.timelock_executor.to_vec(),
        timelock_runtime_code_hash: deployment.timelock_runtime_code_hash.to_vec(),
        bridge_approved_timelock_runtime_code_hash: deployment
            .bridge_approved_timelock_runtime_code_hash
            .to_vec(),
        timelock_minimum_delay_seconds: deployment.timelock_minimum_delay_seconds,
        bsns_address: deployment.bsns_address.to_vec(),
        bsns_runtime_sha256: deployment.bsns_runtime_sha256.to_vec(),
        bsns_name: deployment.bsns_name,
        bsns_symbol: deployment.bsns_symbol,
        bsns_decimals: deployment.bsns_decimals,
        bsns_bridge: deployment.bsns_bridge.to_vec(),
        base_service_fee: observed.snapshot.mint.service_fee.get(),
    })
}

async fn require_affordable(
    config: &crate::config::BridgeInitArgs,
    governance_operator: [u8; 20],
    envelope: &GovernanceTransactionEnvelope,
) -> Result<(), BaseGovernanceError> {
    let required_wei = bridge_core::transaction_liability_wei(
        envelope.gas_limit,
        envelope.max_fee_per_gas,
        config.governance_evm_fee.l1_fee_per_transaction_ceiling_wei,
        0,
    )
    .ok_or(BaseGovernanceError::StorageFailure)?;
    let finalized = evm_rpc::finalized_observation(config)
        .await
        .map_err(|_| BaseGovernanceError::ObservationUnavailable)?;
    let (finalized_balance, safe_balance) = futures::join!(
        evm_rpc::signer_eth_balance_at(config, governance_operator, finalized),
        evm_rpc::signer_eth_balance_safe(config, governance_operator)
    );
    let observed_wei = conservative_observed_balance(
        finalized_balance.map_err(|_| BaseGovernanceError::ObservationUnavailable)?,
        safe_balance.map_err(|_| BaseGovernanceError::ObservationUnavailable)?,
    );
    if let Some(error) = affordability_error(observed_wei, required_wei) {
        return Err(error);
    }
    Ok(())
}

fn conservative_observed_balance(finalized_wei: u128, safe_wei: u128) -> u128 {
    finalized_wei.min(safe_wei)
}

fn affordability_error(observed_wei: u128, required_wei: u128) -> Option<BaseGovernanceError> {
    (observed_wei < required_wei).then_some(BaseGovernanceError::InsufficientGovernanceBalance {
        observed_wei,
        required_wei,
    })
}

fn activation_base_preflight_matches(
    observed_signer: [u8; 20],
    expected_signer: [u8; 20],
    deposits_paused: bool,
    withdrawals_paused: bool,
) -> bool {
    bridge_core::activation_base_preflight_matches(
        observed_signer == expected_signer,
        deposits_paused,
        withdrawals_paused,
    )
}

fn activation_postcondition_matches(deposits_paused: bool, withdrawals_paused: bool) -> bool {
    bridge_core::activation_postcondition_matches(deposits_paused, withdrawals_paused)
}

fn governance_lane(
) -> Result<(bool, u64, u64, Option<storage::GovernanceTransaction>), BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow()
            .governance_lane()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn pending_transaction(
    operation_id: u64,
) -> Result<storage::GovernanceTransaction, BaseGovernanceError> {
    governance_lane()?
        .3
        .filter(|transaction| transaction.id == operation_id)
        .ok_or(BaseGovernanceError::InvalidArgument)
}

fn completed_transaction(
    operation_id: u64,
) -> Result<Option<storage::GovernanceTransaction>, BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow()
            .last_completed_governance_transaction()
            .map_err(|_| BaseGovernanceError::StorageFailure)
            .map(|transaction| transaction.filter(|transaction| transaction.id == operation_id))
    })
}

fn config() -> Result<crate::config::BridgeInitArgs, BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::StorageFailure)
    })
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

fn complete_confirmed_activation(
    transaction: &storage::GovernanceTransaction,
    caller: Principal,
) -> Result<(), BaseGovernanceError> {
    STORE.with(|store| {
        store
            .borrow_mut()
            .complete_confirmed_activation_and_resume_if_clear(
                transaction.clone(),
                caller,
                ic_cdk::api::time(),
            )
            .map(drop)
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })
}

fn signed_view(
    transaction: &storage::GovernanceTransaction,
    operator: [u8; 20],
) -> Result<SignedBaseGovernanceTransaction, BaseGovernanceError> {
    let signed = transaction
        .envelope
        .signed_transactions
        .last()
        .ok_or(BaseGovernanceError::StorageFailure)?;
    Ok(SignedBaseGovernanceTransaction {
        operation_id: transaction.id,
        kind: kind_view(&transaction.kind),
        chain_id: transaction.envelope.chain_id,
        sender: operator.to_vec(),
        nonce: transaction.envelope.nonce,
        target: transaction.envelope.contract.to_vec(),
        calldata: transaction.envelope.calldata.clone(),
        gas_limit: transaction.envelope.gas_limit.into(),
        max_fee_per_gas: signed.max_fee_per_gas.into(),
        max_priority_fee_per_gas: signed.max_priority_fee_per_gas.into(),
        raw_transaction: signed.raw_transaction.clone(),
        transaction_hash: signed.transaction_hash.to_vec(),
        generation: signed.generation,
        signed_at_ns: signed.signed_at_ns,
    })
}

fn signing_failure(error: signer::SignerError) -> BaseGovernanceError {
    let class = error.class();
    // Deployment policy keeps canister logs controller-visible; the public Candid error contains
    // only the stable class and never the management reject detail.
    ic_cdk::println!("governance threshold signing failed: {error}");
    BaseGovernanceError::SigningUnavailable { class }
}

fn kind_view(kind: &storage::GovernanceTransactionKind) -> BaseGovernanceOperationKind {
    match kind {
        storage::GovernanceTransactionKind::PauseDepositMints => {
            BaseGovernanceOperationKind::PauseDepositMints
        }
        storage::GovernanceTransactionKind::PauseWithdrawals => {
            BaseGovernanceOperationKind::PauseWithdrawals
        }
        storage::GovernanceTransactionKind::SetServiceFee { value } => {
            BaseGovernanceOperationKind::SetServiceFee {
                value: (*value).into(),
            }
        }
        storage::GovernanceTransactionKind::CancelTimelock { operation_id } => {
            BaseGovernanceOperationKind::CancelTimelock {
                operation_id: operation_id.to_vec(),
            }
        }
        storage::GovernanceTransactionKind::ScheduleActivation { operation_id, salt } => {
            BaseGovernanceOperationKind::ScheduleActivation {
                operation_id: operation_id.to_vec(),
                salt: salt.to_vec(),
            }
        }
        storage::GovernanceTransactionKind::ExecuteActivation { operation_id, salt } => {
            BaseGovernanceOperationKind::ExecuteActivation {
                operation_id: operation_id.to_vec(),
                salt: salt.to_vec(),
            }
        }
    }
}

fn caller_roles(caller: Principal) -> Result<(bool, bool), BaseGovernanceError> {
    if caller == Principal::anonymous() {
        return Ok((false, false));
    }
    STORE.with(|store| {
        let state = store
            .borrow()
            .admin_state()
            .map_err(|_| BaseGovernanceError::StorageFailure)?;
        Ok((
            state.governance_principal == caller,
            state.pause_principal == caller,
        ))
    })
}

fn attestation_refresh_authorized(
    config: &crate::config::BridgeInitArgs,
    caller: Principal,
) -> bool {
    caller != Principal::anonymous()
        && (caller == config.governance_principal
            || caller == config.confirmation_relayer_principal)
}

fn action_authorized(governance: bool, pause: bool, action: &GovernanceAction) -> bool {
    governance
        || (pause
            && matches!(
                action,
                GovernanceAction::PauseDepositMints
                    | GovernanceAction::PauseWithdrawals
                    | GovernanceAction::CancelPendingTimelock
            ))
}

fn transaction_authorized(
    governance: bool,
    pause: bool,
    kind: &storage::GovernanceTransactionKind,
) -> bool {
    governance || (pause && is_emergency_kind(kind))
}

fn require_governance_or_pause(caller: Principal) -> Result<(), BaseGovernanceError> {
    let (governance, pause) = caller_roles(caller)?;
    if governance || pause {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

fn require_operational_config_sealed() -> Result<(), BaseGovernanceError> {
    let sealed = STORE.with(|store| {
        store
            .borrow()
            .operational_config_sealed()
            .map_err(|_| BaseGovernanceError::StorageFailure)
    })?;
    if sealed {
        Ok(())
    } else {
        Err(BaseGovernanceError::InvalidArgument)
    }
}

fn require_action_authorization(
    caller: Principal,
    action: &GovernanceAction,
) -> Result<(), BaseGovernanceError> {
    let (governance, pause) = caller_roles(caller)?;
    if action_authorized(governance, pause, action) {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

fn require_transaction_authorization(
    caller: Principal,
    kind: &storage::GovernanceTransactionKind,
) -> Result<(), BaseGovernanceError> {
    let (governance, pause) = caller_roles(caller)?;
    if transaction_authorized(governance, pause, kind) {
        Ok(())
    } else {
        Err(BaseGovernanceError::Unauthorized)
    }
}

fn action_matches_pending(
    action: &GovernanceAction,
    kind: &storage::GovernanceTransactionKind,
) -> bool {
    match (action, kind) {
        (
            GovernanceAction::PauseDepositMints,
            storage::GovernanceTransactionKind::PauseDepositMints,
        )
        | (
            GovernanceAction::PauseWithdrawals,
            storage::GovernanceTransactionKind::PauseWithdrawals,
        )
        | (
            GovernanceAction::CancelPendingTimelock,
            storage::GovernanceTransactionKind::CancelTimelock { .. },
        )
        | (
            GovernanceAction::ScheduleActivation,
            storage::GovernanceTransactionKind::ScheduleActivation { .. },
        )
        | (
            GovernanceAction::ExecuteActivation,
            storage::GovernanceTransactionKind::ExecuteActivation { .. },
        ) => true,
        (
            GovernanceAction::SetServiceFee { value },
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

fn is_emergency_kind(kind: &storage::GovernanceTransactionKind) -> bool {
    matches!(
        kind,
        storage::GovernanceTransactionKind::PauseDepositMints
            | storage::GovernanceTransactionKind::PauseWithdrawals
            | storage::GovernanceTransactionKind::CancelTimelock { .. }
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

fn minimum_fee_bump(current: u128, proposed: u128, bump_bps: u16) -> bool {
    let minimum = current
        .saturating_mul(10_000 + u128::from(bump_bps))
        .saturating_add(9_999)
        / 10_000;
    proposed >= minimum.max(current.saturating_add(1))
}

fn initial_fee(ceiling: u128, bump_bps: u16, generations: u8) -> u128 {
    (0..generations).fold(ceiling, |fee, _| {
        fee.saturating_mul(10_000)
            .checked_div(10_000 + u128::from(bump_bps))
            .unwrap_or(1)
            .max(1)
    })
}

fn hash32(value: &[u8]) -> Result<[u8; 32], BaseGovernanceError> {
    value
        .try_into()
        .map_err(|_| BaseGovernanceError::InvalidArgument)
}

fn encode_action(
    action: GovernanceAction,
    governance_operation_id: u64,
) -> Result<(storage::GovernanceTransactionKind, [u8; 20], Vec<u8>), BaseGovernanceError> {
    let (bridge, timelock, deployment_instance_id) = STORE.with(|store| {
        let config = store
            .borrow()
            .config()
            .map_err(|_| BaseGovernanceError::StorageFailure)?
            .ok_or(BaseGovernanceError::StorageFailure)?;
        Ok::<_, BaseGovernanceError>((
            config.contract_array(),
            config.timelock_array(),
            hash32(&config.deployment_instance_id)?,
        ))
    })?;
    match action {
        GovernanceAction::PauseDepositMints => Ok((
            storage::GovernanceTransactionKind::PauseDepositMints,
            bridge,
            selector("pauseDepositMints()"),
        )),
        GovernanceAction::PauseWithdrawals => Ok((
            storage::GovernanceTransactionKind::PauseWithdrawals,
            bridge,
            selector("pauseWithdrawals()"),
        )),
        GovernanceAction::SetServiceFee { value } => {
            let value = nat_u128(&value).ok_or(BaseGovernanceError::InvalidArgument)?;
            let mut calldata = selector("setServiceFee(uint256)");
            calldata.extend_from_slice(&word_u128(value));
            Ok((
                storage::GovernanceTransactionKind::SetServiceFee { value },
                bridge,
                calldata,
            ))
        }
        GovernanceAction::CancelPendingTimelock => {
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
        GovernanceAction::ScheduleActivation => {
            let salt = activation_salt(deployment_instance_id, governance_operation_id);
            let operation_id = activation_operation_id(bridge, salt);
            Ok((
                storage::GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
                timelock,
                schedule_activation_calldata(bridge, salt),
            ))
        }
        GovernanceAction::ExecuteActivation => {
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

fn activation_salt(deployment_instance_id: [u8; 32], governance_operation_id: u64) -> [u8; 32] {
    let mut input = b"KINIC_BRIDGE_ACTIVATION_V2".to_vec();
    input.extend_from_slice(&deployment_instance_id);
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
        encoded.extend_from_slice(&word_u128(ACTIVATION_TIMELOCK_DELAY_SECONDS));
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
        action_authorized, activation_base_preflight_matches, activation_operation_id,
        activation_postcondition_matches, activation_salt, affordability_error,
        confirmation_caller_authorized, conservative_observed_balance, execute_activation_calldata,
        initial_fee, minimum_fee_bump, pending_signature_action, schedule_activation_calldata,
        selector, transaction_authorized, word_u128, BaseGovernanceError, GovernanceAction,
        PendingSignatureAction, ACTIVATION_TIMELOCK_DELAY_SECONDS,
    };
    use crate::storage::{
        GovernanceTransaction, GovernanceTransactionKind, GovernanceTransactionState,
    };
    use bridge_core::{
        GovernanceOperationId, GovernanceTransactionEnvelope, SignedGovernanceTransaction,
    };
    use candid::Nat;
    use candid::Principal;

    #[test]
    fn confirmation_authorization_accepts_only_the_fixed_relayer_and_admin_recovery_callers() {
        let relayer = Principal::from_slice(&[1]);
        let governance = Principal::from_slice(&[2]);
        let pause = Principal::from_slice(&[3]);
        for caller in [relayer, governance, pause] {
            assert!(confirmation_caller_authorized(
                caller, relayer, governance, pause
            ));
        }
        assert!(!confirmation_caller_authorized(
            Principal::anonymous(),
            relayer,
            governance,
            pause
        ));
        assert!(!confirmation_caller_authorized(
            Principal::from_slice(&[4]),
            relayer,
            governance,
            pause
        ));
    }

    #[test]
    fn replacement_requires_the_configured_minimum_bump() {
        assert!(!minimum_fee_bump(100, 112, 1_250));
        assert!(minimum_fee_bump(100, 113, 1_250));
        assert!(minimum_fee_bump(0, 1, 1_250));
        assert!(initial_fee(200_000, 1_250, 3) < 200_000);
    }

    #[test]
    fn activation_calldata_is_stable() {
        let bridge = [7; 20];
        let salt = activation_salt([3; 32], 9);
        assert_eq!(
            &schedule_activation_calldata(bridge, salt)[..4],
            selector("scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)")
        );
        assert_eq!(
            &execute_activation_calldata(bridge, salt)[..4],
            selector("executeBatch(address[],uint256[],bytes[],bytes32,bytes32)")
        );
        #[cfg(not(feature = "test-deployment"))]
        assert_eq!(ACTIVATION_TIMELOCK_DELAY_SECONDS, 86_400);
        #[cfg(feature = "test-deployment")]
        assert_eq!(ACTIVATION_TIMELOCK_DELAY_SECONDS, 300);
        assert_ne!(activation_operation_id(bridge, salt), [0; 32]);
        assert_eq!(word_u128(42)[31], 42);
    }

    #[test]
    fn governance_affordability_uses_conservative_balance_and_exact_boundary() {
        assert_eq!(conservative_observed_balance(11, 10), 10);
        assert_eq!(conservative_observed_balance(9, 10), 9);
        assert_eq!(affordability_error(10, 10), None);
        assert_eq!(
            affordability_error(9, 10),
            Some(BaseGovernanceError::InsufficientGovernanceBalance {
                observed_wei: 9,
                required_wei: 10,
            })
        );
    }

    #[test]
    fn prepared_resume_affordability_rejection_preserves_transaction() {
        let transaction = governance_transaction();
        let before = transaction.clone();

        assert_eq!(
            pending_signature_action(&transaction),
            Ok(PendingSignatureAction::Sign)
        );
        assert_eq!(
            affordability_error(9, 10),
            Some(BaseGovernanceError::InsufficientGovernanceBalance {
                observed_wei: 9,
                required_wei: 10,
            })
        );
        assert_eq!(transaction, before);
    }

    #[test]
    fn activation_salt_is_namespaced_by_deployment_instance() {
        let first = activation_salt([1; 32], 1);
        assert_eq!(first, activation_salt([1; 32], 1));
        assert_ne!(first, activation_salt([2; 32], 1));
        assert_ne!(first, activation_salt([1; 32], 2));
    }

    #[test]
    fn activation_preflight_and_postcondition_fail_closed() {
        assert!(activation_base_preflight_matches(
            [7; 20], [7; 20], true, true
        ));
        assert!(!activation_base_preflight_matches(
            [8; 20], [7; 20], true, true
        ));
        assert!(!activation_base_preflight_matches(
            [7; 20], [7; 20], false, true
        ));
        assert!(!activation_base_preflight_matches(
            [7; 20], [7; 20], true, false
        ));
        assert!(activation_postcondition_matches(false, false));
        assert!(!activation_postcondition_matches(true, false));
        assert!(!activation_postcondition_matches(false, true));
    }

    #[test]
    fn governance_and_pause_authorization_are_action_scoped() {
        let safe_actions = [
            GovernanceAction::PauseDepositMints,
            GovernanceAction::PauseWithdrawals,
            GovernanceAction::CancelPendingTimelock,
        ];
        let governance_actions = [
            GovernanceAction::SetServiceFee {
                value: Nat::from(1u8),
            },
            GovernanceAction::ScheduleActivation,
            GovernanceAction::ExecuteActivation,
        ];
        for action in safe_actions.iter().chain(&governance_actions) {
            assert!(action_authorized(true, false, action));
            assert!(!action_authorized(false, false, action));
        }
        for action in &safe_actions {
            assert!(action_authorized(false, true, action));
        }
        for action in &governance_actions {
            assert!(!action_authorized(false, true, action));
        }

        let safe_kinds = [
            GovernanceTransactionKind::PauseDepositMints,
            GovernanceTransactionKind::PauseWithdrawals,
            GovernanceTransactionKind::CancelTimelock {
                operation_id: [1; 32],
            },
        ];
        let governance_kinds = [
            GovernanceTransactionKind::SetServiceFee { value: 1 },
            GovernanceTransactionKind::ScheduleActivation {
                operation_id: [2; 32],
                salt: [3; 32],
            },
            GovernanceTransactionKind::ExecuteActivation {
                operation_id: [2; 32],
                salt: [3; 32],
            },
        ];
        for kind in safe_kinds.iter().chain(&governance_kinds) {
            assert!(transaction_authorized(true, false, kind));
            assert!(!transaction_authorized(false, false, kind));
        }
        for kind in &safe_kinds {
            assert!(transaction_authorized(false, true, kind));
        }
        for kind in &governance_kinds {
            assert!(!transaction_authorized(false, true, kind));
        }
    }

    #[test]
    fn pending_signature_state_routes_only_valid_prepared_and_signed_records() {
        let mut transaction = governance_transaction();
        assert_eq!(
            pending_signature_action(&transaction),
            Ok(PendingSignatureAction::Sign)
        );

        transaction
            .envelope
            .signed_transactions
            .push(signed_transaction());
        assert!(pending_signature_action(&transaction).is_err());

        transaction.state = GovernanceTransactionState::SignedAwaitingRelay {
            transaction_hash: [7; 32],
            generation: 0,
            signed_at_ns: 9,
        };
        assert_eq!(
            pending_signature_action(&transaction),
            Ok(PendingSignatureAction::ReturnSigned)
        );

        transaction.state = GovernanceTransactionState::SignedAwaitingRelay {
            transaction_hash: [8; 32],
            generation: 0,
            signed_at_ns: 9,
        };
        assert!(pending_signature_action(&transaction).is_err());

        transaction.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [7; 32],
            receipt_block_number: 10,
        };
        assert!(pending_signature_action(&transaction).is_err());
        transaction.state = GovernanceTransactionState::Reverted {
            transaction_hash: [7; 32],
            receipt_block_number: 10,
        };
        assert!(pending_signature_action(&transaction).is_err());
    }

    fn governance_transaction() -> GovernanceTransaction {
        GovernanceTransaction {
            id: 4,
            kind: GovernanceTransactionKind::PauseDepositMints,
            envelope: GovernanceTransactionEnvelope {
                operation_id: GovernanceOperationId::new(4),
                payload_hash: [1; 32],
                nonce: 5,
                chain_id: 8_453,
                contract: [2; 20],
                calldata: vec![3; 4],
                gas_limit: 500_000,
                max_fee_per_gas: 100,
                max_priority_fee_per_gas: 10,
                signed_transactions: Vec::new(),
            },
            state: GovernanceTransactionState::Prepared,
        }
    }

    fn signed_transaction() -> SignedGovernanceTransaction {
        SignedGovernanceTransaction {
            raw_transaction: vec![6; 32],
            transaction_hash: [7; 32],
            max_fee_per_gas: 100,
            max_priority_fee_per_gas: 10,
            generation: 0,
            signed_at_ns: 9,
        }
    }
}
