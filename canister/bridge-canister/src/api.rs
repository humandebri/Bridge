use crate::{config::BridgeInitArgs, evm_rpc, ledger, storage::DepositIntent, STORE};
use bridge_core::{
    Account, Amount, DepositEvent, DepositId, DepositRecord, DepositRequest, DepositState,
    EvmCallIntent, EvmOperationId, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    LedgerCallOutcome, LedgerFailure, LedgerOperation, LedgerTransferIdentity,
    ReconciliationHoldRecord, RequestReference, SafeReceiptOutcome, WithdrawalState,
};
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::Memory;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositArgs {
    pub client_request_id: Vec<u8>,
    pub base_recipient: Vec<u8>,
    pub gross_amount: Nat,
    pub max_service_fee: Nat,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositReceipt {
    pub deposit_id: Vec<u8>,
    pub state: String,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DepositError {
    InvalidRequest(String),
    BaseObservationUnavailable,
    LedgerFeeUnavailable,
    Rejected(String),
    StorageFailure,
    DepositsPaused,
    ReserveUnavailable,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositView {
    pub deposit_id: Vec<u8>,
    pub gross_amount: Nat,
    pub net_amount: Nat,
    pub service_fee: Nat,
    pub base_recipient: Vec<u8>,
    pub state: String,
    pub base_confirmation: Option<BaseConfirmationView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalView {
    pub withdrawal_id: Vec<u8>,
    pub amount: Nat,
    pub min_amount_out: Nat,
    pub state: String,
    pub base_confirmation: Option<BaseConfirmationView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseConfirmationView {
    Submitted {
        transaction_hash: Vec<u8>,
    },
    SafeSucceeded {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
    SafeReverted {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
    Finalized {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
    Reverted {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
}

enum AdmissionOutcome {
    Inserted,
    Existing,
    StaleObservation,
}

fn nat_u128(value: &Nat) -> Result<u128, DepositError> {
    value
        .0
        .to_string()
        .parse()
        .map_err(|_| DepositError::InvalidRequest("amount exceeds u128".into()))
}

fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

fn state_name(state: &DepositState) -> String {
    match state {
        DepositState::PullPending => "PullPending",
        DepositState::Escrowed { .. } => "Escrowed",
        DepositState::MintPending { .. } => "MintPending",
        DepositState::Minted { .. } => "Minted",
        DepositState::MintReverted { .. } => "MintReverted",
        DepositState::ReconciliationHold { .. } => "ReconciliationHold",
        DepositState::Cancelled { .. } => "Cancelled",
    }
    .into()
}

fn config() -> Result<BridgeInitArgs, DepositError> {
    STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| DepositError::StorageFailure)?
            .ok_or(DepositError::StorageFailure)
    })
}

pub async fn request_deposit(
    caller: Principal,
    args: DepositArgs,
) -> Result<DepositReceipt, DepositError> {
    let client_request_id: [u8; 32] =
        args.client_request_id.as_slice().try_into().map_err(|_| {
            DepositError::InvalidRequest("client_request_id must be 32 bytes".into())
        })?;
    let base_recipient: [u8; 20] = args
        .base_recipient
        .as_slice()
        .try_into()
        .map_err(|_| DepositError::InvalidRequest("base_recipient must be 20 bytes".into()))?;
    if base_recipient == [0; 20] || caller == Principal::anonymous() {
        return Err(DepositError::InvalidRequest(
            "anonymous caller or zero recipient".into(),
        ));
    }
    let gross_amount = nat_u128(&args.gross_amount)?;
    let max_service_fee = nat_u128(&args.max_service_fee)?;
    let deposit_id = hash(&[caller.as_slice(), &client_request_id]);
    let payload_hash = hash(&[
        caller.as_slice(),
        &client_request_id,
        &base_recipient,
        &gross_amount.to_be_bytes(),
        &max_service_fee.to_be_bytes(),
    ]);

    if let Some(receipt) = existing_receipt(deposit_id, payload_hash)? {
        return Ok(receipt);
    }

    let config = config()?;
    let deposits_paused = STORE.with(|store| {
        store
            .borrow()
            .admin_state()
            .map(|state| state.deposits_paused)
            .unwrap_or(true)
    });
    if deposits_paused {
        return Err(DepositError::DepositsPaused);
    }
    ensure_reserve(&config).await?;
    crate::tasks::ensure_nonce_initialized(&config)
        .await
        .map_err(|error| match error {
            crate::tasks::NonceInitializationError::Observation => {
                DepositError::BaseObservationUnavailable
            }
            crate::tasks::NonceInitializationError::Storage => DepositError::StorageFailure,
        })?;
    let ledger_fee = ledger::ledger_fee(config.ledger_canister_id)
        .await
        .map_err(|_| DepositError::LedgerFeeUnavailable)?;
    let now = ic_cdk::api::time();
    let memo = hash(&[b"KINIC-DEPOSIT", &deposit_id]);
    let canister = ic_cdk::api::canister_self();
    let transfer = LedgerTransferIdentity {
        operation: LedgerOperation::PullDeposit,
        created_at_time_ns: now,
        memo,
        amount: Amount::new(gross_amount),
        fee: ledger_fee,
        from: Account::new(caller.as_slice().to_vec(), [0; 32])
            .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        to: Account::new(canister.as_slice().to_vec(), [0; 32])
            .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        spender: Some(
            Account::new(canister.as_slice().to_vec(), [0; 32])
                .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        ),
    };
    let intent = DepositIntent {
        deposit_id,
        caller: caller.as_slice().to_vec(),
        client_request_id,
        base_recipient,
        payload_hash,
    };
    let mut admission = AdmissionOutcome::StaleObservation;
    for _ in 0..3 {
        let snapshot = evm_rpc::base_mint_snapshot(&config)
            .await
            .map_err(|_| DepositError::BaseObservationUnavailable)?;
        let record = DepositRecord::accept(
            DepositRequest {
                id: DepositId::new(deposit_id),
                payload_hash,
                gross_amount: Amount::new(gross_amount),
                user_max_service_fee: Amount::new(max_service_fee),
                transfer: transfer.clone(),
            },
            snapshot,
        )
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
        admission = STORE.with(|store| {
            let mut store = store.borrow_mut();
            if let Some(existing) = store
                .deposit(deposit_id)
                .map_err(|_| DepositError::StorageFailure)?
            {
                existing.verify_retry(payload_hash).map_err(|_| {
                    DepositError::InvalidRequest(
                        "client request id conflicts with an existing payload".into(),
                    )
                })?;
                return Ok(AdmissionOutcome::Existing);
            }
            if store
                .admin_state()
                .map_err(|_| DepositError::StorageFailure)?
                .deposits_paused
            {
                return Err(DepositError::DepositsPaused);
            }
            let progress = store
                .external_progress()
                .map_err(|_| DepositError::StorageFailure)?;
            if snapshot.finalized_block_number < progress.last_finalized_mint_block {
                return Ok(AdmissionOutcome::StaleObservation);
            }
            let reserved = store
                .counters()
                .map_err(|_| DepositError::StorageFailure)?
                .reserved_deposit_mint_amount;
            let total = bridge_core::mint_admission_total(
                snapshot.effective_minted_in_window().get(),
                reserved,
                record.net_amount.get(),
            )
            .ok_or_else(|| DepositError::Rejected("mint admission arithmetic overflow".into()))?;
            if total > snapshot.mint_window_limit.get() {
                return Err(DepositError::Rejected("MintWindowLimitExceeded".into()));
            }
            store
                .put_deposit_intent(&intent)
                .map_err(|_| DepositError::StorageFailure)?;
            store
                .put_deposit(&record)
                .map_err(|_| DepositError::StorageFailure)?;
            Ok(AdmissionOutcome::Inserted)
        })?;
        if !matches!(admission, AdmissionOutcome::StaleObservation) {
            break;
        }
    }
    if matches!(admission, AdmissionOutcome::StaleObservation) {
        return Err(DepositError::BaseObservationUnavailable);
    }
    if matches!(admission, AdmissionOutcome::Existing) {
        return existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure);
    }

    match ledger::pull(config.ledger_canister_id, &transfer).await {
        LedgerCallOutcome::Succeeded { block_index }
        | LedgerCallOutcome::Duplicate { block_index } => {
            prepare_mint(deposit_id, block_index, base_recipient, &config)?;
        }
        LedgerCallOutcome::Ambiguous => {
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let hold_id = store
                    .allocate_hold_id()
                    .map_err(|_| DepositError::StorageFailure)?;
                let mut deposit = store
                    .deposit(deposit_id)
                    .map_err(|_| DepositError::StorageFailure)?
                    .ok_or(DepositError::StorageFailure)?;
                deposit
                    .apply(DepositEvent::PullAmbiguous { hold_id })
                    .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Deposit(deposit.id),
                    transfer,
                );
                store
                    .put_deposit(&deposit)
                    .map_err(|_| DepositError::StorageFailure)?;
                store
                    .put_open_reconciliation_hold(&hold)
                    .map_err(|_| DepositError::StorageFailure)
            })?;
        }
        LedgerCallOutcome::DefinitiveFailure { code } => {
            STORE
                .with(|store| cancel_deposit_in_store(&mut store.borrow_mut(), deposit_id, code))?;
            return Err(DepositError::Rejected(format!("{code:?}")));
        }
        LedgerCallOutcome::RetryableFailure { .. } => {}
    }
    existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure)
}

pub(crate) fn cancel_deposit_in_store(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    deposit_id: [u8; 32],
    code: LedgerFailure,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    deposit
        .apply(DepositEvent::PullFailed { code })
        .map_err(|error| DepositError::Rejected(format!("{error:?}")))?;
    store
        .put_deposit(&deposit)
        .map_err(|_| DepositError::StorageFailure)
}

async fn ensure_reserve(config: &BridgeInitArgs) -> Result<(), DepositError> {
    let address = crate::signer::ethereum_address(config)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let eth_balance = evm_rpc::signer_eth_balance(config, address)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let withdrawals = STORE.with(|store| {
        store
            .borrow()
            .nonterminal_withdrawal_count()
            .map_err(|_| DepositError::StorageFailure)
    })?;
    let snapshot = config
        .reserve_policy()
        .snapshot(
            withdrawals,
            eth_balance,
            ic_cdk::api::canister_liquid_cycle_balance(),
        )
        .map_err(|_| DepositError::ReserveUnavailable)?;
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut progress = store
            .external_progress()
            .map_err(|_| DepositError::StorageFailure)?;
        let changed = progress.reserve_sufficient != snapshot.sufficient;
        progress.last_eth_balance_wei = eth_balance;
        progress.reserve_sufficient = snapshot.sufficient;
        progress.last_reserve_observation_ns = ic_cdk::api::time();
        store
            .set_external_progress(&progress)
            .map_err(|_| DepositError::StorageFailure)?;
        if changed {
            store
                .append_audit_event(
                    ic_cdk::api::canister_self(),
                    crate::storage::AuditEventKind::ReserveGateChanged {
                        sufficient: snapshot.sufficient,
                    },
                )
                .map_err(|_| DepositError::StorageFailure)?;
        }
        Ok(())
    })?;
    snapshot
        .sufficient
        .then_some(())
        .ok_or(DepositError::ReserveUnavailable)
}

