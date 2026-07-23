use crate::{
    evm_calls, evm_rpc,
    phases::{DepositPhase, SettlementState},
    storage::{AuditEventKind, DepositRecoveryAdmission, DepositReserveAdmission, RpcAuditBatch},
    STORE,
};
use bridge_core::{
    DepositEvent, DepositRecord, DepositState, EvmOperationEvent, EvmOperationId, EvmOperationKind,
    EvmOperationRecord, EvmOperationState, FinalizedObservationRecord,
};
use candid::{CandidType, Deserialize, Principal};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecoverMintRevertArgs {
    pub deposit_id: Vec<u8>,
    pub reverted_operation_id: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum RecoverMintRevertReceipt {
    Enqueued {
        replacement_operation_id: u64,
        state: SettlementState,
        finalized_block_number: u64,
        finalized_block_hash: Vec<u8>,
    },
    AlreadyStarted {
        replacement_operation_id: u64,
    },
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverMintRevertError {
    AnonymousCaller,
    Unauthorized,
    Busy,
    InvalidId,
    NotFound,
    OperationMismatch,
    NotReverted,
    RpcUnavailable,
    RpcInconsistent,
    BaseStateMismatch,
    BridgeSignerMismatch,
    ReserveUnavailable,
    MintWindowUnavailable,
    StorageFailure,
}

#[derive(Clone)]
pub(crate) enum ValidatedTarget {
    Deposit([u8; 32]),
}

impl ValidatedTarget {
    pub fn id(&self) -> [u8; 32] {
        match self {
            Self::Deposit(id) => *id,
        }
    }

    fn rpc_target(&self) -> evm_rpc::RecoveryTarget {
        match self {
            Self::Deposit(id) => evm_rpc::RecoveryTarget::Deposit(*id),
        }
    }
}

pub(crate) fn validate_target(
    deposit_id: &[u8],
) -> Result<ValidatedTarget, RecoverMintRevertError> {
    deposit_id
        .try_into()
        .map(ValidatedTarget::Deposit)
        .map_err(|_| RecoverMintRevertError::InvalidId)
}

enum Parent {
    Deposit(DepositRecord),
}

struct Preflight {
    target: ValidatedTarget,
    parent: Parent,
    reverted: EvmOperationRecord,
}

fn current_operation_id(parent: &Parent) -> Option<EvmOperationId> {
    match parent {
        Parent::Deposit(record) => match record.state {
            DepositState::MintReverted { operation_id, .. }
            | DepositState::MintPending { operation_id, .. }
            | DepositState::Minted { operation_id, .. } => Some(operation_id),
            _ => None,
        },
    }
}

fn preflight(
    args: &RecoverMintRevertArgs,
    target: ValidatedTarget,
) -> Result<Preflight, RecoverMintRevertError> {
    STORE.with(|store| {
        let store = store.borrow();
        let parent = match target {
            ValidatedTarget::Deposit(id) => Parent::Deposit(
                store
                    .deposit(id)
                    .map_err(|_| RecoverMintRevertError::StorageFailure)?
                    .ok_or(RecoverMintRevertError::NotFound)?,
            ),
        };
        let reverted = store
            .evm_operation(args.reverted_operation_id)
            .map_err(|_| RecoverMintRevertError::StorageFailure)?
            .ok_or(RecoverMintRevertError::NotFound)?;
        let target_matches = matches!(
            (&parent, reverted.kind),
            (Parent::Deposit(_), EvmOperationKind::MintDeposit)
        );
        if !target_matches {
            return Err(RecoverMintRevertError::OperationMismatch);
        }
        match reverted.state {
            EvmOperationState::RecoveryPending { .. } => {
                return Err(RecoverMintRevertError::Busy);
            }
            EvmOperationState::Reverted { .. } => {}
            _ => return Err(RecoverMintRevertError::NotReverted),
        }
        if current_operation_id(&parent) != Some(reverted.id) {
            return Err(RecoverMintRevertError::OperationMismatch);
        }
        Ok(Preflight {
            target,
            parent,
            reverted,
        })
    })
}

fn already_started(
    target: &ValidatedTarget,
    reverted_operation_id: u64,
) -> Result<Option<u64>, RecoverMintRevertError> {
    STORE.with(|store| {
        let store = store.borrow();
        let parent = match target {
            ValidatedTarget::Deposit(id) => store
                .deposit(*id)
                .map_err(|_| RecoverMintRevertError::StorageFailure)?
                .map(Parent::Deposit),
        };
        let Some(parent) = parent else {
            return Ok(None);
        };
        let Some(operation) = store
            .evm_operation(reverted_operation_id)
            .map_err(|_| RecoverMintRevertError::StorageFailure)?
        else {
            return Ok(None);
        };
        let EvmOperationState::RecoveryPending {
            replacement_operation_id,
            ..
        } = operation.state
        else {
            return Ok(None);
        };
        let kind_matches = matches!(
            (&parent, operation.kind),
            (Parent::Deposit(_), EvmOperationKind::MintDeposit)
        );
        if !kind_matches || current_operation_id(&parent) != Some(replacement_operation_id) {
            return Err(RecoverMintRevertError::OperationMismatch);
        }
        Ok(Some(replacement_operation_id.get()))
    })
}

fn rpc_error(error: evm_rpc::ObservationError) -> RecoverMintRevertError {
    match error {
        evm_rpc::ObservationError::Inconsistent => RecoverMintRevertError::RpcInconsistent,
        evm_rpc::ObservationError::Rpc => RecoverMintRevertError::RpcUnavailable,
        _ => RecoverMintRevertError::BaseStateMismatch,
    }
}

fn rpc_audit(observation: &evm_rpc::RecoveryObservation) -> AuditEventKind {
    let evidence = &observation.rpc_audit;
    AuditEventKind::EvmRpcObservation {
        evm_rpc_canister_id: evidence.evm_rpc_canister_id,
        call_method: evidence.call_method.clone(),
        request_digest: evidence.request_digest.to_vec(),
        quorum_response_digest: evidence.quorum_response_digest.to_vec(),
        finalized_block_number: evidence.finalized_block_number,
        finalized_block_hash: evidence.finalized_block_hash.to_vec(),
        transaction_hash: None,
    }
}

fn recovery_audit_started(
    preflight: &Preflight,
    replacement_id: EvmOperationId,
    observation: &evm_rpc::RecoveryObservation,
) -> AuditEventKind {
    AuditEventKind::MintRevertRecoveryStarted {
        target_id: preflight.target.id().to_vec(),
        reverted_operation_id: preflight.reverted.id.get(),
        replacement_operation_id: replacement_id.get(),
        kind: preflight.reverted.kind.into(),
        finalized_block_number: observation.finalized.block_number,
        finalized_block_hash: observation.finalized.block_hash.to_vec(),
        result: "replacement_enqueued".into(),
    }
}

pub(crate) async fn recover(
    caller: Principal,
    args: RecoverMintRevertArgs,
) -> Result<RecoverMintRevertReceipt, RecoverMintRevertError> {
    if caller == Principal::anonymous() {
        return Err(RecoverMintRevertError::AnonymousCaller);
    }
    if !crate::admin::is_governance(caller).map_err(|_| RecoverMintRevertError::StorageFailure)? {
        return Err(RecoverMintRevertError::Unauthorized);
    }
    let target = validate_target(&args.deposit_id)?;

    if let Some(replacement_operation_id) = already_started(&target, args.reverted_operation_id)? {
        return Ok(RecoverMintRevertReceipt::AlreadyStarted {
            replacement_operation_id,
        });
    }

    let preflight = preflight(&args, target)?;
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| RecoverMintRevertError::StorageFailure)?
            .ok_or(RecoverMintRevertError::StorageFailure)
    })?;
    let observation = evm_rpc::recovery_observation(&config, preflight.target.rpc_target())
        .await
        .map_err(rpc_error)?;
    let expected_signer = STORE.with(|store| {
        store
            .borrow()
            .signer_address()
            .map_err(|_| RecoverMintRevertError::StorageFailure)?
            .ok_or(RecoverMintRevertError::StorageFailure)
    })?;
    if observation.snapshot.bridge_signer != expected_signer
        || observation.bridge_identity.signer != expected_signer
    {
        return Err(RecoverMintRevertError::BridgeSignerMismatch);
    }
    match (&preflight.parent, &observation.state) {
        (Parent::Deposit(record), evm_rpc::RecoveryBaseState::DepositProcessed(processed)) => {
            if *processed {
                return Err(RecoverMintRevertError::BaseStateMismatch);
            }
            recover_mint(caller, &config, &preflight, record, &observation).await
        }
    }
}

