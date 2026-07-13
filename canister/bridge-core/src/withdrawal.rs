use crate::{
    Amount, ApplyResult, CoreError, EvmOperationId, HoldId, LedgerOperation,
    LedgerTransferIdentity, Settlement, WithdrawalId,
};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalState {
    Observed,
    ReleasePending {
        transfer: LedgerTransferIdentity,
        settlement: Settlement,
    },
    ReleaseTransferred {
        transfer: LedgerTransferIdentity,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
    },
    AcknowledgePending {
        transfer: LedgerTransferIdentity,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
        operation_id: EvmOperationId,
    },
    Released {
        transfer: LedgerTransferIdentity,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
        operation_id: EvmOperationId,
    },
    RefundPending {
        operation_id: EvmOperationId,
    },
    Refunded {
        operation_id: EvmOperationId,
    },
    ReconciliationHold {
        hold_id: HoldId,
        transfer: LedgerTransferIdentity,
        settlement: Settlement,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalEvent {
    StartRelease {
        transfer: Box<LedgerTransferIdentity>,
        settlement: Settlement,
    },
    ReleaseSucceeded {
        ledger_block_index: u128,
    },
    ReleaseAmbiguous {
        hold_id: HoldId,
    },
    PrepareAcknowledgement {
        operation_id: EvmOperationId,
    },
    AcknowledgementFinalized {
        operation_id: EvmOperationId,
    },
    StartRefund {
        operation_id: EvmOperationId,
    },
    RefundFinalized {
        operation_id: EvmOperationId,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalRecord {
    pub id: WithdrawalId,
    pub payload_hash: [u8; 32],
    pub amount: Amount,
    pub min_amount_out: Amount,
    pub max_service_fee: Amount,
    pub state: WithdrawalState,
}

impl WithdrawalRecord {
    pub fn observed(
        id: WithdrawalId,
        payload_hash: [u8; 32],
        amount: Amount,
        min_amount_out: Amount,
        max_service_fee: Amount,
    ) -> Result<Self, CoreError> {
        if amount == Amount::ZERO || min_amount_out == Amount::ZERO || min_amount_out > amount {
            return Err(CoreError::InvalidAmount);
        }
        Ok(Self {
            id,
            payload_hash,
            amount,
            min_amount_out,
            max_service_fee,
            state: WithdrawalState::Observed,
        })
    }

    pub fn verify_retry(&self, payload_hash: [u8; 32]) -> Result<(), CoreError> {
        if self.payload_hash != payload_hash {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub fn apply(&mut self, event: WithdrawalEvent) -> Result<ApplyResult, CoreError> {
        use WithdrawalEvent as Event;
        use WithdrawalState as State;

        if self.is_idempotent(&event) {
            return Ok(ApplyResult::idempotent());
        }

        let (next, fee_delta) = match (&self.state, event) {
            (
                State::Observed,
                Event::StartRelease {
                    transfer,
                    settlement,
                },
            ) => {
                if transfer.operation != LedgerOperation::ReleaseWithdrawal {
                    return Err(CoreError::InvalidLedgerOperation);
                }
                if transfer.amount != settlement.amount_out {
                    return Err(CoreError::InvalidAmount);
                }
                settlement.validate(self.amount, self.min_amount_out, self.max_service_fee)?;
                (
                    State::ReleasePending {
                        transfer: *transfer,
                        settlement,
                    },
                    Amount::ZERO,
                )
            }
            (
                State::ReleasePending {
                    transfer,
                    settlement,
                },
                Event::ReleaseSucceeded { ledger_block_index },
            ) => (
                State::ReleaseTransferred {
                    transfer: transfer.clone(),
                    settlement: *settlement,
                    ledger_block_index,
                    source_hold: None,
                },
                settlement.service_fee,
            ),
            (
                State::ReleasePending {
                    transfer,
                    settlement,
                },
                Event::ReleaseAmbiguous { hold_id },
            ) => (
                State::ReconciliationHold {
                    hold_id,
                    transfer: transfer.clone(),
                    settlement: *settlement,
                },
                Amount::ZERO,
            ),
            (
                State::ReleaseTransferred {
                    transfer,
                    settlement,
                    ledger_block_index,
                    source_hold,
                },
                Event::PrepareAcknowledgement { operation_id },
            ) => (
                State::AcknowledgePending {
                    transfer: transfer.clone(),
                    settlement: *settlement,
                    ledger_block_index: *ledger_block_index,
                    source_hold: *source_hold,
                    operation_id,
                },
                Amount::ZERO,
            ),
            (
                State::AcknowledgePending {
                    transfer,
                    settlement,
                    ledger_block_index,
                    source_hold,
                    operation_id: current,
                },
                Event::AcknowledgementFinalized { operation_id },
            ) if *current == operation_id => (
                State::Released {
                    transfer: transfer.clone(),
                    settlement: *settlement,
                    ledger_block_index: *ledger_block_index,
                    source_hold: *source_hold,
                    operation_id,
                },
                Amount::ZERO,
            ),
            (State::AcknowledgePending { .. }, Event::AcknowledgementFinalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (State::Observed, Event::StartRefund { operation_id }) => {
                (State::RefundPending { operation_id }, Amount::ZERO)
            }
            (
                State::RefundPending {
                    operation_id: current,
                },
                Event::RefundFinalized { operation_id },
            ) if *current == operation_id => (State::Refunded { operation_id }, Amount::ZERO),
            (State::RefundPending { .. }, Event::RefundFinalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (_, other) => {
                return Err(CoreError::InvalidTransition {
                    entity: "withdrawal",
                    event: other.name(),
                });
            }
        };
        self.state = next;
        Ok(ApplyResult::applied(fee_delta))
    }

    fn is_idempotent(&self, event: &WithdrawalEvent) -> bool {
        use WithdrawalEvent as Event;
        use WithdrawalState as State;

        match (&self.state, event) {
            (
                State::ReleasePending {
                    transfer: current_transfer,
                    settlement: current_settlement,
                }
                | State::ReleaseTransferred {
                    transfer: current_transfer,
                    settlement: current_settlement,
                    ..
                }
                | State::AcknowledgePending {
                    transfer: current_transfer,
                    settlement: current_settlement,
                    ..
                }
                | State::Released {
                    transfer: current_transfer,
                    settlement: current_settlement,
                    ..
                }
                | State::ReconciliationHold {
                    transfer: current_transfer,
                    settlement: current_settlement,
                    ..
                },
                Event::StartRelease {
                    transfer,
                    settlement,
                },
            ) => current_transfer == transfer.as_ref() && current_settlement == settlement,
            (
                State::ReleaseTransferred {
                    ledger_block_index: current,
                    ..
                }
                | State::AcknowledgePending {
                    ledger_block_index: current,
                    ..
                }
                | State::Released {
                    ledger_block_index: current,
                    ..
                },
                Event::ReleaseSucceeded { ledger_block_index },
            ) => current == ledger_block_index,
            (
                State::ReconciliationHold {
                    hold_id: current, ..
                },
                Event::ReleaseAmbiguous { hold_id },
            ) => current == hold_id,
            (
                State::AcknowledgePending {
                    operation_id: current,
                    ..
                }
                | State::Released {
                    operation_id: current,
                    ..
                },
                Event::PrepareAcknowledgement { operation_id },
            )
            | (
                State::Released {
                    operation_id: current,
                    ..
                },
                Event::AcknowledgementFinalized { operation_id },
            )
            | (
                State::RefundPending {
                    operation_id: current,
                }
                | State::Refunded {
                    operation_id: current,
                },
                Event::StartRefund { operation_id },
            )
            | (
                State::Refunded {
                    operation_id: current,
                },
                Event::RefundFinalized { operation_id },
            ) => current == operation_id,
            _ => false,
        }
    }
}

impl WithdrawalEvent {
    const fn name(&self) -> &'static str {
        match self {
            Self::StartRelease { .. } => "start_release",
            Self::ReleaseSucceeded { .. } => "release_succeeded",
            Self::ReleaseAmbiguous { .. } => "release_ambiguous",
            Self::PrepareAcknowledgement { .. } => "prepare_acknowledgement",
            Self::AcknowledgementFinalized { .. } => "acknowledgement_finalized",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundFinalized { .. } => "refund_finalized",
        }
    }
}