pub(crate) fn prepare_mint(
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
) -> Result<(), DepositError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        prepare_mint_in_store(&mut store, deposit_id, block_index, recipient, config)
    })
}

pub(crate) fn prepare_mint_in_store(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    if matches!(
        deposit.state,
        DepositState::MintPending { .. } | DepositState::Minted { .. }
    ) {
        return Ok(());
    }
    deposit
        .apply(DepositEvent::PullSucceeded {
            ledger_block_index: block_index,
        })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let operation_id = store
        .allocate_evm_operation_id()
        .map_err(|_| DepositError::StorageFailure)?;
    deposit
        .apply(DepositEvent::PrepareMint { operation_id })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let operation = EvmOperationRecord::queued(
        operation_id,
        deposit.payload_hash,
        EvmOperationKind::MintDeposit,
    );
    let intent = EvmCallIntent {
        operation_id,
        payload_hash: deposit.payload_hash,
        chain_id: config.base_chain_id,
        contract: config.contract_array(),
        calldata: mint_calldata(
            deposit_id,
            recipient,
            deposit.gross_amount.get(),
            deposit.max_service_fee.get(),
            deposit.service_fee.get(),
        ),
        gas_limit: config.transaction_gas_limit,
        max_fee_per_gas: config.max_fee_per_gas,
        max_priority_fee_per_gas: config.max_priority_fee_per_gas,
    };
    store
        .put_evm_call_intent(&intent)
        .map_err(|_| DepositError::StorageFailure)?;
    store
        .put_evm_operation(&operation)
        .map_err(|_| DepositError::StorageFailure)?;
    store
        .put_deposit(&deposit)
        .map_err(|_| DepositError::StorageFailure)?;
    Ok(())
}

