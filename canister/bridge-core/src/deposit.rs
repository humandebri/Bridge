use crate::{
    Amount, ApplyResult, CoreError, DepositId, EvmOperationId, HoldId, LedgerFailure,
    LedgerOperation, LedgerTransferIdentity, TransferAttempt,
};

pub const MAX_AUTOMATIC_REFUND_FEE_RETRIES: u64 = 3;

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
    ReserveInsufficient,
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
    MintPending {
        ledger_block_index: u128,
        operation_id: EvmOperationId,
    },
    Minted {
        ledger_block_index: u128,
        operation_id: EvmOperationId,
    },
    MintReverted {
        ledger_block_index: u128,
        operation_id: EvmOperationId,
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
    RefundRecoveryRequired {
        reason: DepositRefundReason,
        attempt: TransferAttempt,
        expected_fee: Amount,
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
    CommitQuote {
        quote: DepositQuote,
        operation_id: EvmOperationId,
    },
    StartRefund {
        reason: DepositRefundReason,
        attempt: Box<TransferAttempt>,
    },
    RefundSucceeded {
        ledger_block_index: u128,
    },
    RefundAmbiguous {
        hold_id: HoldId,
    },
    RefundBadFee {
        expected_fee: Amount,
        next_identity: Option<Box<LedgerTransferIdentity>>,
    },
    ResumeRefund {
        identity: Box<LedgerTransferIdentity>,
    },
    MintConfirmed {
        operation_id: EvmOperationId,
    },
    MintReverted {
        operation_id: EvmOperationId,
    },
    RetryMint {
        reverted_operation_id: EvmOperationId,
        replacement_operation_id: EvmOperationId,
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
        matches!(self.state, DepositState::MintPending { .. })
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
        if !crate::deposit_phase_allows(self.state.phase(), event.phase()) {
            return Err(CoreError::InvalidTransition {
                entity: "deposit",
                event: event.name(),
            });
        }

        let (next, next_quote, fee_delta) = match (&self.state, event) {
            (State::FundingPending, Event::FundingSucceeded { ledger_block_index }) => (
                State::EscrowedUnquoted { ledger_block_index },
                None,
                Amount::ZERO,
            ),
            (State::FundingPending, Event::FundingAmbiguous { hold_id }) => (
                State::FundingReconciliationHold { hold_id },
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
                Amount::ZERO,
            ),
            (
                State::EscrowedUnquoted { ledger_block_index },
                Event::CommitQuote {
                    quote,
                    operation_id,
                },
            ) => {
                quote.validate(self.gross_amount)?;
                (
                    State::MintPending {
                        ledger_block_index: *ledger_block_index,
                        operation_id,
                    },
                    Some(quote),
                    Amount::ZERO,
                )
            }
            (State::EscrowedUnquoted { .. }, Event::StartRefund { reason, attempt }) => {
                self.validate_refund_attempt(&attempt)?;
                (
                    State::RefundPending {
                        reason,
                        attempt: *attempt,
                    },
                    None,
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
                None,
                Amount::ZERO,
            ),
            (State::RefundPending { reason, attempt }, Event::RefundAmbiguous { hold_id }) => (
                State::RefundReconciliationHold {
                    reason: *reason,
                    hold_id,
                    attempt: attempt.clone(),
                },
                None,
                Amount::ZERO,
            ),
            (
                State::RefundPending { reason, attempt },
                Event::RefundBadFee {
                    expected_fee,
                    next_identity,
                },
            ) => {
                if let Some(next_identity) = next_identity {
                    if attempt.attempt_no >= MAX_AUTOMATIC_REFUND_FEE_RETRIES {
                        return Err(CoreError::RefundIneligible);
                    }
                    let next_attempt = attempt.retry_after_bad_fee(*next_identity, expected_fee)?;
                    if next_attempt.identity.amount.checked_add(expected_fee)?
                        != self.gross_amount
                    {
                        return Err(CoreError::RefundIneligible);
                    }
                    (
                        State::RefundPending {
                            reason: *reason,
                            attempt: next_attempt,
                        },
                        None,
                        Amount::ZERO,
                    )
                } else {
                    (
                        State::RefundRecoveryRequired {
                            reason: *reason,
                            attempt: attempt.clone(),
                            expected_fee,
                        },
                        None,
                        Amount::ZERO,
                    )
                }
            }
            (
                State::RefundRecoveryRequired {
                    reason,
                    attempt,
                    expected_fee,
                },
                Event::ResumeRefund { identity },
            ) => {
                let next = attempt.retry_after_bad_fee(*identity, *expected_fee)?;
                let normal_refund =
                    next.identity.amount.checked_add(next.identity.fee)? == self.gross_amount;
                let compensated_refund = next.identity.amount == self.gross_amount;
                if !normal_refund && !compensated_refund {
                    return Err(CoreError::RefundIneligible);
                }
                (
                    State::RefundPending {
                        reason: *reason,
                        attempt: next,
                    },
                    None,
                    Amount::ZERO,
                )
            }
            (
                State::MintPending {
                    ledger_block_index,
                    operation_id: current,
                },
                Event::MintConfirmed { operation_id },
            ) if *current == operation_id => {
                let service_fee = self.quote.ok_or(CoreError::InvalidAmount)?.service_fee;
                (
                    State::Minted {
                        ledger_block_index: *ledger_block_index,
                        operation_id,
                    },
                    self.quote,
                    Amount::new(crate::fee_delta_once(false, true, service_fee.get())),
                )
            }
            (State::MintPending { .. }, Event::MintConfirmed { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::MintPending {
                    ledger_block_index,
                    operation_id: current,
                },
                Event::MintReverted { operation_id },
            ) if *current == operation_id => (
                State::MintReverted {
                    ledger_block_index: *ledger_block_index,
                    operation_id,
                },
                self.quote,
                Amount::ZERO,
            ),
            (State::MintPending { .. }, Event::MintReverted { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (
                State::MintReverted {
                    ledger_block_index,
                    operation_id: current,
                },
                Event::RetryMint {
                    reverted_operation_id,
                    replacement_operation_id,
                },
            ) if *current == reverted_operation_id => (
                State::MintPending {
                    ledger_block_index: *ledger_block_index,
                    operation_id: replacement_operation_id,
                },
                self.quote,
                Amount::ZERO,
            ),
            (State::MintReverted { .. }, Event::RetryMint { .. }) => {
                return Err(CoreError::ConflictingReplay);
            }
            (_, other) => {
                return Err(CoreError::InvalidTransition {
                    entity: "deposit",
                    event: other.name(),
                });
            }
        };
        self.state = next;
        self.quote = next_quote;
        Ok(ApplyResult::applied(fee_delta))
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
                State::MintPending {
                    operation_id: current,
                    ..
                },
                Event::CommitQuote {
                    quote,
                    operation_id,
                },
            ) => *current == *operation_id && self.quote == Some(*quote),
            (
                State::RefundPending {
                    reason: current_reason,
                    attempt: current_attempt,
                },
                Event::StartRefund { reason, attempt },
            ) => current_reason == reason && current_attempt == attempt.as_ref(),
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
            (
                State::RefundPending { attempt, .. },
                Event::RefundBadFee {
                    expected_fee,
                    next_identity: Some(next),
                },
            ) => {
                attempt.identity == **next && attempt.identity.fee == *expected_fee
            }
            (
                State::RefundRecoveryRequired {
                    attempt,
                    expected_fee: current,
                    ..
                },
                Event::RefundBadFee {
                    expected_fee,
                    next_identity: None,
                },
            ) => current == expected_fee && attempt.identity.fee != *expected_fee,
            (
                State::RefundPending { attempt, .. },
                Event::ResumeRefund { identity },
            ) => attempt.identity == **identity,
            (
                State::Minted {
                    operation_id: current,
                    ..
                },
                Event::MintConfirmed { operation_id },
            ) => *current == *operation_id,
            (
                State::MintReverted {
                    operation_id: current,
                    ..
                },
                Event::MintReverted { operation_id },
            ) => *current == *operation_id,
            (
                State::MintPending {
                    operation_id: current,
                    ..
                },
                Event::RetryMint {
                    replacement_operation_id,
                    ..
                },
            ) => current == replacement_operation_id,
            _ => false,
        }
    }
}

impl DepositState {
    const fn phase(&self) -> u8 {
        match self {
            Self::FundingPending => 0,
            Self::EscrowedUnquoted { .. } => 1,
            Self::MintPending { .. } => 2,
            Self::Minted { .. } => 3,
            Self::MintReverted { .. } => 4,
            Self::FundingReconciliationHold { .. } => 5,
            Self::RefundPending { .. } => 6,
            Self::RefundReconciliationHold { .. } => 7,
            Self::RefundRecoveryRequired { .. } => 8,
            Self::Refunded { .. } => 9,
            Self::Cancelled { .. } => 10,
        }
    }
}

impl DepositEvent {
    const fn phase(&self) -> u8 {
        match self {
            Self::FundingSucceeded { .. } => 0,
            Self::FundingAmbiguous { .. } => 1,
            Self::FundingFailed { .. } => 2,
            Self::CommitQuote { .. } => 3,
            Self::StartRefund { .. } => 4,
            Self::RefundSucceeded { .. } => 5,
            Self::RefundAmbiguous { .. } => 6,
            Self::RefundBadFee { .. } => 7,
            Self::ResumeRefund { .. } => 8,
            Self::MintConfirmed { .. } => 9,
            Self::MintReverted { .. } => 10,
            Self::RetryMint { .. } => 11,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::FundingSucceeded { .. } => "funding_succeeded",
            Self::FundingAmbiguous { .. } => "funding_ambiguous",
            Self::FundingFailed { .. } => "funding_failed",
            Self::CommitQuote { .. } => "commit_quote",
            Self::StartRefund { .. } => "start_refund",
            Self::RefundSucceeded { .. } => "refund_succeeded",
            Self::RefundAmbiguous { .. } => "refund_ambiguous",
            Self::RefundBadFee { .. } => "refund_bad_fee",
            Self::ResumeRefund { .. } => "resume_refund",
            Self::MintConfirmed { .. } => "mint_confirmed",
            Self::MintReverted { .. } => "mint_reverted",
            Self::RetryMint { .. } => "retry_mint",
        }
    }
}
