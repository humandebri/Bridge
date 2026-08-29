use crate::{
    Amount, ApplyResult, CoreError, DepositId, HoldId, LedgerFailure, LedgerOperation,
    LedgerTransferIdentity, MintAuthorizationRecord, MintExpiryEvidence, MintFinalizationEvidence,
    TransferAttempt,
};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositRequest {
    pub id: DepositId,
    pub payload_hash: [u8; 32],
    pub gross_amount: Amount,
    pub user_max_service_fee: Amount,
    pub transfer: LedgerTransferIdentity,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositQuote {
    pub service_fee: Amount,
    pub net_amount: Amount,
}

impl DepositQuote {
    pub fn validate(self, gross_amount: Amount) -> Result<(), CoreError> {
        if self.net_amount == Amount::ZERO
            || self.net_amount.checked_add(self.service_fee)? != gross_amount
        {
            return Err(CoreError::InvalidAmount);
        }
        Ok(())
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositRefundReason {
    BasePaused,
    ServiceFeeRejected,
    PerDepositLimitExceeded,
    MintWindowLimitExceeded,
    AuthorizationExpired,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositState {
    FundingPending,
    EscrowedUnquoted {
        funding_ledger_block_index: u128,
    },
    AuthorizationPending {
        funding_ledger_block_index: u128,
    },
    AuthorizationAvailable {
        funding_ledger_block_index: u128,
    },
    RefundAvailable {
        reason: DepositRefundReason,
        funding_ledger_block_index: u128,
    },
    Minted {
        funding_ledger_block_index: u128,
    },
    FundingReconciliationHold {
        hold_id: HoldId,
    },
    RefundPending {
        reason: DepositRefundReason,
        funding_ledger_block_index: u128,
        attempt: TransferAttempt,
    },
    RefundReconciliationHold {
        reason: DepositRefundReason,
        funding_ledger_block_index: u128,
        hold_id: HoldId,
        attempt: TransferAttempt,
    },
    Refunded {
        reason: DepositRefundReason,
        funding_ledger_block_index: u128,
        attempt: TransferAttempt,
        refund_ledger_block_index: u128,
        source_hold: Option<HoldId>,
    },
    Cancelled {
        hold_id: Option<HoldId>,
        history_watermark: Option<u128>,
        ledger_failure: Option<LedgerFailure>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositEvent {
    FundingSucceeded {
        funding_ledger_block_index: u128,
    },
    FundingAmbiguous {
        hold_id: HoldId,
    },
    FundingFailed {
        code: LedgerFailure,
    },
    CommitAuthorization {
        quote: DepositQuote,
        authorization: Box<MintAuthorizationRecord>,
    },
    AuthorizationSigned {
        signature: Vec<u8>,
        observed_timestamp: u64,
    },
    MarkRefundAvailable {
        reason: DepositRefundReason,
        finalized_timestamp: Option<u64>,
    },
    MintReconciled {
        evidence: Box<MintFinalizationEvidence>,
    },
    StartRefund {
        reason: DepositRefundReason,
        attempt: Box<TransferAttempt>,
        expiry_evidence: Option<Box<MintExpiryEvidence>>,
    },
    RefundSucceeded {
        refund_ledger_block_index: u128,
    },
    RefundAmbiguous {
        hold_id: HoldId,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositRecord {
    pub id: DepositId,
    pub payload_hash: [u8; 32],
    pub gross_amount: Amount,
    pub max_service_fee: Amount,
    pub quote: Option<DepositQuote>,
    pub mint_authorization: Option<MintAuthorizationRecord>,
    pub mint_finalization_evidence: Option<MintFinalizationEvidence>,
    pub mint_expiry_evidence: Option<MintExpiryEvidence>,
    pub transfer: LedgerTransferIdentity,
    pub state: DepositState,
    pub last_settlement_stop_reason: Option<String>,
}

impl DepositRecord {
    pub fn accept(request: DepositRequest) -> Result<Self, CoreError> {
        if request.transfer.operation != LedgerOperation::PullDeposit {
            return Err(CoreError::InvalidLedgerOperation);
        }
        if request.transfer.amount != request.gross_amount || request.gross_amount == Amount::ZERO {
            return Err(CoreError::InvalidAmount);
        }
        Ok(Self {
            id: request.id,
            payload_hash: request.payload_hash,
            gross_amount: request.gross_amount,
            max_service_fee: request.user_max_service_fee,
            quote: None,
            mint_authorization: None,
            mint_finalization_evidence: None,
            mint_expiry_evidence: None,
            transfer: request.transfer,
            state: DepositState::FundingPending,
            last_settlement_stop_reason: None,
        })
    }

    pub fn verify_retry(&self, payload_hash: [u8; 32]) -> Result<(), CoreError> {
        if !crate::kernel::replay_matches(self.payload_hash == payload_hash) {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub const fn reserves_mint_resources(&self) -> bool {
        crate::kernel::deposit_reservation_active(self.state.code())
    }

    pub fn reserved_mint_amount(&self) -> Result<Amount, CoreError> {
        if !self.reserves_mint_resources() {
            return Ok(Amount::ZERO);
        }
        self.quote
            .map(|quote| quote.net_amount)
            .ok_or(CoreError::InvalidTransition {
                entity: "deposit",
                event: "missing_quote",
            })
    }

    pub fn apply(&mut self, event: DepositEvent) -> Result<ApplyResult, CoreError> {
        use DepositEvent as Event;
        use DepositState as State;

        let current_ledger_blocks = self.ledger_blocks();
        let ledger_event = match &event {
            Event::FundingSucceeded {
                funding_ledger_block_index,
            } => (1, *funding_ledger_block_index),
            Event::RefundSucceeded {
                refund_ledger_block_index,
            } => (2, *refund_ledger_block_index),
            _ => (0, 0),
        };
        let expected_ledger_blocks = crate::kernel::deposit_ledger_block_transition(
            current_ledger_blocks.0,
            current_ledger_blocks.1,
            ledger_event.0,
            ledger_event.1,
        )
        .ok_or(CoreError::LedgerBlockConflict)?;

        let (guard, guard_error) = self.transition_guard(&event);
        let transition_quote = match (&event, self.quote) {
            (Event::CommitAuthorization { quote, .. }, _) => Some(*quote),
            (_, quote) => quote,
        };
        let authorization_issued = self
            .mint_authorization
            .as_ref()
            .is_some_and(|authorization| authorization.signature.is_some())
            || matches!(event, Event::AuthorizationSigned { .. });
        let service_fee = if authorization_issued {
            transition_quote.map_or(0, |quote| quote.service_fee.get())
        } else {
            0
        };
        let net_amount = if matches!(event, Event::StartRefund { .. }) && !authorization_issued {
            self.gross_amount.get()
        } else {
            transition_quote.map_or(0, |quote| quote.net_amount.get())
        };
        let reserved_amount = self.reserved_mint_amount()?.get();
        let transition =
            crate::kernel::deposit_transition_decision(crate::DepositTransitionInput {
                state: self.state.code(),
                event: event.code(),
                guard,
                same_payload: self.is_idempotent(&event),
                gross_amount: self.gross_amount.get(),
                net_amount,
                service_fee,
                reserved_amount,
            });
        let expected_effects = match transition {
            crate::DepositTransitionDecision::Idempotent => return Ok(ApplyResult::idempotent()),
            crate::DepositTransitionDecision::Reject => {
                return Err(guard_error.unwrap_or(CoreError::InvalidTransition {
                    entity: "deposit",
                    event: event.name(),
                }))
            }
            crate::DepositTransitionDecision::Apply(effects) => effects,
        };

        let (
            next_state,
            next_quote,
            next_authorization,
            next_finalization_evidence,
            next_expiry_evidence,
            fee_delta,
        ) = match (&self.state, event) {
            (
                State::FundingPending,
                Event::FundingSucceeded {
                    funding_ledger_block_index,
                },
            ) => (
                State::EscrowedUnquoted {
                    funding_ledger_block_index,
                },
                None,
                None,
                None,
                None,
                Amount::ZERO,
            ),
            (State::FundingPending, Event::FundingAmbiguous { hold_id }) => (
                State::FundingReconciliationHold { hold_id },
                None,
                None,
                None,
                None,
                Amount::ZERO,
            ),
            (State::FundingPending, Event::FundingFailed { code }) => (
                State::Cancelled {
                    hold_id: None,
                    history_watermark: None,
                    ledger_failure: Some(code),
                },
                None,
                None,
                None,
                None,
                Amount::ZERO,
            ),
            (
                State::EscrowedUnquoted {
                    funding_ledger_block_index,
                },
                Event::CommitAuthorization {
                    quote,
                    authorization,
                },
            ) => (
                State::AuthorizationPending {
                    funding_ledger_block_index: *funding_ledger_block_index,
                },
                Some(quote),
                Some(*authorization),
                None,
                None,
                Amount::ZERO,
            ),
            (
                State::AuthorizationPending {
                    funding_ledger_block_index,
                },
                Event::AuthorizationSigned { signature, .. },
            ) => {
                let mut authorization =
                    self.mint_authorization
                        .clone()
                        .ok_or(CoreError::InvalidTransition {
                            entity: "deposit",
                            event: "missing_authorization",
                        })?;
                if !authorization.install_signature(signature) {
                    return Err(CoreError::ConflictingReplay);
                }
                (
                    State::AuthorizationAvailable {
                        funding_ledger_block_index: *funding_ledger_block_index,
                    },
                    self.quote,
                    Some(authorization),
                    self.mint_finalization_evidence.clone(),
                    self.mint_expiry_evidence.clone(),
                    self.quote.ok_or(CoreError::InvalidAmount)?.service_fee,
                )
            }
            (
                State::EscrowedUnquoted {
                    funding_ledger_block_index,
                }
                | State::AuthorizationPending {
                    funding_ledger_block_index,
                },
                Event::MarkRefundAvailable {
                    reason,
                    finalized_timestamp: _,
                },
            ) => (
                State::RefundAvailable {
                    reason,
                    funding_ledger_block_index: *funding_ledger_block_index,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (
                State::AuthorizationAvailable {
                    funding_ledger_block_index,
                },
                Event::MarkRefundAvailable {
                    reason,
                    finalized_timestamp: _,
                },
            ) => (
                State::RefundAvailable {
                    reason,
                    funding_ledger_block_index: *funding_ledger_block_index,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (
                State::RefundAvailable {
                    funding_ledger_block_index,
                    ..
                },
                Event::MintReconciled { evidence },
            ) => (
                State::Minted {
                    funding_ledger_block_index: *funding_ledger_block_index,
                },
                self.quote,
                self.mint_authorization.clone(),
                Some(*evidence),
                None,
                Amount::ZERO,
            ),
            (
                State::RefundAvailable {
                    funding_ledger_block_index,
                    ..
                },
                Event::StartRefund {
                    reason,
                    attempt,
                    expiry_evidence,
                },
            ) => (
                State::RefundPending {
                    reason,
                    funding_ledger_block_index: *funding_ledger_block_index,
                    attempt: *attempt,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                expiry_evidence.map(|evidence| *evidence),
                Amount::ZERO,
            ),
            (
                State::RefundPending {
                    reason,
                    funding_ledger_block_index,
                    attempt,
                },
                Event::RefundSucceeded {
                    refund_ledger_block_index,
                },
            ) => (
                State::Refunded {
                    reason: *reason,
                    funding_ledger_block_index: *funding_ledger_block_index,
                    attempt: attempt.clone(),
                    refund_ledger_block_index,
                    source_hold: None,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (
                State::RefundPending {
                    reason,
                    funding_ledger_block_index,
                    attempt,
                },
                Event::RefundAmbiguous { hold_id },
            ) => (
                State::RefundReconciliationHold {
                    reason: *reason,
                    funding_ledger_block_index: *funding_ledger_block_index,
                    hold_id,
                    attempt: attempt.clone(),
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (_, other) => {
                return Err(CoreError::InvalidTransition {
                    entity: "deposit",
                    event: other.name(),
                });
            }
        };
        if Self::state_ledger_blocks(&next_state) != expected_ledger_blocks {
            return Err(CoreError::InvalidTransition {
                entity: "deposit",
                event: "ledger_block_effect_mismatch",
            });
        }
        let next_state_code = next_state.code();
        let next_reservation_active = crate::deposit_reservation_active(next_state_code);
        if next_state_code != expected_effects.next_state
            || next_reservation_active != expected_effects.reservation_active
            || (self.reserves_mint_resources() && !next_reservation_active)
                != expected_effects.release_reservation
            || next_quote.map_or(0, |quote| {
                if next_reservation_active {
                    quote.net_amount.get()
                } else {
                    0
                }
            }) != expected_effects.reservation_after
            || (if !self.reserves_mint_resources() && next_reservation_active {
                next_quote.map_or(0, |quote| quote.net_amount.get())
            } else {
                0
            }) != expected_effects.reservation_add
            || fee_delta.get() != expected_effects.fee_credit
        {
            return Err(CoreError::InvalidTransition {
                entity: "deposit",
                event: "transition_effect_mismatch",
            });
        }
        self.state = next_state;
        self.quote = next_quote;
        self.mint_authorization = next_authorization;
        self.mint_finalization_evidence = next_finalization_evidence;
        self.mint_expiry_evidence = next_expiry_evidence;
        Ok(ApplyResult::applied_deposit(
            crate::DepositAccountingEffects {
                reservation_after: Amount::new(expected_effects.reservation_after),
                reservation_add: Amount::new(expected_effects.reservation_add),
                reservation_release: Amount::new(expected_effects.reservation_release),
                fee_credit: Amount::new(expected_effects.fee_credit),
                pending_liability_debit: Amount::new(expected_effects.pending_liability_debit),
                escrow_debit: Amount::new(expected_effects.escrow_debit),
                mint_supply_increase: Amount::new(expected_effects.mint_supply_increase),
            },
        ))
    }

    pub(crate) fn funding_ledger_block_index(&self) -> Option<u128> {
        self.ledger_blocks().0
    }

    fn ledger_blocks(&self) -> (Option<u128>, Option<u128>) {
        Self::state_ledger_blocks(&self.state)
    }

    fn state_ledger_blocks(state: &DepositState) -> (Option<u128>, Option<u128>) {
        use DepositState as State;
        match state {
            State::EscrowedUnquoted {
                funding_ledger_block_index,
            }
            | State::AuthorizationPending {
                funding_ledger_block_index,
            }
            | State::AuthorizationAvailable {
                funding_ledger_block_index,
            }
            | State::RefundAvailable {
                funding_ledger_block_index,
                ..
            }
            | State::Minted {
                funding_ledger_block_index,
            }
            | State::RefundPending {
                funding_ledger_block_index,
                ..
            }
            | State::RefundReconciliationHold {
                funding_ledger_block_index,
                ..
            } => (Some(*funding_ledger_block_index), None),
            State::Refunded {
                funding_ledger_block_index,
                refund_ledger_block_index,
                ..
            } => (
                Some(*funding_ledger_block_index),
                Some(*refund_ledger_block_index),
            ),
            State::FundingPending
            | State::FundingReconciliationHold { .. }
            | State::Cancelled { .. } => (None, None),
        }
    }

    fn transition_guard(
        &self,
        event: &DepositEvent,
    ) -> (crate::DepositEventGuard, Option<CoreError>) {
        use crate::DepositEventGuard as Guard;
        use DepositEvent as Event;
        use DepositState as State;

        match event {
            Event::FundingSucceeded { .. }
            | Event::FundingAmbiguous { .. }
            | Event::FundingFailed { .. } => (Guard::Funding, None),
            Event::CommitAuthorization {
                quote,
                authorization: record,
            } => {
                let quote_result = quote.validate(self.gross_amount);
                let authorization = record.authorization;
                let fixed_fields_match = authorization.deposit_id == self.id.bytes()
                    && authorization.recipient != [0; 20]
                    && authorization.gross_amount == self.gross_amount
                    && authorization.max_service_fee == self.max_service_fee
                    && authorization.charged_service_fee == quote.service_fee
                    && record.domain.chain_id != 0
                    && record.domain.verifying_contract != [0; 20]
                    && record.digest != [0; 32]
                    && record.origin.finalized_block_hash != [0; 32]
                    && authorization.authorization_epoch != 0;
                let canonical_domain_strings = record.domain.name
                    == crate::MINT_AUTHORIZATION_DOMAIN_NAME
                    && record.domain.version == crate::MINT_AUTHORIZATION_DOMAIN_VERSION;
                let deadline_valid = record
                    .origin
                    .issued_at_timestamp
                    .checked_add(crate::MINT_AUTHORIZATION_TTL_SECONDS)
                    == Some(authorization.deadline);
                let pristine = record.signature.is_none()
                    && !record.signature_dispatched
                    && record.signature_dispatch_attempt == 0;
                let guard = Guard::CommitAuthorization {
                    quote_valid: quote_result.is_ok(),
                    fixed_fields_match,
                    canonical_domain_strings,
                    deadline_valid,
                    pristine,
                };
                (
                    guard,
                    (!guard.accepts()).then_some(if let Err(error) = quote_result {
                        error
                    } else if !deadline_valid
                        && record
                            .origin
                            .issued_at_timestamp
                            .checked_add(crate::MINT_AUTHORIZATION_TTL_SECONDS)
                            .is_none()
                    {
                        CoreError::ArithmeticOverflow
                    } else {
                        CoreError::ConflictingReplay
                    }),
                )
            }
            Event::AuthorizationSigned {
                signature,
                observed_timestamp,
            } => {
                let authorization = self.mint_authorization.as_ref();
                let guard = Guard::InstallSignature {
                    dispatched: authorization.is_some_and(|record| record.signature_dispatched),
                    signature_absent: authorization
                        .is_some_and(|record| record.signature.is_none()),
                    signature_length_valid: signature.len() == 65,
                    minimum_remaining: authorization.is_some_and(|record| {
                        record
                            .authorization
                            .has_minimum_remaining_time(*observed_timestamp)
                    }),
                };
                (
                    guard,
                    (!guard.accepts()).then_some(CoreError::ConflictingReplay),
                )
            }
            Event::MarkRefundAvailable {
                reason,
                finalized_timestamp,
            } => {
                let deadline = self
                    .mint_authorization
                    .as_ref()
                    .map(|record| record.authorization.deadline);
                (
                    Guard::MarkRefundAvailable {
                        policy_allowed: match (&self.state, reason, finalized_timestamp) {
                            (State::EscrowedUnquoted { .. }, reason, None) => {
                                *reason != DepositRefundReason::AuthorizationExpired
                            }
                            (
                                State::AuthorizationPending { .. }
                                | State::AuthorizationAvailable { .. },
                                DepositRefundReason::AuthorizationExpired,
                                Some(finalized_timestamp),
                            ) => deadline.is_some_and(|deadline| *finalized_timestamp > deadline),
                            _ => false,
                        },
                    },
                    None,
                )
            }
            Event::MintReconciled { evidence } => {
                let authorization = self.mint_authorization.as_ref();
                let expected_net = authorization.and_then(|record| {
                    record
                        .authorization
                        .gross_amount
                        .checked_sub(record.authorization.charged_service_fee)
                        .ok()
                });
                let fixed_fields_match = authorization.is_some_and(|authorization| {
                    authorization.signature.is_some()
                        && evidence.authorization_digest == authorization.digest
                        && evidence.deposit_id == authorization.authorization.deposit_id
                        && evidence.recipient == authorization.authorization.recipient
                        && evidence.chain_id == authorization.domain.chain_id
                        && evidence.verifying_contract == authorization.domain.verifying_contract
                        && evidence.gross_amount == authorization.authorization.gross_amount
                        && evidence.charged_service_fee
                            == authorization.authorization.charged_service_fee
                        && Some(evidence.minted_amount) == expected_net
                });
                let origin_inclusion = authorization.is_some_and(|authorization| {
                    evidence.receipt_block_number >= authorization.origin.finalized_block_number
                });
                let audit_complete = evidence.transaction_hash != [0; 32]
                    && evidence.receipt_block_hash != [0; 32]
                    && evidence.finalized_block_hash != [0; 32]
                    && evidence.rpc_request_digest != [0; 32]
                    && evidence.rpc_response_digest != [0; 32];
                let guard = Guard::MintFinalization {
                    fixed_fields_match: fixed_fields_match && origin_inclusion,
                    receipt_succeeded: evidence.receipt_succeeded,
                    receipt_block: evidence.receipt_block_number,
                    finalized_block: evidence.finalized_block_number,
                    audit_complete,
                };
                (
                    guard,
                    (!guard.accepts()).then_some(CoreError::ConflictingReplay),
                )
            }
            Event::StartRefund {
                reason,
                attempt,
                expiry_evidence,
            } => {
                let identity = &attempt.identity;
                let service_fee = if self
                    .mint_authorization
                    .as_ref()
                    .is_some_and(|authorization| authorization.signature.is_some())
                {
                    self.quote.map_or(Amount::ZERO, |quote| quote.service_fee)
                } else {
                    Amount::ZERO
                };
                let attempt_matches = attempt.attempt_no == 0
                    && identity.operation == LedgerOperation::RefundDeposit
                    && identity.spender.is_none()
                    && identity.from == self.transfer.to
                    && identity.to == self.transfer.from
                    && identity
                        .amount
                        .checked_add(identity.fee)
                        .and_then(|amount| amount.checked_add(service_fee))
                        .is_ok_and(|amount| amount == self.gross_amount);
                let signed_authorization = self
                    .mint_authorization
                    .as_ref()
                    .is_some_and(|authorization| authorization.signature.is_some());
                let policy_matches = match (&self.state, reason, expiry_evidence.as_deref()) {
                    (
                        State::RefundAvailable {
                            reason: expected, ..
                        },
                        DepositRefundReason::AuthorizationExpired,
                        Some(evidence),
                    ) if signed_authorization => {
                        *expected == DepositRefundReason::AuthorizationExpired
                            && self.expiry_evidence_matches(evidence)
                    }
                    (
                        State::RefundAvailable {
                            reason: expected, ..
                        },
                        reason,
                        None,
                    ) if !signed_authorization => expected == reason,
                    _ => false,
                };
                let guard = Guard::StartRefund {
                    attempt_matches,
                    policy_matches,
                };
                (
                    guard,
                    (!guard.accepts()).then_some(CoreError::RefundIneligible),
                )
            }
            Event::RefundSucceeded { .. } | Event::RefundAmbiguous { .. } => {
                (Guard::RefundResult, None)
            }
        }
    }

    fn expiry_evidence_matches(&self, evidence: &MintExpiryEvidence) -> bool {
        let Some(authorization) = self.mint_authorization.as_ref() else {
            return false;
        };
        let binding_matches = evidence.authorization_digest == authorization.digest
            && evidence.deposit_id == authorization.authorization.deposit_id
            && evidence.chain_id == authorization.domain.chain_id
            && evidence.verifying_contract == authorization.domain.verifying_contract
            && evidence.finalized_block_number >= authorization.origin.finalized_block_number
            && evidence.finalized_block_hash != [0; 32]
            && evidence.runtime_sha256 != [0; 32]
            && evidence.rpc_request_digest != [0; 32]
            && evidence.rpc_response_digest != [0; 32];
        crate::kernel::expiry_refund_allowed(
            binding_matches,
            evidence.deposit_processed,
            evidence.finalized_block_timestamp,
            authorization.authorization.deadline,
        )
    }

    fn is_idempotent(&self, event: &DepositEvent) -> bool {
        use DepositEvent as Event;
        use DepositState as State;
        if let Event::FundingSucceeded {
            funding_ledger_block_index,
        } = event
        {
            return self.funding_ledger_block_index() == Some(*funding_ledger_block_index);
        }
        match (&self.state, event) {
            (
                State::FundingReconciliationHold { hold_id: current },
                Event::FundingAmbiguous { hold_id },
            ) => *current == *hold_id,
            (
                State::Cancelled {
                    hold_id: None,
                    history_watermark: None,
                    ledger_failure: Some(current),
                },
                Event::FundingFailed { code },
            ) => current == code,
            (
                State::AuthorizationPending { .. }
                | State::AuthorizationAvailable { .. }
                | State::RefundAvailable { .. }
                | State::Minted { .. },
                Event::CommitAuthorization {
                    quote,
                    authorization,
                },
            ) => {
                self.quote == Some(*quote)
                    && self.mint_authorization.as_ref() == Some(authorization)
            }
            (
                State::AuthorizationAvailable { .. },
                Event::AuthorizationSigned { signature, .. },
            ) => {
                self.mint_authorization
                    .as_ref()
                    .and_then(|record| record.signature.as_ref())
                    == Some(signature)
            }
            (
                State::RefundAvailable {
                    reason: current, ..
                },
                Event::MarkRefundAvailable { reason, .. },
            ) => current == reason,
            (State::Minted { .. }, Event::MintReconciled { evidence }) => {
                self.mint_finalization_evidence.as_ref() == Some(evidence)
            }
            (
                State::RefundPending {
                    reason: current_reason,
                    attempt: current_attempt,
                    ..
                },
                Event::StartRefund {
                    reason,
                    attempt,
                    expiry_evidence,
                },
            ) => {
                current_reason == reason
                    && current_attempt == attempt.as_ref()
                    && self.mint_expiry_evidence.as_ref() == expiry_evidence.as_deref()
            }
            (
                State::Refunded {
                    refund_ledger_block_index: current,
                    source_hold: None,
                    ..
                },
                Event::RefundSucceeded {
                    refund_ledger_block_index,
                },
            ) => *current == *refund_ledger_block_index,
            (
                State::RefundReconciliationHold {
                    hold_id: current, ..
                },
                Event::RefundAmbiguous { hold_id },
            ) => *current == *hold_id,
            _ => false,
        }
    }
}

impl DepositEvent {
    pub const fn code(&self) -> u8 {
        match self {
            Self::FundingSucceeded { .. } => 0,
            Self::FundingAmbiguous { .. } => 1,
            Self::FundingFailed { .. } => 2,
            Self::CommitAuthorization { .. } => 3,
            Self::MarkRefundAvailable { .. } => 4,
            Self::AuthorizationSigned { .. } => 5,
            Self::MintReconciled { .. } => 6,
            Self::StartRefund { .. } => 7,
            Self::RefundSucceeded { .. } => 8,
            Self::RefundAmbiguous { .. } => 9,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::FundingSucceeded { .. } => "funding_succeeded",
            Self::FundingAmbiguous { .. } => "funding_ambiguous",
            Self::FundingFailed { .. } => "funding_failed",
            Self::CommitAuthorization { .. } => "commit_authorization",
            Self::AuthorizationSigned { .. } => "authorization_signed",
            Self::MarkRefundAvailable { .. } => "mark_refund_available",
            Self::MintReconciled { .. } => "mint_reconciled",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundSucceeded { .. } => "refund_succeeded",
            Self::RefundAmbiguous { .. } => "refund_ambiguous",
        }
    }
}

impl DepositState {
    pub const fn code(&self) -> u8 {
        match self {
            Self::FundingPending => 0,
            Self::EscrowedUnquoted { .. } => 1,
            Self::AuthorizationPending { .. } => 2,
            Self::AuthorizationAvailable { .. } => 3,
            Self::RefundAvailable { .. } => 4,
            Self::FundingReconciliationHold { .. } => 5,
            Self::RefundPending { .. } => 6,
            Self::RefundReconciliationHold { .. } => 7,
            Self::Refunded { .. } => 8,
            Self::Cancelled { .. } => 9,
            Self::Minted { .. } => 10,
        }
    }
}
