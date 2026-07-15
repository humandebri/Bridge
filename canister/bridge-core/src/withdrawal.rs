use crate::{
    Amount, ApplyResult, CoreError, EvmCallIntent, EvmOperationId, EvmOperationKind,
    EvmOperationRecord, HoldId, LedgerOperation, LedgerTransferIdentity, Settlement, WithdrawalId,
};

const REFUND_WITHDRAWAL_SELECTOR: [u8; 4] = [0xf0, 0x65, 0xe1, 0xff];

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
    ReleaseCancellationPending {
        attempt: Option<TransferAttempt>,
        settlement: Option<Settlement>,
        operation_id: EvmOperationId,
        expected_ledger_fee: Amount,
    },
    ReleaseCancelled {
        operation_id: EvmOperationId,
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

    pub fn reprice_after_bad_fee(
        &self,
        identity: LedgerTransferIdentity,
    ) -> Result<Self, CoreError> {
        if identity.created_at_time_ns <= self.identity.created_at_time_ns
            || identity.memo == self.identity.memo
            || identity.operation != self.identity.operation
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
    pub confirmed_base_block: u64,
    pub base_status_pending: bool,
    pub release_transfer_proven_absent: bool,
    pub reason: RefundReason,
}

impl RefundEligibility {
    fn validate(self) -> Result<(), CoreError> {
        if !crate::refund_allowed(
            self.base_status_pending,
            self.release_transfer_proven_absent,
        ) {
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
    RepriceRelease {
        attempt: Box<TransferAttempt>,
        settlement: Settlement,
    },
    ReleaseSucceeded {
        ledger_block_index: u128,
    },
    ReleaseAmbiguous {
        hold_id: HoldId,
    },
    PrepareReleaseCancellation {
        operation_id: EvmOperationId,
        expected_ledger_fee: Amount,
    },
    ReleaseCancellationConfirmed {
        operation_id: EvmOperationId,
    },
    PrepareAcknowledgement {
        operation_id: EvmOperationId,
    },
    AcknowledgementConfirmed {
        operation_id: EvmOperationId,
    },
    AcknowledgementReverted {
        operation_id: EvmOperationId,
    },
    StartRefund {
        operation_id: EvmOperationId,
        eligibility: RefundEligibility,
    },
    RefundConfirmed {
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
    pub owner: Vec<u8>,
    pub payload_hash: [u8; 32],
    pub amount: Amount,
    pub min_amount_out: Amount,
    pub max_service_fee: Amount,
    pub state: WithdrawalState,
    pub last_settlement_stop_reason: Option<String>,
}

impl WithdrawalRecord {
    pub fn refund_operation_matches(
        &self,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
    ) -> bool {
        let operation_id = match self.state {
            WithdrawalState::RefundPending { operation_id, .. } => operation_id,
            _ => return false,
        };
        let calldata_matches = intent.calldata.len() == 36
            && intent.calldata[..4] == REFUND_WITHDRAWAL_SELECTOR
            && intent.calldata[4..] == self.id.bytes();
        crate::refund_operation_binding(
            operation.kind == EvmOperationKind::RefundWithdrawal,
            operation.id == operation_id && intent.operation_id == operation_id,
            operation.payload_hash == self.payload_hash && intent.payload_hash == self.payload_hash,
            calldata_matches,
        )
    }

    pub fn observed(
        id: WithdrawalId,
        owner: Vec<u8>,
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
            owner,
            payload_hash,
            amount,
            min_amount_out,
            max_service_fee,
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

        if !crate::withdrawal_phase_allows(self.state.phase(), event.phase()) {
            return Err(CoreError::InvalidTransition {
                entity: "withdrawal",
                event: event.name(),
            });
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
                if !crate::release_transfer_matches(
                    attempt.identity.amount.get(),
                    attempt.identity.fee.get(),
                    settlement.amount_out.get(),
                    settlement.ledger_fee.get(),
                ) {
                    return Err(CoreError::SettlementMismatch);
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
                State::Observed,
                Event::PrepareReleaseCancellation {
                    operation_id,
                    expected_ledger_fee,
                },
            ) => (
                State::ReleaseCancellationPending {
                    attempt: None,
                    settlement: None,
                    operation_id,
                    expected_ledger_fee,
                },
                Amount::ZERO,
            ),
            (
                State::ReleasePending {
                    attempt: current_attempt,
                    settlement: current_settlement,
                },
                Event::RepriceRelease {
                    attempt,
                    settlement,
                },
            ) => {
                let expected_attempt =
                    current_attempt.reprice_after_bad_fee(attempt.identity.clone())?;
                if expected_attempt != *attempt
                    || settlement.service_fee != current_settlement.service_fee
                {
                    return Err(CoreError::AttemptPayloadChanged);
                }
                let expected_amount_out = crate::bad_fee_reprice_amount(
                    self.amount.get(),
                    settlement.service_fee.get(),
                    settlement.ledger_fee.get(),
                    self.min_amount_out.get(),
                    true,
                    false,
                )
                .ok_or(CoreError::MinimumAmountNotMet)?;
                if settlement.amount_out.get() != expected_amount_out
                    || attempt.identity.amount != settlement.amount_out
                    || attempt.identity.fee != settlement.ledger_fee
                {
                    return Err(CoreError::SettlementMismatch);
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
                Amount::new(crate::fee_delta_once(
                    false,
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
                State::ReleasePending {
                    attempt,
                    settlement,
                },
                Event::PrepareReleaseCancellation {
                    operation_id,
                    expected_ledger_fee,
                },
            ) => {
                if crate::bad_fee_reprice_amount(
                    self.amount.get(),
                    settlement.service_fee.get(),
                    expected_ledger_fee.get(),
                    self.min_amount_out.get(),
                    true,
                    false,
                )
                .is_some()
                {
                    return Err(CoreError::MinimumAmountNotMet);
                }
                (
                    State::ReleaseCancellationPending {
                        attempt: Some(attempt.clone()),
                        settlement: Some(*settlement),
                        operation_id,
                        expected_ledger_fee,
                    },
                    Amount::ZERO,
                )
            }
            (
                State::ReleaseCancellationPending {
                    operation_id: current,
                    ..
                },
                Event::ReleaseCancellationConfirmed { operation_id },
            ) if *current == operation_id => {
                (State::ReleaseCancelled { operation_id }, Amount::ZERO)
            }
            (
                State::ReleaseCancellationPending { .. },
                Event::ReleaseCancellationConfirmed { .. },
            ) => return Err(CoreError::ConflictingReplay),
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
                Event::AcknowledgementConfirmed { operation_id },
            ) if *current == operation_id => {
                if settlement.terminal_liability_residual(self.amount)? != Amount::ZERO {
                    return Err(CoreError::SettlementMismatch);
                }
                (
                    State::Released {
                        attempt: attempt.clone(),
                        settlement: *settlement,
                        ledger_block_index: *ledger_block_index,
                        source_hold: *source_hold,
                        operation_id,
                    },
                    Amount::ZERO,
                )
            }
            (State::AcknowledgePending { .. }, Event::AcknowledgementConfirmed { .. }) => {
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
                State::Observed | State::ReleaseCancelled { .. },
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
                Event::RefundConfirmed { operation_id },
            ) if *current == operation_id => {
                // RefundConfirmed is bound to the previously prepared Base refund operation by
                // the adapter. This core event carries no independent amount; under that binding,
                // the operation discharges this record's full gross liability.
                if crate::terminal_liability_residual(self.amount.get(), self.amount.get(), 0, 0)
                    != Some(0)
                {
                    return Err(CoreError::SettlementMismatch);
                }
                (State::Refunded { operation_id }, Amount::ZERO)
            }
            (State::RefundPending { .. }, Event::RefundConfirmed { .. }) => {
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
                State::ReleasePending {
                    attempt: current_attempt,
                    settlement: current_settlement,
                },
                Event::RepriceRelease {
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
                State::ReleaseCancellationPending {
                    operation_id: current,
                    expected_ledger_fee: current_fee,
                    ..
                },
                Event::PrepareReleaseCancellation {
                    operation_id,
                    expected_ledger_fee,
                },
            ) => current == operation_id && current_fee == expected_ledger_fee,
            (
                State::ReleaseCancelled {
                    operation_id: current,
                },
                Event::ReleaseCancellationConfirmed { operation_id },
            ) => current == operation_id,
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
                Event::AcknowledgementConfirmed { operation_id },
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
                Event::RefundConfirmed { operation_id },
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

impl WithdrawalState {
    const fn phase(&self) -> u8 {
        match self {
            Self::Observed => 0,
            Self::ReleasePending { .. } => 1,
            Self::ReleaseTransferred { .. } => 2,
            Self::AcknowledgePending { .. } => 3,
            Self::AcknowledgeReverted { .. } => 4,
            Self::Released { .. } => 5,
            Self::RefundPending { .. } => 6,
            Self::RefundReverted { .. } => 7,
            Self::Refunded { .. } => 8,
            Self::ReconciliationHold { .. } => 9,
            Self::ReleaseCancellationPending { .. } => 10,
            Self::ReleaseCancelled { .. } => 11,
        }
    }
}

impl WithdrawalEvent {
    const fn phase(&self) -> u8 {
        match self {
            Self::StartRelease { .. } => 0,
            Self::RepriceRelease { .. } => 1,
            Self::ReleaseSucceeded { .. } => 2,
            Self::ReleaseAmbiguous { .. } => 3,
            Self::PrepareReleaseCancellation { .. } => 4,
            Self::ReleaseCancellationConfirmed { .. } => 5,
            Self::PrepareAcknowledgement { .. } => 6,
            Self::AcknowledgementConfirmed { .. } => 7,
            Self::AcknowledgementReverted { .. } => 8,
            Self::StartRefund { .. } => 9,
            Self::RefundConfirmed { .. } => 10,
            Self::RefundReverted { .. } => 11,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::StartRelease { .. } => "start_release",
            Self::RepriceRelease { .. } => "reprice_release",
            Self::ReleaseSucceeded { .. } => "release_succeeded",
            Self::ReleaseAmbiguous { .. } => "release_ambiguous",
            Self::PrepareReleaseCancellation { .. } => "prepare_release_cancellation",
            Self::ReleaseCancellationConfirmed { .. } => "release_cancellation_confirmed",
            Self::PrepareAcknowledgement { .. } => "prepare_acknowledgement",
            Self::AcknowledgementConfirmed { .. } => "acknowledgement_confirmed",
            Self::AcknowledgementReverted { .. } => "acknowledgement_reverted",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundConfirmed { .. } => "refund_confirmed",
            Self::RefundReverted { .. } => "refund_reverted",
        }
    }
}
