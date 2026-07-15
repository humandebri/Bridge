use crate::{
    Amount, ApplyResult, BaseMintSnapshot, CoreError, DepositId, EvmOperationId, HoldId,
    LedgerFailure, LedgerOperation, LedgerTransferIdentity,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositState {
    PullPending,
    Escrowed {
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
    ReconciliationHold {
        hold_id: HoldId,
    },
    Cancelled {
        hold_id: Option<HoldId>,
        history_watermark: Option<u128>,
        ledger_failure: Option<LedgerFailure>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositEvent {
    PullSucceeded { ledger_block_index: u128 },
    PullAmbiguous { hold_id: HoldId },
    PullFailed { code: LedgerFailure },
    PrepareMint { operation_id: EvmOperationId },
    MintConfirmed { operation_id: EvmOperationId },
    MintReverted { operation_id: EvmOperationId },
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
    pub service_fee: Amount,
    pub net_amount: Amount,
    pub transfer: LedgerTransferIdentity,
    pub state: DepositState,
    pub last_settlement_stop_reason: Option<String>,
}

impl DepositRecord {
    pub fn accept(request: DepositRequest, base: BaseMintSnapshot) -> Result<Self, CoreError> {
        if request.transfer.operation != LedgerOperation::PullDeposit {
            return Err(CoreError::InvalidLedgerOperation);
        }
        if request.transfer.amount != request.gross_amount {
            return Err(CoreError::InvalidAmount);
        }
        let net_amount = base.quote(request.gross_amount, request.user_max_service_fee)?;
        Ok(Self {
            id: request.id,
            payload_hash: request.payload_hash,
            gross_amount: request.gross_amount,
            max_service_fee: request.user_max_service_fee,
            service_fee: base.service_fee,
            net_amount,
            transfer: request.transfer,
            state: DepositState::PullPending,
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
        !matches!(
            self.state,
            DepositState::Minted { .. }
                | DepositState::MintReverted { .. }
                | DepositState::Cancelled { .. }
        )
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

        let (next, fee_delta) = match (&self.state, event) {
            (State::PullPending, Event::PullSucceeded { ledger_block_index }) => {
                (State::Escrowed { ledger_block_index }, Amount::ZERO)
            }
            (State::PullPending, Event::PullAmbiguous { hold_id }) => {
                (State::ReconciliationHold { hold_id }, Amount::ZERO)
            }
            (State::PullPending, Event::PullFailed { code }) => (
                State::Cancelled {
                    hold_id: None,
                    history_watermark: None,
                    ledger_failure: Some(code),
                },
                Amount::ZERO,
            ),
            (State::Escrowed { ledger_block_index }, Event::PrepareMint { operation_id }) => (
                State::MintPending {
                    ledger_block_index: *ledger_block_index,
                    operation_id,
                },
                Amount::ZERO,
            ),
            (
                State::MintPending {
                    ledger_block_index,
                    operation_id: current,
                },
                Event::MintConfirmed { operation_id },
            ) if *current == operation_id => (
                State::Minted {
                    ledger_block_index: *ledger_block_index,
                    operation_id,
                },
                Amount::new(crate::fee_delta_once(false, true, self.service_fee.get())),
            ),
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
                Amount::ZERO,
            ),
            (State::MintPending { .. }, Event::MintReverted { .. }) => {
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
        Ok(ApplyResult::applied(fee_delta))
    }

    fn is_idempotent(&self, event: &DepositEvent) -> bool {
        use DepositEvent as Event;
        use DepositState as State;
        match (&self.state, event) {
            (
                State::Escrowed {
                    ledger_block_index: current,
                },
                Event::PullSucceeded { ledger_block_index },
            ) => *current == *ledger_block_index,
            (State::ReconciliationHold { hold_id: current }, Event::PullAmbiguous { hold_id }) => {
                *current == *hold_id
            }
            (
                State::MintPending {
                    operation_id: current,
                    ..
                },
                Event::PrepareMint { operation_id },
            )
            | (
                State::Minted {
                    operation_id: current,
                    ..
                },
                Event::PrepareMint { operation_id },
            )
            | (
                State::Minted {
                    operation_id: current,
                    ..
                },
                Event::MintConfirmed { operation_id },
            ) => *current == *operation_id,
            (
                State::Cancelled {
                    hold_id: None,
                    history_watermark: None,
                    ledger_failure: Some(current),
                },
                Event::PullFailed { code },
            ) => current == code,
            (
                State::MintReverted {
                    operation_id: current,
                    ..
                },
                Event::MintReverted { operation_id },
            ) => *current == *operation_id,
            _ => false,
        }
    }
}

impl DepositState {
    const fn phase(&self) -> u8 {
        match self {
            Self::PullPending => 0,
            Self::Escrowed { .. } => 1,
            Self::MintPending { .. } => 2,
            Self::Minted { .. } => 3,
            Self::MintReverted { .. } => 4,
            Self::ReconciliationHold { .. } => 5,
            Self::Cancelled { .. } => 6,
        }
    }
}

impl DepositEvent {
    const fn phase(&self) -> u8 {
        match self {
            Self::PullSucceeded { .. } => 0,
            Self::PullAmbiguous { .. } => 1,
            Self::PullFailed { .. } => 2,
            Self::PrepareMint { .. } => 3,
            Self::MintConfirmed { .. } => 4,
            Self::MintReverted { .. } => 5,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::PullSucceeded { .. } => "pull_succeeded",
            Self::PullAmbiguous { .. } => "pull_ambiguous",
            Self::PullFailed { .. } => "pull_failed",
            Self::PrepareMint { .. } => "prepare_mint",
            Self::MintConfirmed { .. } => "mint_confirmed",
            Self::MintReverted { .. } => "mint_reverted",
        }
    }
}
