use crate::{
    Amount, ApplyResult, CoreError, HoldId, LedgerOperation, LedgerTransferIdentity, Settlement,
    WithdrawalId,
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
    Paid {
        attempt: TransferAttempt,
        settlement: Settlement,
        ledger_block_index: u128,
        source_hold: Option<HoldId>,
    },
    ReconciliationHold {
        hold_id: HoldId,
        attempt: TransferAttempt,
        settlement: Settlement,
    },
}

impl WithdrawalState {
    const fn phase_code(&self) -> u8 {
        match self {
            Self::Observed => 0,
            Self::ReleasePending { .. } => 1,
            Self::Paid { .. } => 2,
            Self::ReconciliationHold { .. } => 3,
        }
    }
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
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalRecord {
    pub id: WithdrawalId,
    pub base_requester: [u8; 20],
    pub owner: Vec<u8>,
    pub subaccount: [u8; 32],
    pub payload_hash: [u8; 32],
    pub amount: Amount,
    pub max_service_fee: Amount,
    pub charged_service_fee: Amount,
    pub amount_out: Amount,
    pub observed_at_ns: u64,
    pub state: WithdrawalState,
    pub last_settlement_stop_reason: Option<String>,
}

impl WithdrawalRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn observed(
        id: WithdrawalId,
        base_requester: [u8; 20],
        owner: Vec<u8>,
        subaccount: [u8; 32],
        payload_hash: [u8; 32],
        amount: Amount,
        max_service_fee: Amount,
        charged_service_fee: Amount,
        amount_out: Amount,
        observed_at_ns: u64,
    ) -> Result<Self, CoreError> {
        if amount == Amount::ZERO
            || amount_out == Amount::ZERO
            || charged_service_fee > max_service_fee
            || amount_out.checked_add(charged_service_fee)? != amount
        {
            return Err(CoreError::InvalidAmount);
        }
        Ok(Self {
            id,
            base_requester,
            owner,
            subaccount,
            payload_hash,
            amount,
            max_service_fee,
            charged_service_fee,
            amount_out,
            observed_at_ns,
            state: WithdrawalState::Observed,
            last_settlement_stop_reason: None,
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

        let state_code = self.state.phase_code();
        let event_code = event.phase_code();
        if !crate::withdrawal_phase_allows(state_code, event_code) {
            return Err(CoreError::InvalidTransition {
                entity: "withdrawal",
                event: event.name(),
            });
        }

        let settlement_for_effects = match (&self.state, &event) {
            (_, Event::StartRelease { settlement, .. }) => *settlement,
            (State::ReleasePending { settlement, .. }, _)
            | (State::ReconciliationHold { settlement, .. }, _) => *settlement,
            _ => Settlement {
                amount_out: Amount::ZERO,
                service_fee: Amount::ZERO,
                ledger_fee: Amount::ZERO,
            },
        };

        let next = match (&self.state, event) {
            (
                State::Observed,
                Event::StartRelease {
                    attempt,
                    settlement,
                },
            ) => {
                self.validate_attempt(&attempt, settlement)?;
                State::ReleasePending {
                    attempt: *attempt,
                    settlement,
                }
            }
            (
                State::ReleasePending {
                    attempt,
                    settlement,
                },
                Event::ReleaseSucceeded { ledger_block_index },
            ) => {
                self.validate_attempt(attempt, *settlement)?;
                State::Paid {
                    attempt: attempt.clone(),
                    settlement: *settlement,
                    ledger_block_index,
                    source_hold: None,
                }
            }
            (
                State::ReleasePending {
                    attempt,
                    settlement,
                },
                Event::ReleaseAmbiguous { hold_id },
            ) => State::ReconciliationHold {
                hold_id,
                attempt: attempt.clone(),
                settlement: *settlement,
            },
            (_, other) => {
                return Err(CoreError::InvalidTransition {
                    entity: "withdrawal",
                    event: other.name(),
                });
            }
        };
        let effects = crate::withdrawal_transition_effects(
            state_code,
            event_code,
            settlement_for_effects.amount_out.get(),
            settlement_for_effects.ledger_fee.get(),
            settlement_for_effects.service_fee.get(),
        )
        .ok_or(CoreError::InvalidTransition {
            entity: "withdrawal",
            event: "effect_mismatch",
        })?;
        debug_assert_eq!(next.phase_code(), effects.0);
        let fee_delta = Amount::new(effects.2);
        self.state = next;
        Ok(ApplyResult::applied(fee_delta))
    }

    fn validate_attempt(
        &self,
        attempt: &TransferAttempt,
        settlement: Settlement,
    ) -> Result<(), CoreError> {
        if matches!(self.state, WithdrawalState::Observed) && attempt.attempt_no != 0 {
            return Err(CoreError::InvalidLedgerOperation);
        }
        if attempt.identity.operation != LedgerOperation::ReleaseWithdrawal
            || !crate::release_transfer_matches(
                attempt.identity.amount.get(),
                attempt.identity.fee.get(),
                self.amount_out.get(),
                settlement.ledger_fee.get(),
            )
            || attempt.identity.to.owner() != self.owner
            || attempt.identity.to.subaccount() != self.subaccount
            || settlement.amount_out != self.amount_out
            || settlement.service_fee != self.charged_service_fee
        {
            return Err(CoreError::SettlementMismatch);
        }
        settlement.validate_committed(self.amount, self.max_service_fee)
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
                | State::Paid {
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
                State::Paid {
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
            _ => false,
        }
    }
}

impl WithdrawalEvent {
    const fn phase_code(&self) -> u8 {
        match self {
            Self::StartRelease { .. } => 0,
            Self::ReleaseSucceeded { .. } => 2,
            Self::ReleaseAmbiguous { .. } => 3,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::StartRelease { .. } => "start_release",
            Self::ReleaseSucceeded { .. } => "release_succeeded",
            Self::ReleaseAmbiguous { .. } => "release_ambiguous",
        }
    }
}
