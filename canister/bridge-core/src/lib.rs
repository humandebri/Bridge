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
pub use deposit::{
    DepositEvent, DepositQuote, DepositRecord, DepositRefundReason, DepositRequest, DepositState,
};
pub use evm::{
    EvmOperationEvent, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    EvmRecoveryResolution,
};
pub use external::{
    EvmCallIntent, EvmFeeQuote, EvmTransactionEnvelope, ExternalProgress, FinalizedObservationRecord,
    LedgerCallOutcome, LedgerFailure, ReconciliationArchiveRange, ReconciliationLedgerPage,
    ReconciliationScanPhase, ReconciliationScanProgress, ReconciliationTarget,
};
pub use kernel::{
    administrator_authorized, audit_next, can_assign_nonce, canonical_probe_matches,
    checked_counter_transition, checked_requirement, committed_quote_matches, counter_delta,
    deposit_admission_decision, deposit_phase_allows, deposit_phase_step, deposit_refund_amount,
    evidence_matches, evm_operation_indexed, fee_delta_once, fee_recipient_rotation_allowed,
    fee_recipient_rotation_decision, funding_attempt_decision, hold_resolution_decision,
    hold_retry_allowed, lease_generation_next, lease_lane_claim_decision, lease_outcome_decision,
    lease_outcome_is_current, manual_claim_allowed, manual_claim_decision, mint_admission_total,
    monotone, next_attempt, nonce_next, nonce_too_low_is_submitted, notification_admission_allowed,
    outbound_settlement, payout_allowed, payout_debit, payout_decision,
    reconciliation_hold_indexed, refresh_generation_next, refresh_owner_matches,
    release_transfer_matches, replay_matches, reservation_decision,
    reserve_admission_preserves_requirement, reserve_token_matches, resources_sufficient,
    restored_pending_blocked, scan_complete, service_fee_change_allowed, settlement_decision,
    withdrawal_finalization_decision, withdrawal_liability_indexed, withdrawal_phase_allows,
    withdrawal_phase_step, DepositAdmissionDecision, FeeRecipientRotationDecision,
    FundingAttemptDecision, HoldResolutionDecision, LeaseLaneClaimDecision, LeaseOutcomeDecision,
    ManualClaimDecision, PayoutDecision, ReservationDecision, SettlementDecision,
    WithdrawalFinalizationDecision,
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