fn mint_calldata(
    deposit_id: [u8; 32],
    recipient: [u8; 20],
    gross: u128,
    max_fee: u128,
    charged_fee: u128,
) -> Vec<u8> {
    let mut selector_hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(b"mintDeposit((bytes32,address,uint256,uint256,uint256))");
    keccak.finalize(&mut selector_hash);
    let mut data = selector_hash[..4].to_vec();
    data.extend_from_slice(&deposit_id);
    data.extend_from_slice(&[0; 12]);
    data.extend_from_slice(&recipient);
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&gross.to_be_bytes());
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&max_fee.to_be_bytes());
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&charged_fee.to_be_bytes());
    data
}

fn existing_receipt(
    id: [u8; 32],
    payload_hash: [u8; 32],
) -> Result<Option<DepositReceipt>, DepositError> {
    STORE.with(|store| {
        let store = store.borrow();
        let Some(record) = store
            .deposit(id)
            .map_err(|_| DepositError::StorageFailure)?
        else {
            return Ok(None);
        };
        record.verify_retry(payload_hash).map_err(|_| {
            DepositError::InvalidRequest(
                "client request id conflicts with an existing payload".into(),
            )
        })?;
        Ok(Some(DepositReceipt {
            deposit_id: id.to_vec(),
            state: state_name(&record.state),
        }))
    })
}

