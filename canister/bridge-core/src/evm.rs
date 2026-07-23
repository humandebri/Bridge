use crate::{ApplyOutcome, CoreError, EvmOperationId};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationKind {
    MintDeposit,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationState {
    Queued,
    Prepared,
    Submitted {
        transaction_hash: [u8; 32],
    },
    Confirmed {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    Reverted {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    RecoveryPending {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
        replacement_operation_id: EvmOperationId,
    },
    Recovered {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
        resolution: EvmRecoveryResolution,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmRecoveryResolution {
    ReplacementConfirmed {
        replacement_operation_id: EvmOperationId,
    },
    ReplacementReverted {
        replacement_operation_id: EvmOperationId,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationEvent {
    Prepared,
    Submitted {
        transaction_hash: [u8; 32],
    },
    Confirmed {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    Reverted {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    StartRecovery {
        replacement_operation_id: EvmOperationId,
    },
    ResolveRecovery {
        resolution: EvmRecoveryResolution,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvmOperationRecord {
    pub id: EvmOperationId,
    pub payload_hash: [u8; 32],
    pub kind: EvmOperationKind,
    pub state: EvmOperationState,
    pub recovery_of: Option<EvmOperationId>,
}

impl EvmOperationRecord {
    pub const fn queued(
        id: EvmOperationId,
        payload_hash: [u8; 32],
        kind: EvmOperationKind,
    ) -> Self {
        Self {
            id,
            payload_hash,
            kind,
            state: EvmOperationState::Queued,
            recovery_of: None,
        }
    }
    pub const fn queued_recovery(
        id: EvmOperationId,
        payload_hash: [u8; 32],
        kind: EvmOperationKind,
        recovery_of: EvmOperationId,
    ) -> Self {
        Self {
            id,
            payload_hash,
            kind,
            state: EvmOperationState::Queued,
            recovery_of: Some(recovery_of),
        }
    }
    pub const fn prepared(
        id: EvmOperationId,
        payload_hash: [u8; 32],
        kind: EvmOperationKind,
    ) -> Self {
        Self {
            id,
            payload_hash,
            kind,
            state: EvmOperationState::Prepared,
            recovery_of: None,
        }
    }

    pub fn verify_retry(&self, payload_hash: [u8; 32]) -> Result<(), CoreError> {
        if !crate::replay_matches(self.payload_hash == payload_hash) {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub fn apply(&mut self, event: EvmOperationEvent) -> Result<ApplyOutcome, CoreError> {
        use EvmOperationEvent as Event;
        use EvmOperationState as State;
        let next = match (self.state, event) {
            (State::Queued, Event::Prepared) => State::Prepared,
            (State::Prepared, Event::Prepared) => return Ok(ApplyOutcome::Idempotent),
            (
                State::Queued,
                Event::Submitted { .. } | Event::Confirmed { .. } | Event::Reverted { .. },
            ) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "operation not prepared",
                })
            }
            (
                State::Submitted { .. } | State::Confirmed { .. } | State::Reverted { .. },
                Event::Prepared,
            ) => return Ok(ApplyOutcome::Idempotent),
            (State::Prepared, Event::Submitted { transaction_hash }) => {
                State::Submitted { transaction_hash }
            }
            (
                State::Submitted {
                    transaction_hash: current,
                },
                Event::Submitted { transaction_hash },
            ) if current == transaction_hash => return Ok(ApplyOutcome::Idempotent),
            (State::Submitted { .. }, Event::Submitted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Submitted {
                    transaction_hash: current,
                },
                Event::Confirmed {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
            ) if current == transaction_hash => State::Confirmed {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            },
            (
                State::Confirmed {
                    transaction_hash: current_hash,
                    receipt_block_number: current_receipt_block,
                    finalized_head_block_number: current_block,
                },
                Event::Confirmed {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
            ) if current_hash == transaction_hash
                && current_receipt_block == receipt_block_number
                && current_block == finalized_head_block_number =>
            {
                return Ok(ApplyOutcome::Idempotent);
            }
            (State::Submitted { .. }, Event::Confirmed { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Submitted {
                    transaction_hash: current,
                },
                Event::Reverted {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
            ) if current == transaction_hash => State::Reverted {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            },
            (
                State::Reverted {
                    transaction_hash: current_hash,
                    receipt_block_number: current_receipt_block,
                    finalized_head_block_number: current_block,
                },
                Event::Reverted {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
            ) if current_hash == transaction_hash
                && current_receipt_block == receipt_block_number
                && current_block == finalized_head_block_number =>
            {
                return Ok(ApplyOutcome::Idempotent);
            }
            (
                State::Submitted { .. } | State::Confirmed { .. } | State::Reverted { .. },
                Event::Reverted { .. },
            ) => return Err(CoreError::ConflictingReplay),
            (
                State::Confirmed {
                    transaction_hash: current,
                    ..
                },
                Event::Submitted { transaction_hash },
            ) if current == transaction_hash => return Ok(ApplyOutcome::Idempotent),
            (State::Confirmed { .. }, Event::Submitted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Reverted {
                    transaction_hash: current,
                    ..
                },
                Event::Submitted { transaction_hash },
            ) if current == transaction_hash => return Ok(ApplyOutcome::Idempotent),
            (State::Reverted { .. }, Event::Submitted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (State::Prepared, Event::Confirmed { .. } | Event::Reverted { .. }) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "terminal before submission",
                });
            }
            (State::Confirmed { .. } | State::Reverted { .. }, Event::Confirmed { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Reverted {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
                Event::StartRecovery {
                    replacement_operation_id,
                },
            ) => State::RecoveryPending {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
                replacement_operation_id,
            },
            (
                State::RecoveryPending {
                    replacement_operation_id: current,
                    ..
                },
                Event::StartRecovery {
                    replacement_operation_id,
                },
            ) if current == replacement_operation_id => return Ok(ApplyOutcome::Idempotent),
            (State::RecoveryPending { .. }, Event::StartRecovery { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Reverted {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                },
                Event::ResolveRecovery { resolution },
            ) => State::Recovered {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
                resolution,
            },
            (
                State::RecoveryPending {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                    replacement_operation_id,
                },
                Event::ResolveRecovery { resolution },
            ) if resolution.replacement_operation_id() == Some(replacement_operation_id) => {
                State::Recovered {
                    transaction_hash,
                    receipt_block_number,
                    finalized_head_block_number,
                    resolution,
                }
            }
            (State::RecoveryPending { .. }, Event::ResolveRecovery { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Recovered {
                    resolution: current,
                    ..
                },
                Event::ResolveRecovery { resolution },
            ) if current == resolution => return Ok(ApplyOutcome::Idempotent),
            (State::Recovered { .. }, Event::ResolveRecovery { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (State::Recovered { .. }, Event::StartRecovery { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Queued | State::Prepared | State::Submitted { .. } | State::Confirmed { .. },
                Event::StartRecovery { .. } | Event::ResolveRecovery { .. },
            ) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "operation is not reverted",
                });
            }
            (
                State::RecoveryPending { .. } | State::Recovered { .. },
                Event::Prepared
                | Event::Submitted { .. }
                | Event::Confirmed { .. }
                | Event::Reverted { .. },
            ) => return Err(CoreError::ConflictingReplay),
        };
        if !crate::monotone(self.state.rank(), next.rank()) {
            return Err(CoreError::ConflictingReplay);
        }
        self.state = next;
        Ok(ApplyOutcome::Applied)
    }
}

impl EvmOperationState {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Queued => 0,
            Self::Prepared => 1,
            Self::Submitted { .. } => 2,
            Self::Confirmed { .. } => 3,
            Self::Reverted { .. } => 3,
            Self::RecoveryPending { .. } => 4,
            Self::Recovered { .. } => 5,
        }
    }
}

impl EvmRecoveryResolution {
    pub const fn replacement_operation_id(self) -> Option<EvmOperationId> {
        match self {
            Self::ReplacementConfirmed {
                replacement_operation_id,
            }
            | Self::ReplacementReverted {
                replacement_operation_id,
            } => Some(replacement_operation_id),
        }
    }
}
