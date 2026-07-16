//! Deterministic domain state for the KINIC–Base Bridge.
//!
//! This crate deliberately has no IC, Candid, storage, clock, or network dependency. Callers
//! provide confirmed external facts as events and persist the returned state atomically.

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
pub use evm::{
    EvmOperationEvent, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    EvmRecoveryResolution,
};
pub use external::{
    EvmCallIntent, EvmTransactionEnvelope, ExternalProgress, FinalizedObservationRecord,
    LedgerCallOutcome, LedgerFailure, ReconciliationArchiveRange, ReconciliationLedgerPage,
    ReconciliationScanPhase, ReconciliationScanProgress, ReconciliationTarget,
};
pub use kernel::{
    administrator_authorized, audit_next, can_assign_nonce, checked_counter_transition,
    checked_requirement, committed_quote_matches, counter_delta, deposit_phase_allows,
    deposit_phase_step, evidence_matches, fee_delta_once, mint_admission_total, monotone,
    next_attempt, nonce_next, nonce_too_low_is_submitted, payout_allowed, payout_debit,
    release_transfer_matches, replay_matches, resources_sufficient, scan_complete,
    withdrawal_phase_allows, withdrawal_phase_step,
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
pub use withdrawal::{TransferAttempt, WithdrawalEvent, WithdrawalRecord, WithdrawalState};
