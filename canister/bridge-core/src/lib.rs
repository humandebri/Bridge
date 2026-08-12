//! Deterministic domain state for the KINIC–Base Bridge.
//!
//! This crate deliberately has no IC, Candid, storage, clock, or network dependency. Callers
//! provide confirmed external facts as events and persist the returned state atomically.

mod accounting;
mod authorization;
mod deposit;
mod external;
mod kernel;
mod reconciliation;
mod reserve;
mod types;
mod withdrawal;

pub use accounting::{AccountingState, FeeKind};
pub use authorization::{
    MintAuthorization, MintAuthorizationDomain, MintAuthorizationOrigin, MintAuthorizationRecord,
    MintExpiryEvidence, MintFinalizationEvidence, MINT_AUTHORIZATION_DOMAIN_NAME,
    MINT_AUTHORIZATION_DOMAIN_VERSION, MINT_AUTHORIZATION_TTL_SECONDS,
};
pub use deposit::{
    DepositEvent, DepositQuote, DepositRecord, DepositRefundReason, DepositRequest, DepositState,
};
pub use external::{
    ExternalProgress, FinalizedObservationRecord, GovernanceCallIntent,
    GovernanceTransactionEnvelope, LedgerCallOutcome, LedgerFailure, ReconciliationArchiveRange,
    ReconciliationLedgerPage, ReconciliationScanPhase, ReconciliationScanProgress,
    ReconciliationTarget, SignedGovernanceTransaction,
};
pub use kernel::{
    administrator_authorized, audit_next, authorization_commit_allowed, canonical_probe_matches,
    checked_counter_transition, checked_requirement, committed_quote_matches, counter_delta,
    deposit_admission_decision, deposit_charge_service_fee, deposit_identity_decision,
    deposit_ledger_block_transition, deposit_nonterminal_indexed, deposit_numeric_effects,
    deposit_refund_amount, deposit_releases_reservation, deposit_reservation_active,
    deposit_transition, deposit_transition_decision, deposit_transition_effects, evidence_matches,
    expiry_refund_allowed, fee_recipient_rotation_allowed, fee_recipient_rotation_decision,
    funding_attempt_decision, funding_reconciliation_decision, hold_resolution_decision,
    hold_retry_allowed, lease_generation_next, lease_lane_claim_decision, lease_outcome_decision,
    lease_outcome_is_current, manual_claim_allowed, manual_claim_decision, mint_admission_total,
    mint_finalization_allowed, next_attempt, notification_admission_allowed,
    notification_ingestion_allowed, outbound_settlement, payout_allowed, payout_debit,
    payout_decision, reconciliation_hold_indexed, refresh_generation_next, refresh_owner_matches,
    refund_request_identity_decision, refund_start_allowed, release_transfer_matches,
    replay_matches, reservation_decision, reserve_admission_preserves_requirement,
    runtime_attestation_matches, scan_complete, service_fee_change_allowed, settlement_decision,
    signature_install_allowed, transaction_liability_wei, withdrawal_ledger_block_transition,
    withdrawal_liability_indexed, withdrawal_phase_allows, withdrawal_phase_step,
    withdrawal_transition_effects, DepositAdmissionDecision, DepositEffects, DepositEventGuard,
    DepositIdentityDecision, DepositTransitionDecision, DepositTransitionInput,
    FeeRecipientRotationDecision, FundingAttemptDecision, FundingReconciliationDecision,
    HoldResolutionDecision, LeaseLaneClaimDecision, LeaseOutcomeDecision, ManualClaimDecision,
    PayoutDecision, RefundRequestIdentityDecision, ReservationDecision, SettlementDecision,
};
pub use reconciliation::{
    resolve_deposit_hold, resolve_withdrawal_hold, DepositHoldResolution, ReconciliationHoldRecord,
    ReconciliationHoldState, RequestReference, WithdrawalHoldResolution,
};
pub use reserve::{ReservePolicy, ReserveSnapshot};
pub use types::{
    Account, Amount, ApplyOutcome, ApplyResult, BaseMintSnapshot, CoreError,
    DepositAccountingEffects, DepositId, GovernanceOperationId, HoldId, LedgerOperation,
    LedgerTransferIdentity, Settlement, WithdrawalId,
};
pub use withdrawal::{TransferAttempt, WithdrawalEvent, WithdrawalRecord, WithdrawalState};
