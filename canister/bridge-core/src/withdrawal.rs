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
        attempt: TransferAttempt,
        settlement: Settlement,
    },
    ReleaseTransferred {
        attempt: TransferAttempt,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
    },
    AcknowledgePending {
        attempt: TransferAttempt,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
        operation_id: EvmOperationId,
    },
    AcknowledgeReverted {
        attempt: TransferAttempt,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
        operation_id: EvmOperationId,
    },
    Released {
        attempt: TransferAttempt,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
        operation_id: EvmOperationId,
    },
    RefundPending {
        operation_id: EvmOperationId,
        eligibility: RefundEligibility,
    },
    RefundReverted {
        operation_id: EvmOperationId,
        eligibility: RefundEligibility,
    },
    Refunded {
        operation_id: EvmOperationId,
    },
    ReconciliationHold {
        hold_id: HoldId,
        attempt: TransferAttempt,
        settlement: Settlement,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferAttempt {
    pub attempt_no: u64,
    pub identity: LedgerTransferIdentity,
}

impl TransferAttempt {
    pub fn retry_after_absence(&self, identity: LedgerTransferIdentity) -> Result<Self, CoreError> {
        if identity.created_at_time_ns <= self.identity.created_at_time_ns
            || identity.memo == self.identity.memo
            || identity.operation != self.identity.operation
            || identity.amount != self.identity.amount
            || identity.fee != self.identity.fee
            || identity.from != self.identity.from
            || identity.to != self.identity.to
            || identity.spender != self.identity.spender
        {
            return Err(CoreError::AttemptPayloadChanged);
        }
        Ok(Self {
            attempt_no: crate::next_attempt(self.attempt_no).ok_or(CoreError::AttemptOverflow)?,
            identity,
        })
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundReason {
    BridgeStopped,
    AmountBelowMinimum,
    InvalidRecipient,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundEligibility {
    pub finalized_base_block: u64,
    pub base_status_pending: bool,
    pub release_attempt_created: bool,
    pub reason: RefundReason,
}

impl RefundEligibility {
    fn validate(self) -> Result<(), CoreError> {
        if !crate::refund_allowed(self.base_status_pending, self.release_attempt_created) {
            return Err(CoreError::RefundIneligible);
        }
        Ok(())
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalEvent {
    StartRelease {
        attempt: Box<TransferAttempt>,
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
    AcknowledgementReverted {
        operation_id: EvmOperationId,
    },
    StartRefund {
        operation_id: EvmOperationId,
        eligibility: RefundEligibility,
    },
    RefundFinalized {
        operation_id: EvmOperationId,
    },
    RefundReverted {
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
        if !crate::replay_matches(self.payload_hash == payload_hash) {
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
                    attempt,
                    settlement,
                },
            ) => {
                if attempt.attempt_no != 0
                    || attempt.identity.operation != LedgerOperation::ReleaseWithdrawal
                {
                    return Err(CoreError::InvalidLedgerOperation);
                }
                if attempt.identity.amount != settlement.amount_out {
                    return Err(CoreError::InvalidAmount);
                }
                settlement.validate(self.amount, self.min_amount_out, self.max_service_fee)?;
                (
                    State::ReleasePending {
                        attempt: *attempt,
                        settlement,
                    },
                    Amount::ZERO,
                )
            }
            (
                State::ReleasePending {
                    attempt,
                    settlement,
                },
                Event::ReleaseSucceeded { ledger_block_index },
            ) => (
                State::ReleaseTransferred {
                    attempt: attempt.clone(),
                    settlement: *settlement,
                    ledger_block_index,
                    source_hold: None,
                },
                Amount::new(crate::terminal_retry_fee(
                    true,
                    settlement.service_fee.get(),
                )),
            ),
            (
                State::ReleasePending {
                    attempt,
                    settlement,
                },
                Event::ReleaseAmbiguous { hold_id },
            ) => (
                State::ReconciliationHold {
                    hold_id,
                    attempt: attempt.clone(),
                    settlement: *settlement,
                },
                Amount::ZERO,
            ),
            (
                State::ReleaseTransferred {
                    attempt,
                    settlement,
                    ledger_block_index,
                    source_hold,
                },
                Event::PrepareAcknowledgement { operation_id },
            ) => (
                State::AcknowledgePending {
                    attempt: attempt.clone(),
                    settlement: *settlement,
                    ledger_block_index: *ledger_block_index,
                    source_hold: *source_hold,
                    operation_id,
                },
                Amount::ZERO,
            ),
            (
                State::AcknowledgePending {
                    attempt,
                    settlement,
                    ledger_block_index,
                    source_hold,
                    operation_id: current,
                },
                Event::AcknowledgementFinalized { operation_id },
            ) if *current == operation_id => (
                State::Released {
                    attempt: attempt.clone(),
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
            (
                State::AcknowledgePending {
                    attempt,
                    settlement,
                    ledger_block_index,
                    source_hold,
                    operation_id: current,
                },
                Event::AcknowledgementReverted { operation_id },
            ) if *current == operation_id => (
                State::AcknowledgeReverted {
                    attempt: attempt.clone(),
                    settlement: *settlement,
                    ledger_block_index: *ledger_block_index,
                    source_hold: *source_hold,
                    operation_id,
                },
                Amount::ZERO,
            ),
            (State::AcknowledgePending { .. }, Event::AcknowledgementReverted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::Observed,
                Event::StartRefund {
                    operation_id,
                    eligibility,
                },
            ) => {
                eligibility.validate()?;
                (
                    State::RefundPending {
                        operation_id,
                        eligibility,
                    },
                    Amount::ZERO,
                )
            }
            (
                State::RefundPending {
                    operation_id: current,
                    ..
                },
                Event::RefundFinalized { operation_id },
            ) if *current == operation_id => (State::Refunded { operation_id }, Amount::ZERO),
            (State::RefundPending { .. }, Event::RefundFinalized { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::RefundPending {
                    operation_id: current,
                    eligibility,
                },
                Event::RefundReverted { operation_id },
            ) if *current == operation_id => (
                State::RefundReverted {
                    operation_id,
                    eligibility: *eligibility,
                },
                Amount::ZERO,
            ),
            (State::RefundPending { .. }, Event::RefundReverted { .. }) => {
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
                    attempt: current_attempt,
                    settlement: current_settlement,
                }
                | State::ReleaseTransferred {
                    attempt: current_attempt,
                    settlement: current_settlement,
                    ..
                }
                | State::AcknowledgePending {
                    attempt: current_attempt,
                    settlement: current_settlement,
                    ..
                }
                | State::Released {
                    attempt: current_attempt,
                    settlement: current_settlement,
                    ..
                }
                | State::ReconciliationHold {
                    attempt: current_attempt,
                    settlement: current_settlement,
                    ..
                },
                Event::StartRelease {
                    attempt,
                    settlement,
                },
            ) => current_attempt == attempt.as_ref() && current_settlement == settlement,
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
            ) => current == operation_id,
            (
                State::AcknowledgeReverted {
                    operation_id: current,
                    ..
                },
                Event::AcknowledgementReverted { operation_id },
            ) => current == operation_id,
            (
                State::RefundPending {
                    operation_id: current,
                    eligibility: current_eligibility,
                },
                Event::StartRefund {
                    operation_id,
                    eligibility,
                },
            ) => current == operation_id && current_eligibility == eligibility,
            (
                State::Refunded {
                    operation_id: current,
                },
                Event::StartRefund { operation_id, .. },
            )
            | (
                State::Refunded {
                    operation_id: current,
                },
                Event::RefundFinalized { operation_id },
            ) => current == operation_id,
            (
                State::RefundReverted {
                    operation_id: current,
                    ..
                },
                Event::RefundReverted { operation_id },
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
            Self::AcknowledgementReverted { .. } => "acknowledgement_reverted",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundFinalized { .. } => "refund_finalized",
            Self::RefundReverted { .. } => "refund_reverted",
        }
    }
}