pub fn get_deposit(id: Vec<u8>) -> Option<DepositView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let store = store.borrow();
        let record = store.deposit(id).ok()??;
        let intent = store.deposit_intent(id).ok()??;
        let operation_id = match &record.state {
            DepositState::MintPending { operation_id, .. }
            | DepositState::Minted { operation_id, .. }
            | DepositState::MintReverted { operation_id, .. } => Some(*operation_id),
            _ => None,
        };
        Some(DepositView {
            deposit_id: id.to_vec(),
            gross_amount: Nat::from(record.gross_amount.get()),
            net_amount: Nat::from(record.net_amount.get()),
            service_fee: Nat::from(record.service_fee.get()),
            base_recipient: intent.base_recipient.to_vec(),
            state: state_name(&record.state),
            base_confirmation: base_confirmation(&store, operation_id),
        })
    })
}

pub fn get_withdrawal(id: Vec<u8>) -> Option<WithdrawalView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let record = store.borrow().withdrawal(id).ok()??;
        let operation_id = match &record.state {
            WithdrawalState::AcknowledgePending { operation_id, .. }
            | WithdrawalState::AcknowledgeReverted { operation_id, .. }
            | WithdrawalState::Released { operation_id, .. }
            | WithdrawalState::RefundPending { operation_id, .. }
            | WithdrawalState::RefundReverted { operation_id, .. }
            | WithdrawalState::Refunded { operation_id } => Some(*operation_id),
            _ => None,
        };
        let state = match record.state {
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
        };
        Some(WithdrawalView {
            withdrawal_id: id.to_vec(),
            amount: Nat::from(record.amount.get()),
            min_amount_out: Nat::from(record.min_amount_out.get()),
            state: state.into(),
            base_confirmation: base_confirmation(&store.borrow(), operation_id),
        })
    })
}

fn base_confirmation<M: Memory>(
    store: &crate::storage::StableStore<M>,
    operation_id: Option<EvmOperationId>,
) -> Option<BaseConfirmationView> {
    let operation = store.evm_operation(operation_id?.get()).ok()??;
    let observed_head = store
        .external_progress()
        .map(|progress| progress.last_finalized_base_block)
        .unwrap_or_default();
    match operation.state {
        EvmOperationState::Queued | EvmOperationState::Prepared => None,
        EvmOperationState::Submitted { transaction_hash } => {
            match store
                .evm_safe_observation(operation.id.get())
                .ok()
                .flatten()
            {
                Some(observation) => {
                    let common = (
                        observation.transaction_hash.to_vec(),
                        observation.receipt_block_number,
                        observation.safe_block_number,
                    );
                    Some(match observation.outcome {
                        SafeReceiptOutcome::Succeeded => BaseConfirmationView::SafeSucceeded {
                            transaction_hash: common.0,
                            receipt_block_number: common.1,
                            observed_head: common.2,
                        },
                        SafeReceiptOutcome::Reverted => BaseConfirmationView::SafeReverted {
                            transaction_hash: common.0,
                            receipt_block_number: common.1,
                            observed_head: common.2,
                        },
                    })
                }
                None => Some(BaseConfirmationView::Submitted {
                    transaction_hash: transaction_hash.to_vec(),
                }),
            }
        }
        EvmOperationState::Finalized {
            transaction_hash,
            finalized_block_number,
        } => Some(BaseConfirmationView::Finalized {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number: finalized_block_number,
            observed_head,
        }),
        EvmOperationState::Reverted {
            transaction_hash,
            finalized_block_number,
        } => Some(BaseConfirmationView::Reverted {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number: finalized_block_number,
            observed_head,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_identity_binds_caller_and_client_id_and_calldata_is_static() {
        assert_ne!(hash(&[b"a", &[1; 32]]), hash(&[b"b", &[1; 32]]));
        let calldata = mint_calldata([1; 32], [2; 20], 3, 4, 5);
        assert_eq!(calldata.len(), 4 + 32 * 5);
        assert_eq!(&calldata[..4], &[0x84, 0xc7, 0x27, 0xfe]);
        assert_eq!(&calldata[4..36], &[1; 32]);
        assert_eq!(&calldata[36..48], &[0; 12]);
        assert_eq!(&calldata[48..68], &[2; 20]);
        assert_eq!(&calldata[68..84], &[0; 16]);
        assert_eq!(&calldata[84..100], &3u128.to_be_bytes());
        assert_eq!(&calldata[100..116], &[0; 16]);
        assert_eq!(&calldata[116..132], &4u128.to_be_bytes());
        assert_eq!(&calldata[132..148], &[0; 16]);
        assert_eq!(&calldata[148..164], &5u128.to_be_bytes());
    }
}
