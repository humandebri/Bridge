use super::{
    DepositRecord, Deserialize, EvmOperationRecord, EvmOperationState, Serialize, SqlMap,
    StableBlob, StorageError,
};

pub(super) fn evm_state_tag(state: EvmOperationState) -> Option<u8> {
    match state {
        EvmOperationState::Queued => Some(0),
        EvmOperationState::Prepared => Some(1),
        EvmOperationState::Submitted { .. } => Some(2),
        EvmOperationState::Confirmed { .. }
        | EvmOperationState::Reverted { .. }
        | EvmOperationState::RecoveryPending { .. }
        | EvmOperationState::Recovered { .. } => None,
    }
}

pub(super) fn evm_state_index_key(
    value: &EvmOperationRecord,
) -> Result<Option<StableBlob>, StorageError> {
    let Some(tag) = evm_state_tag(value.state) else {
        return Ok(None);
    };
    let mut bytes = Vec::with_capacity(10);
    bytes.push(tag);
    bytes.push(0);
    bytes.extend_from_slice(&value.id.get().to_be_bytes());
    StableBlob::new(bytes).map(Some)
}

pub(super) fn first_evm_index_id(
    index: &SqlMap<StableBlob, u8>,
    tag: u8,
) -> Result<Option<u64>, StorageError> {
    let start = StableBlob::new(vec![tag])?;
    let end = StableBlob::new(vec![tag.saturating_add(1)])?;
    let Some(entry) = index.first_in_range(start..end) else {
        return Ok(None);
    };
    evm_index_id(entry.key()).map(Some)
}

pub(super) fn evm_index_id(key: &StableBlob) -> Result<u64, StorageError> {
    let bytes: [u8; 8] = key
        .as_slice()
        .get(2..10)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::from_be_bytes(bytes))
}

pub(super) fn deposit_operation_id(value: &DepositRecord) -> Option<u64> {
    match value.state {
        bridge_core::DepositState::MintPending { operation_id, .. } => Some(operation_id.get()),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum OperationOwner {
    Deposit([u8; 32]),
}

pub(super) fn is_pending_evm(value: &EvmOperationRecord) -> bool {
    !matches!(
        value.state,
        EvmOperationState::Confirmed { .. }
            | EvmOperationState::Reverted { .. }
            | EvmOperationState::RecoveryPending { .. }
            | EvmOperationState::Recovered { .. }
    )
}

pub(super) fn is_reverted_evm(value: &EvmOperationRecord) -> bool {
    matches!(
        value.state,
        EvmOperationState::Reverted { .. } | EvmOperationState::RecoveryPending { .. }
    )
}
