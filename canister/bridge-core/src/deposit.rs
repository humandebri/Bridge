use crate::{
    Amount, ApplyResult, BaseMintSnapshot, CoreError, DepositId, EvmOperationId, HoldId,
    LedgerOperation, LedgerTransferIdentity, ResourceBudget, ResourceCost,
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
    ReconciliationHold {
        hold_id: HoldId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositEvent {
    PullSucceeded { ledger_block_index: u128 },
    PullAmbiguous { hold_id: HoldId },
    PrepareMint { operation_id: EvmOperationId },
    MintFinalized { operation_id: EvmOperationId },
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
    pub service_fee: Amount,
    pub net_amount: Amount,
    pub transfer: LedgerTransferIdentity,
    pub state: DepositState,
}

impl DepositRecord {
    pub fn accept(
        request: DepositRequest,
        base: BaseMintSnapshot,
        resources: ResourceBudget,
        deposit_cost: ResourceCost,
    ) -> Result<Self, CoreError> {
        if request.transfer.operation != LedgerOperation::PullDeposit {
            return Err(CoreError::InvalidLedgerOperation);
        }
        if request.transfer.amount != request.gross_amount {
            return Err(CoreError::InvalidAmount);
        }
        resources.ensure_deposit_can_reserve(deposit_cost)?;
        let net_amount = base.quote(request.gross_amount, request.user_max_service_fee)?;
        Ok(Self {
            id: request.id,
            payload_hash: request.payload_hash,
            gross_amount: request.gross_amount,
            service_fee: base.service_fee,
            net_amount,
            transfer: request.transfer,
            state: DepositState::PullPending,
        })
    }

    pub fn verify_retry(&self, payload_hash: [u8; 32]) -> Result<(), CoreError> {
        if self.payload_hash != payload_hash {
            return Err(CoreError::PayloadConflict);
        }
        Ok(())
    }

    pub fn apply(&mut self, event: DepositEvent) -> Result<ApplyResult, CoreError> {
        use DepositEvent as Event;
        use DepositState as State;

        if self.is_idempotent(event) {
            return Ok(ApplyResult::idempotent());
        }

        let (next, fee_delta) = match (&self.state, event) {
            (State::PullPending, Event::PullSucceeded { ledger_block_index }) => {
                (State::Escrowed { ledger_block_index }, Amount::ZERO)
            }
            (State::PullPending, Event::PullAmbiguous { hold_id }) => {
                (State::ReconciliationHold { hold_id }, Amount::ZERO)
            }
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
                Event::MintFinalized { operation_id },
            ) if *current == operation_id => (
                State::Minted {
                    ledger_block_index: *ledger_block_index,
                    operation_id,
                },
                self.service_fee,
            ),
            (State::MintPending { .. }, Event::MintFinalized { .. }) => {
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

    fn is_idempotent(&self, event: DepositEvent) -> bool {
        use DepositEvent as Event;
        use DepositState as State;
        match (&self.state, event) {
            (
                State::Escrowed {
                    ledger_block_index: current,
                },
                Event::PullSucceeded { ledger_block_index },
            ) => *current == ledger_block_index,
            (State::ReconciliationHold { hold_id: current }, Event::PullAmbiguous { hold_id }) => {
                *current == hold_id
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
                Event::MintFinalized { operation_id },
            ) => *current == operation_id,
            _ => false,
        }
    }
}

impl DepositEvent {
    const fn name(self) -> &'static str {
        match self {
            Self::PullSucceeded { .. } => "pull_succeeded",
            Self::PullAmbiguous { .. } => "pull_ambiguous",
            Self::PrepareMint { .. } => "prepare_mint",
            Self::MintFinalized { .. } => "mint_finalized",
        }
    }
}
