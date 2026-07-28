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
        ledger_block_index: u128,
    },
    AuthorizationPending {
        ledger_block_index: u128,
    },
    AuthorizationAvailable {
        ledger_block_index: u128,
    },
    ExpiryReconciliation {
        ledger_block_index: u128,
    },
    Minted {
        ledger_block_index: u128,
    },
    FundingReconciliationHold {
        hold_id: HoldId,
    },
    RefundPending {
        reason: DepositRefundReason,
        attempt: TransferAttempt,
    },
    RefundReconciliationHold {
        reason: DepositRefundReason,
        hold_id: HoldId,
        attempt: TransferAttempt,
    },
    Refunded {
        reason: DepositRefundReason,
        attempt: TransferAttempt,
        ledger_block_index: u128,
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
        ledger_block_index: u128,
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
    },
    BeginExpiryReconciliation,
    MintReconciled {
        evidence: Box<MintFinalizationEvidence>,
    },
    StartRefund {
        reason: DepositRefundReason,
        attempt: Box<TransferAttempt>,
        expiry_evidence: Option<Box<MintExpiryEvidence>>,
    },
    RefundSucceeded {
        ledger_block_index: u128,
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
        if !crate::replay_matches(self.payload_hash == payload_hash) {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub const fn reserves_mint_resources(&self) -> bool {
        matches!(
            self.state,
            DepositState::AuthorizationPending { .. }
                | DepositState::AuthorizationAvailable { .. }
                | DepositState::ExpiryReconciliation { .. }
        )
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

        if self.is_idempotent(&event) {
            return Ok(ApplyResult::idempotent());
        }

        let (
            next_state,
            next_quote,
            next_authorization,
            next_finalization_evidence,
            next_expiry_evidence,
            fee_delta,
        ) = match (&self.state, event) {
            (State::FundingPending, Event::FundingSucceeded { ledger_block_index }) => (
                State::EscrowedUnquoted { ledger_block_index },
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
                State::EscrowedUnquoted { ledger_block_index },
                Event::CommitAuthorization {
                    quote,
                    authorization,
                },
            ) => {
                quote.validate(self.gross_amount)?;
                self.validate_authorization(&authorization, quote)?;
                (
                    State::AuthorizationPending {
                        ledger_block_index: *ledger_block_index,
                    },
                    Some(quote),
                    Some(*authorization),
                    None,
                    None,
                    Amount::ZERO,
                )
            }
            (
                State::AuthorizationPending { ledger_block_index },
                Event::AuthorizationSigned { signature },
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
                        ledger_block_index: *ledger_block_index,
                    },
                    self.quote,
                    Some(authorization),
                    self.mint_finalization_evidence.clone(),
                    self.mint_expiry_evidence.clone(),
                    Amount::ZERO,
                )
            }
            (
                State::AuthorizationPending { ledger_block_index }
                | State::AuthorizationAvailable { ledger_block_index },
                Event::BeginExpiryReconciliation,
            ) => (
                State::ExpiryReconciliation {
                    ledger_block_index: *ledger_block_index,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (
                State::ExpiryReconciliation { ledger_block_index },
                Event::MintReconciled { evidence },
            ) => {
                self.validate_finalization_evidence(&evidence)?;
                let service_fee = self.quote.ok_or(CoreError::InvalidAmount)?.service_fee;
                (
                    State::Minted {
                        ledger_block_index: *ledger_block_index,
                    },
                    self.quote,
                    self.mint_authorization.clone(),
                    Some(*evidence),
                    None,
                    Amount::new(crate::fee_delta_once(false, true, service_fee.get())),
                )
            }
            (
                State::EscrowedUnquoted { .. } | State::ExpiryReconciliation { .. },
                Event::StartRefund {
                    reason,
                    attempt,
                    expiry_evidence,
                },
            ) => {
                self.validate_refund_attempt(&attempt)?;
                match (&self.state, reason, expiry_evidence.as_deref()) {
                    (
                        State::ExpiryReconciliation { .. },
                        DepositRefundReason::AuthorizationExpired,
                        Some(evidence),
                    ) => self.validate_expiry_evidence(evidence)?,
                    (
                        State::EscrowedUnquoted { .. },
                        DepositRefundReason::AuthorizationExpired,
                        _,
                    )
                    | (State::ExpiryReconciliation { .. }, _, _)
                    | (State::EscrowedUnquoted { .. }, _, Some(_)) => {
                        return Err(CoreError::RefundIneligible)
                    }
                    (State::EscrowedUnquoted { .. }, _, None) => {}
                    _ => return Err(CoreError::RefundIneligible),
                }
                (
                    State::RefundPending {
                        reason,
                        attempt: *attempt,
                    },
                    self.quote,
                    self.mint_authorization.clone(),
                    self.mint_finalization_evidence.clone(),
                    expiry_evidence.map(|evidence| *evidence),
                    Amount::ZERO,
                )
            }
            (
                State::RefundPending { reason, attempt },
                Event::RefundSucceeded { ledger_block_index },
            ) => (
                State::Refunded {
                    reason: *reason,
                    attempt: attempt.clone(),
                    ledger_block_index,
                    source_hold: None,
                },
                self.quote,
                self.mint_authorization.clone(),
                self.mint_finalization_evidence.clone(),
                self.mint_expiry_evidence.clone(),
                Amount::ZERO,
            ),
            (State::RefundPending { reason, attempt }, Event::RefundAmbiguous { hold_id }) => (
                State::RefundReconciliationHold {
                    reason: *reason,
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
        self.state = next_state;
        self.quote = next_quote;
        self.mint_authorization = next_authorization;
        self.mint_finalization_evidence = next_finalization_evidence;
        self.mint_expiry_evidence = next_expiry_evidence;
        Ok(ApplyResult::applied(fee_delta))
    }

    fn validate_authorization(
        &self,
        record: &MintAuthorizationRecord,
        quote: DepositQuote,
    ) -> Result<(), CoreError> {
        let authorization = record.authorization;
        if authorization.deposit_id != self.id.bytes()
            || authorization.gross_amount != self.gross_amount
            || authorization.max_service_fee != self.max_service_fee
            || authorization.charged_service_fee != quote.service_fee
            || record.signature.is_some()
            || record.signature_dispatched
            || record.signature_dispatch_attempt != 0
            || record.domain.name != crate::MINT_AUTHORIZATION_DOMAIN_NAME
            || record.domain.version != crate::MINT_AUTHORIZATION_DOMAIN_VERSION
            || record.domain.chain_id == 0
            || record.domain.verifying_contract == [0; 20]
            || record.digest == [0; 32]
            || record.origin.finalized_block_hash == [0; 32]
            || authorization.recipient == [0; 20]
            || authorization.authorization_epoch == 0
            || authorization.deadline
                != record
                    .origin
                    .finalized_block_timestamp
                    .checked_add(crate::MINT_AUTHORIZATION_TTL_SECONDS)
                    .ok_or(CoreError::ArithmeticOverflow)?
        {
            return Err(CoreError::ConflictingReplay);
        }
        Ok(())
    }

    fn validate_refund_attempt(&self, attempt: &TransferAttempt) -> Result<(), CoreError> {
        let identity = &attempt.identity;
        if attempt.attempt_no != 0
            || identity.operation != LedgerOperation::RefundDeposit
            || identity.spender.is_some()
            || identity.from != self.transfer.to
            || identity.to != self.transfer.from
            || identity.amount.checked_add(identity.fee)? != self.gross_amount
        {
            return Err(CoreError::RefundIneligible);
        }
        Ok(())
    }

    fn validate_expiry_evidence(&self, evidence: &MintExpiryEvidence) -> Result<(), CoreError> {
        let authorization = self
            .mint_authorization
            .as_ref()
            .ok_or(CoreError::RefundIneligible)?;
        if evidence.authorization_digest != authorization.digest
            || evidence.chain_id != authorization.domain.chain_id
            || evidence.finalized_block_number < authorization.origin.finalized_block_number
            || evidence.finalized_block_hash == [0; 32]
            || evidence.runtime_sha256 == [0; 32]
            || evidence.rpc_request_digest == [0; 32]
            || evidence.rpc_response_digest == [0; 32]
            || evidence.finalized_block_timestamp <= authorization.authorization.deadline
        {
            return Err(CoreError::RefundIneligible);
        }
        Ok(())
    }

    fn validate_finalization_evidence(
        &self,
        evidence: &MintFinalizationEvidence,
    ) -> Result<(), CoreError> {
        let authorization =
            self.mint_authorization
                .as_ref()
                .ok_or(CoreError::InvalidTransition {
                    entity: "deposit",
                    event: "missing_authorization",
                })?;
        if evidence.authorization_digest != authorization.digest
            || evidence.chain_id != authorization.domain.chain_id
            || evidence.transaction_hash == [0; 32]
            || evidence.receipt_block_number < authorization.origin.finalized_block_number
            || evidence.receipt_block_number > evidence.finalized_block_number
            || evidence.receipt_block_hash == [0; 32]
            || evidence.finalized_block_hash == [0; 32]
            || evidence.rpc_request_digest == [0; 32]
            || evidence.rpc_response_digest == [0; 32]
        {
            return Err(CoreError::ConflictingReplay);
        }
        Ok(())
    }

    fn is_idempotent(&self, event: &DepositEvent) -> bool {
        use DepositEvent as Event;
        use DepositState as State;
        match (&self.state, event) {
            (
                State::EscrowedUnquoted {
                    ledger_block_index: current,
                },
                Event::FundingSucceeded { ledger_block_index },
            ) => *current == *ledger_block_index,
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
                | State::ExpiryReconciliation { .. }
                | State::Minted { .. },
                Event::CommitAuthorization {
                    quote,
                    authorization,
                },
            ) => {
                self.quote == Some(*quote)
                    && self.mint_authorization.as_ref() == Some(authorization)
            }
            (State::AuthorizationAvailable { .. }, Event::AuthorizationSigned { signature }) => {
                self.mint_authorization
                    .as_ref()
                    .and_then(|record| record.signature.as_ref())
                    == Some(signature)
            }
            (State::ExpiryReconciliation { .. }, Event::BeginExpiryReconciliation) => true,
            (State::Minted { .. }, Event::MintReconciled { evidence }) => {
                self.mint_finalization_evidence.as_ref() == Some(evidence)
            }
            (
                State::RefundPending {
                    reason: current_reason,
                    attempt: current_attempt,
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
                    ledger_block_index: current,
                    source_hold: None,
                    ..
                },
                Event::RefundSucceeded { ledger_block_index },
            ) => *current == *ledger_block_index,
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
    const fn name(&self) -> &'static str {
        match self {
            Self::FundingSucceeded { .. } => "funding_succeeded",
            Self::FundingAmbiguous { .. } => "funding_ambiguous",
            Self::FundingFailed { .. } => "funding_failed",
            Self::CommitAuthorization { .. } => "commit_authorization",
            Self::AuthorizationSigned { .. } => "authorization_signed",
            Self::BeginExpiryReconciliation => "begin_expiry_reconciliation",
            Self::MintReconciled { .. } => "mint_reconciled",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundSucceeded { .. } => "refund_succeeded",
            Self::RefundAmbiguous { .. } => "refund_ambiguous",
        }
    }
}
