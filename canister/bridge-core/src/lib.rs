//! Deterministic domain state for the KINIC–Base Bridge.
//!
//! This crate deliberately has no IC, Candid, storage, clock, or network dependency. Callers
//! provide finalized external facts as events and persist the returned state atomically.

mod accounting;
mod deposit;
mod evm;
mod external;
mod kernel;
mod reconciliation;
mod reserve;
mod types;
mod withdrawal;

pub use accounting::{AccountingState, FeeKind};
pub use deposit::{DepositEvent, DepositRecord, DepositRequest, DepositState};
pub use evm::{EvmOperationEvent, EvmOperationKind, EvmOperationRecord, EvmOperationState};
pub use external::{
    EvmCallIntent, EvmTransactionEnvelope, ExternalProgress, LedgerCallOutcome, LedgerFailure,
    ReconciliationScanProgress,
};
pub use kernel::{
    administrator_authorized, audit_next, can_assign_nonce, candidate_precedes,
    checked_requirement, counter_delta, evidence_matches, mint_admission_total, monotone,
    next_attempt, nonce_next, payout_allowed, payout_debit, refund_allowed, replay_matches,
    resources_sufficient, scan_complete, scheduler_priority, terminal_retry_fee,
};
pub use reconciliation::{
    resolve_deposit_hold, resolve_withdrawal_hold, DepositHoldResolution, ReconciliationHoldRecord,
    ReconciliationHoldState, RequestReference, WithdrawalHoldResolution,
};
pub use reserve::{ReservePolicy, ReserveSnapshot};
pub use types::{
    Account, Amount, ApplyOutcome, ApplyResult, BaseMintSnapshot, CoreError, DepositId,
    EvmOperationId, HoldId, LedgerOperation, LedgerTransferIdentity, Settlement, WithdrawalId,
};
pub use withdrawal::{
    RefundEligibility, RefundReason, TransferAttempt, WithdrawalEvent, WithdrawalRecord,
    WithdrawalState,
};
