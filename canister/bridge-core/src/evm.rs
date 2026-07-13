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

impl EvmOperationKind {
    pub const fn scheduler_code(self) -> u8 {
        match self {
            Self::AcknowledgeRelease => 0,
            Self::RefundWithdrawal => 1,
            Self::MintDeposit => 2,
        }
    }

    pub const fn scheduler_priority(self) -> u8 {
        crate::scheduler_priority(self.scheduler_code())
    }
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
    Finalized {
        transaction_hash: [u8; 32],
        finalized_block_number: u64,
    },
    Reverted {
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
    Prepared,
    Submitted {
        transaction_hash: [u8; 32],
    },
    Finalized {
        transaction_hash: [u8; 32],
        finalized_block_number: u64,
    },
    Reverted {
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
                Event::Submitted { .. } | Event::Finalized { .. } | Event::Reverted { .. },
            ) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "operation not prepared",
                })
            }
            (
                State::Submitted { .. } | State::Finalized { .. } | State::Reverted { .. },
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
            (State::Submitted { .. }, Event::Finalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Submitted {
                    transaction_hash: current,
                },
                Event::Reverted {
                    transaction_hash,
                    finalized_block_number,
                },
            ) if current == transaction_hash => State::Reverted {
                transaction_hash,
                finalized_block_number,
            },
            (
                State::Reverted {
                    transaction_hash: current_hash,
                    finalized_block_number: current_block,
                },
                Event::Reverted {
                    transaction_hash,
                    finalized_block_number,
                },
            ) if current_hash == transaction_hash && current_block == finalized_block_number => {
                return Ok(ApplyOutcome::Idempotent);
            }
            (
                State::Submitted { .. } | State::Finalized { .. } | State::Reverted { .. },
                Event::Reverted { .. },
            ) => return Err(CoreError::ConflictingReplay),
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
            (State::Prepared, Event::Finalized { .. } | Event::Reverted { .. }) => {
                return Err(CoreError::InvalidTransition {
                    entity: "evm_operation",
                    event: "terminal before submission",
                });
            }
            (State::Finalized { .. } | State::Reverted { .. }, Event::Finalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
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
            Self::Finalized { .. } => 3,
            Self::Reverted { .. } => 3,
        }
    }
}