async fn recover_mint(
    caller: Principal,
    config: &crate::config::BridgeInitArgs,
    preflight: &Preflight,
    record: &DepositRecord,
    observation: &evm_rpc::RecoveryObservation,
) -> Result<RecoverMintRevertReceipt, RecoverMintRevertError> {
    if observation.snapshot.deposits_paused {
        return Err(RecoverMintRevertError::MintWindowUnavailable);
    }
    let quote = record.quote.ok_or(RecoverMintRevertError::StorageFailure)?;
    let mint = observation.snapshot.mint;
    if quote.net_amount > mint.per_deposit_limit
        || quote.service_fee > mint.max_service_fee
        || quote.service_fee > record.max_service_fee
    {
        return Err(RecoverMintRevertError::MintWindowUnavailable);
    }
    let (counters, nonterminal_withdrawals, expected_token) = STORE.with(|store| {
        let store = store.borrow();
        Ok::<_, RecoverMintRevertError>((
            store
                .counters()
                .map_err(|_| RecoverMintRevertError::StorageFailure)?,
            store
                .nonterminal_withdrawal_count()
                .map_err(|_| RecoverMintRevertError::StorageFailure)?,
            store
                .deposit_reserve_token()
                .map_err(|_| RecoverMintRevertError::StorageFailure)?,
        ))
    })?;
    let consumed = bridge_core::mint_admission_total(
        mint.effective_minted_in_window().get(),
        counters.reserved_deposit_mint_amount,
        quote.net_amount.get(),
    )
    .ok_or(RecoverMintRevertError::MintWindowUnavailable)?;
    if consumed > mint.mint_window_limit.get() {
        return Err(RecoverMintRevertError::MintWindowUnavailable);
    }
    let finalized_eth_balance = evm_rpc::signer_eth_balance_at(
        config,
        observation.bridge_identity.signer,
        observation.finalized,
    )
    .await
    .map_err(rpc_error)?;
    let safe_eth_balance = evm_rpc::signer_eth_balance(config, observation.bridge_identity.signer)
        .await
        .map_err(rpc_error)?;
    let eth_balance = finalized_eth_balance.min(safe_eth_balance);
    let reserve = config
        .reserve_policy()
        .snapshot(
            nonterminal_withdrawals,
            counters.reserved_deposit_mint_operations,
            1,
            eth_balance,
            ic_cdk::api::canister_liquid_cycle_balance(),
        )
        .map_err(|_| RecoverMintRevertError::ReserveUnavailable)?;
    if !reserve.sufficient {
        return Err(RecoverMintRevertError::ReserveUnavailable);
    }
    let admission = DepositRecoveryAdmission {
        reserve: DepositReserveAdmission {
            audit_caller: caller,
            expected_token,
            observed_at_ns: ic_cdk::api::time(),
            eth_balance_wei: eth_balance,
            cycles_balance: ic_cdk::api::canister_liquid_cycle_balance(),
            reserve_policy: config.reserve_policy(),
            mint_snapshot: mint,
        },
        finalized_observation: FinalizedObservationRecord {
            chain_id: observation.finalized.chain_id,
            block_number: observation.finalized.block_number,
            block_hash: observation.finalized.block_hash,
            observed_at_ns: observation.finalized.observed_at_ns,
            bridge_signer: observation.bridge_identity.signer,
            runtime_sha256: observation.bridge_identity.runtime_sha256,
        },
    };
    let intent_record = STORE.with(|store| {
        store
            .borrow()
            .deposit_intent(record.id.bytes())
            .map_err(|_| RecoverMintRevertError::StorageFailure)?
            .ok_or(RecoverMintRevertError::StorageFailure)
    })?;
    let replacement_id = STORE.with(|store| {
        store
            .borrow()
            .next_evm_operation_id()
            .map_err(|_| RecoverMintRevertError::StorageFailure)
    })?;
    let mut next = record.clone();
    next.apply(DepositEvent::RetryMint {
        reverted_operation_id: preflight.reverted.id,
        replacement_operation_id: replacement_id,
    })
    .map_err(|_| RecoverMintRevertError::OperationMismatch)?;
    let mut old_next = preflight.reverted;
    old_next
        .apply(EvmOperationEvent::StartRecovery {
            replacement_operation_id: replacement_id,
        })
        .map_err(|_| RecoverMintRevertError::OperationMismatch)?;
    let replacement = EvmOperationRecord::queued_recovery(
        replacement_id,
        record.payload_hash,
        EvmOperationKind::MintDeposit,
        preflight.reverted.id,
    );
    let intent = evm_calls::mint_deposit(
        config,
        replacement_id,
        record.payload_hash,
        evm_calls::MintDepositArgs {
            deposit_id: record.id.bytes(),
            recipient: intent_record.base_recipient,
            gross_amount: record.gross_amount.get(),
            max_service_fee: record.max_service_fee.get(),
            charged_service_fee: quote.service_fee.get(),
        },
    );
    STORE.with(|store| {
        store
            .borrow_mut()
            .commit_deposit_recovery_bundle(
                record,
                &next,
                &preflight.reverted,
                &old_next,
                &replacement,
                &intent,
                admission,
                RpcAuditBatch {
                    caller,
                    timestamp_ns: ic_cdk::api::time(),
                    kinds: vec![
                        rpc_audit(observation),
                        recovery_audit_started(preflight, replacement_id, observation),
                    ],
                },
            )
            .map_err(|error| match error {
                crate::storage::StorageError::ReserveUnavailable
                | crate::storage::StorageError::StaleReserveObservation => {
                    RecoverMintRevertError::ReserveUnavailable
                }
                crate::storage::StorageError::Core(
                    bridge_core::CoreError::MintWindowLimitExceeded,
                ) => RecoverMintRevertError::MintWindowUnavailable,
                crate::storage::StorageError::Core(
                    bridge_core::CoreError::StaleFinalizedObservation
                    | bridge_core::CoreError::ConflictingFinalizedObservation,
                ) => RecoverMintRevertError::BaseStateMismatch,
                _ => RecoverMintRevertError::StorageFailure,
            })
    })?;
    Ok(RecoverMintRevertReceipt::Enqueued {
        replacement_operation_id: replacement_id.get(),
        state: SettlementState::Deposit(DepositPhase::from(&next.state)),
        finalized_block_number: observation.finalized.block_number,
        finalized_block_hash: observation.finalized.block_hash.to_vec(),
    })
}
