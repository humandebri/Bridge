//! Deterministic domain state for the KINIC–Base Bridge.
//!
//! This crate deliberately has no IC, Candid, storage, clock, or network dependency. Callers
//! provide finalized external facts as events and persist the returned state atomically.

mod accounting;
mod deposit;
mod evm;
mod reconciliation;
mod types;
mod withdrawal;

pub use accounting::{AccountingState, FeeKind, ResourceBudget, ResourceCost};
pub use deposit::{DepositEvent, DepositRecord, DepositRequest, DepositState};
pub use evm::{EvmOperationEvent, EvmOperationKind, EvmOperationRecord, EvmOperationState};
pub use reconciliation::{
    resolve_deposit_hold, resolve_withdrawal_hold, HoldResolution, ReconciliationHoldRecord,
    ReconciliationHoldState, RequestReference,
};
pub use types::{
    Account, Amount, ApplyOutcome, ApplyResult, BaseMintSnapshot, CoreError, DepositId,
    EvmOperationId, HoldId, LedgerOperation, LedgerTransferIdentity, Settlement, WithdrawalId,
};
pub use withdrawal::{WithdrawalEvent, WithdrawalRecord, WithdrawalState};
