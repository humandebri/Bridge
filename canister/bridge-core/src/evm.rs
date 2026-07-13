use crate::{ApplyOutcome, CoreError, EvmOperationId};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationKind {
    MintDeposit,
    AcknowledgeRelease,
    RefundWithdrawal,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationState {
    Prepared,
    Submitted {
        transaction_hash: [u8; 32],
    },
    Finalized {
        transaction_hash: [u8; 32],
        finalized_block_number: u64,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvmOperationEvent {
    Submitted {
        transaction_hash: [u8; 32],
    },
    Finalized {
        transaction_hash: [u8; 32],
        finalized_block_number: u64,
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
}

impl EvmOperationRecord {
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
        }
    }

    pub fn verify_retry(&self, payload_hash: [u8; 32]) -> Result<(), CoreError> {
        if self.payload_hash != payload_hash {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub fn apply(&mut self, event: EvmOperationEvent) -> Result<ApplyOutcome, CoreError> {
        use EvmOperationEvent as Event;
        use EvmOperationState as State;
        let next = match (self.state, event) {
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
                Event::Finalized {
                    transaction_hash,
                    finalized_block_number,
                },
            ) if current == transaction_hash => State::Finalized {
                transaction_hash,
                finalized_block_number,
            },
            (
                State::Finalized {
                    transaction_hash: current_hash,
                    finalized_block_number: current_block,
                },
                Event::Finalized {
                    transaction_hash,
                    finalized_block_number,
                },
            ) if current_hash == transaction_hash && current_block == finalized_block_number => {
                return Ok(ApplyOutcome::Idempotent);
            }
            (State::Submitted { .. } | State::Finalized { .. }, Event::Finalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Finalized {
                    transaction_hash: current,
                    ..
                },
                Event::Submitted { transaction_hash },
            ) if current == transaction_hash => return Ok(ApplyOutcome::Idempotent),
            (State::Finalized { .. }, Event::Submitted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (State::Prepared, Event::Finalized { .. }) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "finalized",
                });
            }
        };
        self.state = next;
        Ok(ApplyOutcome::Applied)
    }
}
