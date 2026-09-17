# Claim statements and specification correspondence

Proposition types and major definitions interpreted by Lean. Proof bodies are omitted. This is not evidence of semantic agreement with the specification or independent kernel checking.

Toolchain: `leanprover/lean4:v4.30.0`

## claim: activation_preflight

Specification: `docs/canister-state-machine.md`

Premises: Reachable ControlPlane state, not paused, activationCount>0

Conclusion: The last activation is validated

Unproved boundary: Excludes the initial activationCount=0 case. Correctness of external observations is assumed

Evidence and external assumptions: `claims.tsv:activation_preflight` / `claims.tsv:activation_preflight`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.activation_preflight_witness : BridgeSpec.ClaimContracts.ActivationPreflight
```

Major definitions: `BridgeSpec.ClaimContracts.ActivationPreflight`

## claim: authorization_binding

Specification: `docs/adr/0023-use-wallet-funded-eip712-mint-authorization.md`

Premises: Authorization commit or signature installation is accepted

Conclusion: Binds the domain, epoch, and 900-second deadline from IC time; signing requires the u64 range and at least 300 seconds remaining

Unproved boundary: Cryptographic signature authenticity and correspondence between IC time and Nat are external assumptions

Evidence and external assumptions: `claims.tsv:authorization_binding` / `claims.tsv:authorization_binding`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.authorization_binding_witness : BridgeSpec.ClaimContracts.AuthorizationBinding
```

Major definitions: `BridgeSpec.ClaimContracts.AuthorizationBinding`, `BridgeSpec.MintAuthorization.installSignature`, `BridgeSpec.signatureTimeAllowed`, `BridgeSpec.MintAuthorization.Authorization.valid`

## claim: automatic_retry_limit

Specification: `docs/canister-state-machine.md`

Premises: Inputs are the lane, failure count, and limit

Conclusion: Allowed only for the automatic lane with failures<limit

Unproved boundary: Persistence of the failure count and user resumption after stopping require separate evidence

Evidence and external assumptions: `claims.tsv:automatic_retry_limit` / `claims.tsv:automatic_retry_limit`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.automatic_retry_limit_witness : BridgeSpec.ClaimContracts.AutomaticRetryLimit
```

Major definitions: `BridgeSpec.ClaimContracts.AutomaticRetryLimit`

## claim: canonical_probe

Specification: `docs/bridge-flow.md`

Premises: Receipt and snapshot block numbers

Conclusion: The predicate is equivalent to equality of the numbers

Unproved boundary: Number equality alone does not guarantee canonical hash authenticity

Evidence and external assumptions: `claims.tsv:canonical_probe` / `claims.tsv:canonical_probe`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.canonical_probe_witness : BridgeSpec.ClaimContracts.CanonicalProbe
```

Major definitions: `BridgeSpec.ClaimContracts.CanonicalProbe`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: committed_quote

Specification: `docs/canister-state-machine.md`

Premises: Commit accepted, initially Safe, and an accepted trace

Conclusion: The net amount is positive, gross=net+fee, and the terminal recipient and net amount match the initially stored quote

Unproved boundary: A complete mapping from production to Lean traces is unproved

Evidence and external assumptions: `claims.tsv:committed_quote` / `claims.tsv:committed_quote`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.committed_quote_witness : BridgeSpec.ClaimContracts.CommittedQuote
```

Major definitions: `BridgeSpec.ClaimContracts.CommittedQuote`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: confirmed_activation_evidence_binding

Specification: `docs/canister-state-machine.md`

Premises: Inputs are the activation match count, generation, signedAt, and the upgrade completion hook's caller and timestamp

Conclusion: Activation requires a single match and exact metadata equality. For SNS upgrades, the Root completion hook must follow adoption and handoff and must not be in the future

Unproved boundary: IC callback, caller, and timestamp authenticity, absence of concurrent upgrades, and completion of SNS and IC execution are external assumptions

Evidence and external assumptions: `claims.tsv:confirmed_activation_evidence_binding` / `claims.tsv:confirmed_activation_evidence_binding`

Review rationale: Adds a distinction between acceptance of a same-Wasm proposal and completion of post_upgrade. Actual SNS failure and completion are verified separately.

```lean
BridgeSpec.ClaimContracts.confirmed_activation_evidence_binding_witness : BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding
```

Major definitions: `BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding`

## claim: cycles_top_up_request_policy

Specification: `docs/canister-state-machine.md`

Premises: Inputs are balance, threshold, inProgress, and authorized

Conclusion: A request is allowed only when authorized, not in progress, and balance≤threshold

Unproved boundary: Launcher replenishment and future balance recovery are unproved

Evidence and external assumptions: `claims.tsv:cycles_top_up_request_policy` / `claims.tsv:cycles_top_up_request_policy`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.cycles_top_up_request_policy_witness : BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy
```

Major definitions: `BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy`

## claim: deposit_admission

Specification: `docs/canister-state-machine.md`

Premises: admitDeposit returns a net amount

Conclusion: The fee, positive net amount, per-deposit limit, and window limit are satisfied

Unproved boundary: The local model has no independent reserved-amount input. Production admission including reservations has a separate Verus obligation

Evidence and external assumptions: `claims.tsv:deposit_admission` / `claims.tsv:deposit_admission`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.deposit_admission_witness : BridgeSpec.ClaimContracts.DepositAdmission
```

Major definitions: `BridgeSpec.ClaimContracts.DepositAdmission`

## claim: deposit_backing

Specification: `docs/canister-state-machine.md`

Premises: Initial accounting invariant and accepted signature, mint, and refund history

Conclusion: Preserves backing and applies the prescribed accounting deltas for each operation

Unproved boundary: An accounting model phase does not prove actual external operations

Evidence and external assumptions: `claims.tsv:deposit_backing` / `claims.tsv:deposit_backing`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.deposit_backing_witness : BridgeSpec.ClaimContracts.DepositBacking
```

Major definitions: `BridgeSpec.ClaimContracts.DepositBacking`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: deposit_identity_preflight

Specification: `docs/canister-state-machine.md`

Premises: Candidate preflight is accepted in a reachable IdentityHistory state

Conclusion: The candidate ID has not been processed

Unproved boundary: RPC authenticity and install instance uniqueness are external boundaries

Evidence and external assumptions: `claims.tsv:deposit_identity_preflight` / `claims.tsv:deposit_identity_preflight`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.deposit_identity_preflight_witness : BridgeSpec.ClaimContracts.DepositIdentityPreflight
```

Major definitions: `BridgeSpec.ClaimContracts.DepositIdentityPreflight`

## claim: epoch_invalidation

Specification: `docs/canister-state-machine.md`

Premises: An existing authorization or retiredSigner≠replacementSigner

Conclusion: Rejects authorization reissuance and rejects the old signer even in future epochs

Unproved boundary: Signature recovery and EVM execution of epoch updates require separate evidence

Evidence and external assumptions: `claims.tsv:epoch_invalidation` / `claims.tsv:epoch_invalidation`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.epoch_invalidation_witness : BridgeSpec.ClaimContracts.EpochInvalidation
```

Major definitions: `BridgeSpec.ClaimContracts.EpochInvalidation`

## claim: exact_mint_finalization

Specification: `docs/canister-state-machine.md`

Premises: completeMint is accepted

Conclusion: The successful receipt is at or below finalized, and deposit, recipient, and digest match the authorization

Unproved boundary: Nonzero hashes or RPC digests do not prove authenticity. Cryptography and canonicality are external assumptions

Evidence and external assumptions: `claims.tsv:exact_mint_finalization` / `claims.tsv:exact_mint_finalization`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.exact_mint_finalization_witness : BridgeSpec.ClaimContracts.ExactMintFinalization
```

Major definitions: `BridgeSpec.ClaimContracts.ExactMintFinalization`, `BridgeSpec.MintAuthorization.MintEvidence.valid`

## claim: expiry_refund

Specification: `docs/canister-state-machine.md`

Premises: startExpiredRefund is accepted

Conclusion: Unprocessed, matching deposit/digest, strictly past the deadline; preserves accounting backing

Unproved boundary: Finalized evidence authenticity and actual Ledger transfers are external boundaries

Evidence and external assumptions: `claims.tsv:expiry_refund` / `claims.tsv:expiry_refund`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.expiry_refund_witness : BridgeSpec.ClaimContracts.ExpiryRefund
```

Major definitions: `BridgeSpec.ClaimContracts.ExpiryRefund`, `BridgeSpec.MintAuthorization.ExpiryEvidence.valid`

## claim: fee_accounting_once

Specification: `docs/canister-state-machine.md`

Premises: Accepted trace from the initial Deposit and signature installation

Conclusion: At most one fee credit; the fee delta at signing equals the authorization value

Unproved boundary: There is no complete proof connecting GlobalHistory and DepositHistory to production histories

Evidence and external assumptions: `claims.tsv:fee_accounting_once` / `claims.tsv:fee_accounting_once`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.fee_accounting_once_witness : BridgeSpec.ClaimContracts.FeeAccountingOnce
```

Major definitions: `BridgeSpec.ClaimContracts.FeeAccountingOnce`, `BridgeSpec.MintAuthorization.installSignature`, `BridgeSpec.signatureTimeAllowed`, `BridgeSpec.MintAuthorization.Authorization.valid`

## claim: fee_payout

Specification: `docs/canister-state-machine.md`

Premises: feePayoutAllowed is accepted

Conclusion: Pending and new debits fit within the reserve; debit is accounted for only on success

Unproved boundary: Correctness of external transfer results and persistence are external boundaries

Evidence and external assumptions: `claims.tsv:fee_payout` / `claims.tsv:fee_payout`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.fee_payout_witness : BridgeSpec.ClaimContracts.FeePayout
```

Major definitions: `BridgeSpec.ClaimContracts.FeePayout`

## claim: fee_recipient_rotation

Specification: `docs/canister-state-machine.md`

Premises: rotateFeeRecipient is accepted

Conclusion: Pending payout is zero; updates the recipient while preserving balances and previously accounted fees

Unproved boundary: Production authorization and SQL commit require separate evidence

Evidence and external assumptions: `claims.tsv:fee_recipient_rotation` / `claims.tsv:fee_recipient_rotation`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.fee_recipient_rotation_witness : BridgeSpec.ClaimContracts.FeeRecipientRotation
```

Major definitions: `BridgeSpec.ClaimContracts.FeeRecipientRotation`

## claim: funding_attempt_lifecycle

Specification: `docs/canister-state-machine.md`

Premises: Input is a funding outcome enum

Conclusion: Classifies success, duplicate, ambiguous, retryable, and definitive failure into their prescribed decisions

Unproved boundary: Decoding external results into the enum and SQL transactions require separate evidence

Evidence and external assumptions: `claims.tsv:funding_attempt_lifecycle` / `claims.tsv:funding_attempt_lifecycle`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.funding_attempt_lifecycle_witness : BridgeSpec.ClaimContracts.FundingAttemptLifecycle
```

Major definitions: `BridgeSpec.ClaimContracts.FundingAttemptLifecycle`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: funding_reconciliation_freshness

Specification: `docs/canister-state-machine.md`

Premises: Inputs are absence, finalScan, and dedupExpired booleans

Conclusion: Distinguishes conditions requiring a fresh scan from conditions permitting release

Unproved boundary: Complete history, elapsed time, and scan result authenticity are external boundaries

Evidence and external assumptions: `claims.tsv:funding_reconciliation_freshness` / `claims.tsv:funding_reconciliation_freshness`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.funding_reconciliation_freshness_witness : BridgeSpec.ClaimContracts.FundingReconciliationFreshness
```

Major definitions: `BridgeSpec.ClaimContracts.FundingReconciliationFreshness`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: governance_confirmation_authorization

Specification: `docs/canister-state-machine.md`

Premises: The caller and current relayer, governance, and pause principals are represented as Nat

Conclusion: Accepts only a nonzero caller matching one of the three authorized principals

Unproved boundary: Correspondence between Nat zero and the anonymous Principal, and controller retrieval, are production boundaries

Evidence and external assumptions: `claims.tsv:governance_confirmation_authorization` / `claims.tsv:governance_confirmation_authorization`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.governance_confirmation_authorization_witness : BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization
```

Major definitions: `BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization`

## claim: governance_nonce_chain_binding

Specification: `docs/canister-state-machine.md`

Premises: A reachable ControlPlane state has a last governance chain

Conclusion: The governance chain matches the configured chain

Unproved boundary: Does not claim an RPC-observed chainId or resistance to provider chain switching

Evidence and external assumptions: `claims.tsv:governance_nonce_chain_binding` / `claims.tsv:governance_nonce_chain_binding`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.governance_nonce_chain_binding_witness : BridgeSpec.ClaimContracts.GovernanceNonceChainBinding
```

Major definitions: `BridgeSpec.ClaimContracts.GovernanceNonceChainBinding`

## claim: governance_transaction_affordability

Specification: `docs/canister-state-machine.md`

Premises: Decoded finalized and safe balances and a checked required amount

Conclusion: The decision reports their minimum and accepts exactly when both balances cover the required amount, including equality

Unproved boundary: External balance authenticity and fee-estimate adequacy remain assumptions; shared-expression Verus binds the production decision

Evidence and external assumptions: `claims.tsv:governance_transaction_affordability` / `claims.tsv:governance_transaction_affordability`

Review rationale: Bind the conservative minimum and acceptance boundary to the production-shared decision and its negative fixture.

```lean
BridgeSpec.ClaimContracts.governance_transaction_affordability_witness : BridgeSpec.ClaimContracts.GovernanceTransactionAffordability
```

Major definitions: `BridgeSpec.ClaimContracts.GovernanceTransactionAffordability`, `BridgeSpec.ClaimContracts.governanceAffordabilityDecision`

## claim: hold_resolution

Specification: `docs/canister-state-machine.md`

Premises: holdRetryAllowed is accepted

Conclusion: Exact success or complete absence exists

Unproved boundary: Evidence authenticity cannot be derived from input booleans

Evidence and external assumptions: `claims.tsv:hold_resolution` / `claims.tsv:hold_resolution`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.hold_resolution_witness : BridgeSpec.ClaimContracts.HoldResolution
```

Major definitions: `BridgeSpec.ClaimContracts.HoldResolution`

## claim: initial_activation_authorization

Specification: `docs/canister-state-machine.md`

Premises: Inputs classify bootstrap, governance, sealed state, phase, and migration

Conclusion: Specifies bootstrap authorization and consumption, seal caller conditions, and migration classification

Unproved boundary: Unfolding the migration classification definition alone does not prove actual restoration

Evidence and external assumptions: `claims.tsv:initial_activation_authorization` / `claims.tsv:initial_activation_authorization`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.initial_activation_authorization_witness : BridgeSpec.ClaimContracts.InitialActivationAuthorization
```

Major definitions: `BridgeSpec.ClaimContracts.InitialActivationAuthorization`

## claim: lease_lane_isolation

Specification: `docs/canister-state-machine.md`

Premises: The lane claim decision is allow

Conclusion: The target is inactive and below lane capacity

Unproved boundary: Lane input classification and the runtime dispatcher require separate evidence

Evidence and external assumptions: `claims.tsv:lease_lane_isolation` / `claims.tsv:lease_lane_isolation`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.lease_lane_isolation_witness : BridgeSpec.ClaimContracts.LeaseLaneIsolation
```

Major definitions: `BridgeSpec.ClaimContracts.LeaseLaneIsolation`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: lease_outcome

Specification: `docs/canister-state-machine.md`

Premises: The lease outcome predicate and GlobalHistory update are accepted

Conclusion: The lease is active with a matching generation; preserves accounting invariants and other records

Unproved boundary: Async execution, SQL row selection, and external callback effects require separate evidence

Evidence and external assumptions: `claims.tsv:lease_outcome` / `claims.tsv:lease_outcome`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.lease_outcome_witness : BridgeSpec.ClaimContracts.LeaseOutcome
```

Major definitions: `BridgeSpec.ClaimContracts.LeaseOutcome`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: ledger_block_provenance

Specification: `docs/canister-state-machine.md`

Premises: An accepted trace installing Ledger indices

Conclusion: Preserves existing indices, rejects conflicts, and requires a funding index for refunds

Unproved boundary: Historical authenticity of index values and SQL row selection are external assumptions

Evidence and external assumptions: `claims.tsv:ledger_block_provenance` / `claims.tsv:ledger_block_provenance`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.ledger_block_provenance_witness : BridgeSpec.ClaimContracts.LedgerBlockProvenance
```

Major definitions: `BridgeSpec.ClaimContracts.LedgerBlockProvenance`

## claim: nonterminal_deposit_index_consistency

Specification: `docs/canister-state-machine.md`

Premises: Input is a DepositPhase

Conclusion: Only phases other than refunded, cancelled, and minted are indexed

Unproved boundary: SQL index maintenance and actual record scans require separate evidence

Evidence and external assumptions: `claims.tsv:nonterminal_deposit_index_consistency` / `claims.tsv:nonterminal_deposit_index_consistency`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.nonterminal_deposit_index_consistency_witness : BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency
```

Major definitions: `BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency`

## claim: notification_quota_isolation

Specification: `docs/canister-state-machine.md`

Premises: Notification admission and ingestion predicates accept

Conclusion: Below global, caller, and ingestion limits; cooldown requires matching hashes and time before expiry

Unproved boundary: Persistent counters, period updates, and caller authenticity require separate evidence

Evidence and external assumptions: `claims.tsv:notification_quota_isolation` / `claims.tsv:notification_quota_isolation`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.notification_quota_isolation_witness : BridgeSpec.ClaimContracts.NotificationQuotaIsolation
```

Major definitions: `BridgeSpec.ClaimContracts.NotificationQuotaIsolation`

## claim: operational_config_seal

Specification: `docs/canister-state-machine.md`

Premises: Inputs are sealed and candidateValid

Conclusion: Seals only when not already sealed and the candidate is valid; allows asset operations only when sealed

Unproved boundary: Correct construction of boolean inputs by callers and persistence require separate evidence

Evidence and external assumptions: `claims.tsv:operational_config_seal` / `claims.tsv:operational_config_seal`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.operational_config_seal_witness : BridgeSpec.ClaimContracts.OperationalConfigSeal
```

Major definitions: `BridgeSpec.ClaimContracts.OperationalConfigSeal`

## claim: paid_call_cycle_reserve

Specification: `docs/canister-state-machine.md`

Premises: reserve+attachedCycles+margin≤liquid, actual charge≤budget

Conclusion: Preserves the reserve after charging

Unproved boundary: Actual IC charges, refunds, and runtime accounting are external boundaries

Evidence and external assumptions: `claims.tsv:paid_call_cycle_reserve` / `claims.tsv:paid_call_cycle_reserve`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.paid_call_cycle_reserve_witness : BridgeSpec.ClaimContracts.PaidCallCycleReserve
```

Major definitions: `BridgeSpec.ClaimContracts.PaidCallCycleReserve`

## claim: payment_identity

Specification: `docs/canister-state-machine.md`

Premises: A GlobalHistory payout or record update is accepted

Conclusion: The payout net amount and recipient match the record; records with other IDs remain unchanged

Unproved boundary: The callback's paid phase alone is not evidence of an actual transfer

Evidence and external assumptions: `claims.tsv:payment_identity` / `claims.tsv:payment_identity`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.payment_identity_witness : BridgeSpec.ClaimContracts.PaymentIdentity
```

Major definitions: `BridgeSpec.ClaimContracts.PaymentIdentity`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: pending_queue

Specification: `docs/bridge-flow.md`

Premises: An existing blocked entry or a storage write failure

Conclusion: Preserves blocked retries; on write failure, retains the session with no durable result

Unproved boundary: Web Locks and browser storage atomicity are external assumptions

Evidence and external assumptions: `claims.tsv:pending_queue` / `claims.tsv:pending_queue`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.pending_queue_witness : BridgeSpec.ClaimContracts.PendingQueue
```

Major definitions: `BridgeSpec.ClaimContracts.PendingQueue`

## claim: refund_evidence_enforcement

Specification: `docs/canister-state-machine.md`

Premises: An accepted trace from the initial Deposit includes the start of an expired refund

Conclusion: Requires unprocessed status, matching deposit/digest, and strict expiry

Unproved boundary: RPC canonicality and history authenticity are external assumptions

Evidence and external assumptions: `claims.tsv:refund_evidence_enforcement` / `claims.tsv:refund_evidence_enforcement`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.refund_evidence_enforcement_witness : BridgeSpec.ClaimContracts.RefundEvidenceEnforcement
```

Major definitions: `BridgeSpec.ClaimContracts.RefundEvidenceEnforcement`, `BridgeSpec.MintAuthorization.ExpiryEvidence.valid`

## claim: refund_request_authorization

Specification: `docs/canister-state-machine.md`

Premises: requestExpiredRefund is accepted after an accepted prefix

Conclusion: Requires authenticated=true and depositProcessed=false

Unproved boundary: Production evidence establishes that callers cannot change an existing record's recipient or amount

Evidence and external assumptions: `claims.tsv:refund_request_authorization` / `claims.tsv:refund_request_authorization`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.refund_request_authorization_witness : BridgeSpec.ClaimContracts.RefundRequestAuthorization
```

Major definitions: `BridgeSpec.ClaimContracts.RefundRequestAuthorization`

## claim: reservation_commit

Specification: `docs/canister-state-machine.md`

Premises: Reservation arithmetic within finite-width bounds and an accepted reservation release

Conclusion: Preserves the reservation-plus-candidate total, releases to exactly zero, and rejects double release

Unproved boundary: The GlobalHistory release event itself contains no timing evidence

Evidence and external assumptions: `claims.tsv:reservation_commit` / `claims.tsv:reservation_commit`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.reservation_commit_witness : BridgeSpec.ClaimContracts.ReservationCommit
```

Major definitions: `BridgeSpec.ClaimContracts.ReservationCommit`

## claim: reservation_lifecycle

Specification: `docs/canister-state-machine.md`

Premises: A GlobalHistory reservation release event is accepted

Conclusion: Sets the reservation to exactly zero and rejects double release

Unproved boundary: Expiry decisions require separate DepositHistory and production evidence

Evidence and external assumptions: `claims.tsv:reservation_lifecycle` / `claims.tsv:reservation_lifecycle`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.reservation_lifecycle_witness : BridgeSpec.ClaimContracts.ReservationLifecycle
```

Major definitions: `BridgeSpec.ClaimContracts.ReservationLifecycle`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: runtime_attestation_reuse

Specification: `docs/canister-state-machine.md`

Premises: A reachable ControlPlane state has a reuse domain

Conclusion: The reuse domain matches the current install domain

Unproved boundary: Runtime immutability, correctness of warm observations, and persistence/reuse paths require separate evidence

Evidence and external assumptions: `claims.tsv:runtime_attestation_reuse` / `claims.tsv:runtime_attestation_reuse`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.runtime_attestation_reuse_witness : BridgeSpec.ClaimContracts.RuntimeAttestationReuse
```

Major definitions: `BridgeSpec.ClaimContracts.RuntimeAttestationReuse`

## claim: service_fee_maximum

Specification: `docs/canister-state-machine.md`

Premises: Local fee-bound predicates and Deposit signing history

Conclusion: Fee changes stay within fixed bounds; signing fees are accounted for once

Unproved boundary: The pending payout trace bound is a separate reserve property. A fee-configuration mapping for all executions is unproved

Evidence and external assumptions: `claims.tsv:service_fee_maximum` / `claims.tsv:service_fee_maximum`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.service_fee_maximum_witness : BridgeSpec.ClaimContracts.ServiceFeeMaximum
```

Major definitions: `BridgeSpec.ClaimContracts.ServiceFeeMaximum`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: settlement_backing

Specification: `docs/canister-state-machine.md`

Premises: Initial accounting invariant and accepted history/payout

Conclusion: Preserves backing and applies prescribed deltas to escrow, feeReserve, and outstanding liabilities

Unproved boundary: Actual Ledger transfers and SQL atomicity are external boundaries

Evidence and external assumptions: `claims.tsv:settlement_backing` / `claims.tsv:settlement_backing`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.settlement_backing_witness : BridgeSpec.ClaimContracts.SettlementBacking
```

Major definitions: `BridgeSpec.ClaimContracts.SettlementBacking`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: signing_cycle_reserve

Specification: `docs/canister-state-machine.md`

Premises: reserve+signingCost+margin≤liquid, actual charge≤estimate

Conclusion: Preserves the reserve after billing

Unproved boundary: Correspondence between IC charges and estimates is an external assumption

Evidence and external assumptions: `claims.tsv:signing_cycle_reserve` / `claims.tsv:signing_cycle_reserve`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.signing_cycle_reserve_witness : BridgeSpec.ClaimContracts.SigningCycleReserve
```

Major definitions: `BridgeSpec.ClaimContracts.SigningCycleReserve`

## claim: withdrawal_admission_boundary

Specification: `docs/canister-state-machine.md`

Premises: withdrawalIdAdmissible is accepted

Conclusion: The minimum is nonzero and observed is at least the minimum

Unproved boundary: Correspondence between Nat and 32-byte big-endian IDs is covered by a shared predicate and tests

Evidence and external assumptions: `claims.tsv:withdrawal_admission_boundary` / `claims.tsv:withdrawal_admission_boundary`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.withdrawal_admission_boundary_witness : BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary
```

Major definitions: `BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary`

## claim: withdrawal_finality_quorum

Specification: `docs/bridge-flow.md`

Premises: Selects a checkpoint from three providers' heads or identities

Conclusion: Two providers attest the height; the identity variant requires agreement on both height and hash

Unproved boundary: Correct provider chain configuration and response authenticity are external assumptions

Evidence and external assumptions: `claims.tsv:withdrawal_finality_quorum` / `claims.tsv:withdrawal_finality_quorum`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.withdrawal_finality_quorum_witness : BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum
```

Major definitions: `BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum`

## claim: withdrawal_finalization

Specification: `docs/bridge-flow.md`

Premises: Inputs are receipt success, canonicality, and block

Conclusion: Notify requires success, a block at or below finalized, and canonicality; missing finalized yields retry

Unproved boundary: Does not prove provider responses or the entire browser implementation

Evidence and external assumptions: `claims.tsv:withdrawal_finalization` / `claims.tsv:withdrawal_finalization`

Review rationale: Initial correspondence table. Propositions, dependent definitions, and external boundaries are registered separately.

```lean
BridgeSpec.ClaimContracts.withdrawal_finalization_witness : BridgeSpec.ClaimContracts.WithdrawalFinalization
```

Major definitions: `BridgeSpec.ClaimContracts.WithdrawalFinalization`

## liveness: deposit_terminal_progress_lemmas

Specification: `verification/conditional-liveness.md`

Premises: Continuous admissibility of the target terminal operation, weak fairness, external availability from readyAt onward, and registered user/keeper actions

Conclusion: Conjunction of conditional mint and refund implications, not an either/or guarantee for one shared execution

Unproved boundary: Production scheduling and derivation of admissibility from receipt of funds are unproved. Excluded from release claims

Evidence and external assumptions: `conditional-liveness.tsv:deposit_terminal_progress_lemmas` / `conditional-liveness.tsv:deposit_terminal_progress_lemmas`

Review rationale: Initial correspondence table. Distinguishes conditional lemmas from production reachability.

```lean
BridgeSpec.Liveness.deposit_terminal_progress_lemmas : BridgeSpec.Liveness.DepositTerminalProgressLemmas
```

Major definitions: `BridgeSpec.Liveness.DepositTerminalProgressLemmas`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: expired_deposit_eventually_refunded

Specification: `verification/conditional-liveness.md`

Premises: Continuous admissibility of the target terminal operation, weak fairness, external availability from readyAt onward, and registered user/keeper actions

Conclusion: The target deposit reaches refunded

Unproved boundary: Production scheduling and derivation of admissibility from receipt of funds are unproved. Excluded from release claims

Evidence and external assumptions: `conditional-liveness.tsv:expired_deposit_eventually_refunded` / `conditional-liveness.tsv:expired_deposit_eventually_refunded`

Review rationale: Initial correspondence table. Distinguishes conditional lemmas from production reachability.

```lean
BridgeSpec.Liveness.expired_deposit_eventually_refunded : BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded
```

Major definitions: `BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: funded_deposit_eventually_minted

Specification: `verification/conditional-liveness.md`

Premises: Continuous admissibility of the target terminal operation, weak fairness, external availability from readyAt onward, and registered user/keeper actions

Conclusion: The target deposit reaches minted

Unproved boundary: Production scheduling and derivation of admissibility from receipt of funds are unproved. Excluded from release claims

Evidence and external assumptions: `conditional-liveness.tsv:funded_deposit_eventually_minted` / `conditional-liveness.tsv:funded_deposit_eventually_minted`

Review rationale: Initial correspondence table. Distinguishes conditional lemmas from production reachability.

```lean
BridgeSpec.Liveness.funded_deposit_eventually_minted : BridgeSpec.Liveness.FundedDepositEventuallyMinted
```

Major definitions: `BridgeSpec.Liveness.FundedDepositEventuallyMinted`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: funding_failure_eventually_cancelled

Specification: `verification/conditional-liveness.md`

Premises: Continuous admissibility of the target terminal operation, weak fairness, external availability from readyAt onward, and registered user/keeper actions

Conclusion: The target deposit reaches cancelled

Unproved boundary: Production scheduling and derivation of admissibility from receipt of funds are unproved. Excluded from release claims

Evidence and external assumptions: `conditional-liveness.tsv:funding_failure_eventually_cancelled` / `conditional-liveness.tsv:funding_failure_eventually_cancelled`

Review rationale: Initial correspondence table. Distinguishes conditional lemmas from production reachability.

```lean
BridgeSpec.Liveness.funding_failure_eventually_cancelled : BridgeSpec.Liveness.FundingFailureEventuallyCancelled
```

Major definitions: `BridgeSpec.Liveness.FundingFailureEventuallyCancelled`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: withdrawal_eventually_paid

Specification: `verification/conditional-liveness.md`

Premises: Continuous admissibility of the target terminal operation, weak fairness, external availability from readyAt onward, and registered user/keeper actions

Conclusion: The target withdrawal reaches paid

Unproved boundary: Production scheduling and derivation of admissibility from receipt of funds are unproved. Excluded from release claims

Evidence and external assumptions: `conditional-liveness.tsv:withdrawal_eventually_paid` / `conditional-liveness.tsv:withdrawal_eventually_paid`

Review rationale: Initial correspondence table. Distinguishes conditional lemmas from production reachability.

```lean
BridgeSpec.Liveness.committed_withdrawal_eventually_paid : BridgeSpec.Liveness.WithdrawalEventuallyPaid
```

Major definitions: `BridgeSpec.Liveness.WithdrawalEventuallyPaid`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## Shared definitions

### BridgeSpec.ClaimContracts.ActivationPreflight

specification: The last activation is validated

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ActivationPreflight : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    Eq.{1} (BridgeSpec.ControlPlane.State.paused state) Bool.false →
      GT.gt.{0} (BridgeSpec.ControlPlane.State.activationCount state) 0 →
        Eq.{1} (BridgeSpec.ControlPlane.State.lastActivationValidated state) Bool.true
```

### BridgeSpec.ClaimContracts.AuthorizationBinding

specification: Binds the domain, epoch, and 900-second deadline from IC time; signing requires the u64 range and at least 300 seconds remaining

Specification: `docs/adr/0023-use-wallet-funded-eip712-mint-authorization.md`

```lean
BridgeSpec.ClaimContracts.AuthorizationBinding : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState}
    {authorization : BridgeSpec.MintAuthorization.Authorization}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin},
    Eq.{1} (BridgeSpec.MintAuthorization.commitAuthorization state authorization origin) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization next) (Option.some.{0} authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin)
              BridgeSpec.MintAuthorization.authorizationTtl))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.chainId authorization)
              (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedChainId origin))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedVerifyingContract origin))
              (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.epoch authorization)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedEpoch origin))))))
  (And
    (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {observedTimestamp : Nat},
      Eq.{1} (BridgeSpec.MintAuthorization.installSignature state observedTimestamp) (Option.some.{0} next) →
        Exists.{1} fun authorization =>
          And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
            (And
              (LE.le.{0} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
                BridgeSpec.MintAuthorization.maxU64)
              (And (LE.le.{0} observedTimestamp (HSub.hSub.{0, 0, 0} BridgeSpec.MintAuthorization.maxU64 300))
                (LE.le.{0} (HAdd.hAdd.{0, 0, 0} observedTimestamp 300)
                  (BridgeSpec.MintAuthorization.Authorization.deadline authorization)))))
    BridgeSpec.ClaimContracts.DepositTransitionSafety)
```

### BridgeSpec.ClaimContracts.AutomaticRetryLimit

specification: Allowed only for the automatic lane with failures<limit

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.AutomaticRetryLimit : Prop
∀ (automaticLane : Bool) (failures limit : Nat),
  Iff (Eq.{1} (BridgeSpec.ClaimContracts.automaticRetryAllowed automaticLane failures limit) Bool.true)
    (And (Eq.{1} automaticLane Bool.true) (LT.lt.{0} failures limit))
```

### BridgeSpec.ClaimContracts.CanonicalProbe

specification: The predicate is equivalent to equality of the numbers

Specification: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.CanonicalProbe : Prop
And
  (∀ (receiptBlock snapshotBlock : Nat),
    Iff (Eq.{1} (BridgeSpec.canonicalProbeMatches receiptBlock snapshotBlock) Bool.true)
      (Eq.{1} receiptBlock snapshotBlock))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.CommittedQuote

specification: The net amount is positive, gross=net+fee, and the terminal recipient and net amount match the initially stored quote

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.CommittedQuote : Prop
And
  (∀ {amount serviceFee : Nat} {destination : BridgeSpec.Account} {withdrawal : BridgeSpec.Withdrawal},
    Eq.{1} (BridgeSpec.commit amount serviceFee destination) (Option.some.{0} withdrawal) →
      BridgeSpec.QuoteValid withdrawal)
  (And
    (∀ {state final : BridgeSpec.Protocol.ProtocolState} {events : List.{0} BridgeSpec.Protocol.ProtocolEvent},
      BridgeSpec.Protocol.Safe state →
        BridgeSpec.Protocol.Runs state events final →
          And
            (Eq.{1} (BridgeSpec.Withdrawal.destination (BridgeSpec.Protocol.ProtocolState.withdrawal final))
              (BridgeSpec.Protocol.ProtocolState.committedDestination state))
            (Eq.{1} (BridgeSpec.Withdrawal.amountOut (BridgeSpec.Protocol.ProtocolState.withdrawal final))
              (BridgeSpec.Protocol.ProtocolState.committedAmountOut state)))
    BridgeSpec.ClaimContracts.IntegratedProtocolReachability)
```

### BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding

specification: A single activation match with exact metadata equality, and a Root upgrade completion hook after adoption and handoff

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding : Prop
And
  (∀ (foundMatch foundAdditionalMatch : Bool),
    Iff
      (Eq.{1} (BridgeSpec.ClaimContracts.confirmedActivationAttemptIsUnique foundMatch foundAdditionalMatch) Bool.true)
      (And (Eq.{1} foundMatch Bool.true) (Eq.{1} foundAdditionalMatch Bool.false)))
  (And
    (∀ (confirmedGeneration confirmedSignedAt artifactGeneration artifactSignedAt : Nat),
      Iff
        (Eq.{1}
          (BridgeSpec.ClaimContracts.confirmedActivationMetadataMatches confirmedGeneration confirmedSignedAt
            artifactGeneration artifactSignedAt)
          Bool.true)
        (And (Eq.{1} confirmedGeneration artifactGeneration) (Eq.{1} confirmedSignedAt artifactSignedAt)))
    (∀ (root : Bool) (completed decided handover now : Nat),
      Iff (Eq.{1} (BridgeSpec.ClaimContracts.snsUpgradeCompletionAllowed root completed decided handover now) Bool.true)
        (And (Eq.{1} root Bool.true)
          (And (GT.gt.{0} decided 0)
            (And (GE.ge.{0} completed decided) (And (GE.ge.{0} completed handover) (LE.le.{0} completed now)))))))
```

### BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy

specification: A request is allowed only when authorized, not in progress, and balance≤threshold

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy : Prop
∀ (balance threshold : Nat) (inProgress authorized : Bool),
  Iff
    (Eq.{1} (Bool.and (Bool.and authorized (Bool.not inProgress)) (Decidable.decide (LE.le.{0} balance threshold)))
      Bool.true)
    (And (Eq.{1} authorized Bool.true) (And (Eq.{1} inProgress Bool.false) (LE.le.{0} balance threshold)))
```

### BridgeSpec.ClaimContracts.DepositAdmission

specification: The fee, positive net amount, per-deposit limit, and window limit are satisfied

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositAdmission : Prop
And
  (∀ {admission : BridgeSpec.DepositAdmission} {net : Nat},
    Eq.{1} (BridgeSpec.admitDeposit admission) (Option.some.{0} net) →
      And
        (LE.le.{0} (BridgeSpec.DepositAdmission.serviceFee admission)
          (BridgeSpec.DepositAdmission.maximumServiceFee admission))
        (And
          (LT.lt.{0} (BridgeSpec.DepositAdmission.serviceFee admission)
            (BridgeSpec.DepositAdmission.grossAmount admission))
          (And
            (Eq.{1} net
              (HSub.hSub.{0, 0, 0} (BridgeSpec.DepositAdmission.grossAmount admission)
                (BridgeSpec.DepositAdmission.serviceFee admission)))
            (And (GT.gt.{0} net 0)
              (And (LE.le.{0} net (BridgeSpec.DepositAdmission.perDepositLimit admission))
                (LE.le.{0} (HAdd.hAdd.{0, 0, 0} (BridgeSpec.DepositAdmission.mintedInWindow admission) net)
                  (BridgeSpec.DepositAdmission.mintWindowLimit admission)))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.DepositBacking

specification: Preserves backing and applies the prescribed accounting deltas for each operation

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositBacking : Prop
And
  (∀ {state final : BridgeSpec.GlobalHistory.GlobalState} {events : List.{0} BridgeSpec.GlobalHistory.Event},
    BridgeSpec.GlobalHistory.AccountingInvariant state →
      BridgeSpec.GlobalHistory.Runs state events final →
        BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting final))
  (And
    (∀ {record next : BridgeSpec.GlobalHistory.Record},
      Eq.{1}
          (BridgeSpec.GlobalHistory.applyRecord record
            (BridgeSpec.GlobalHistory.Event.installSignature (BridgeSpec.GlobalHistory.Record.id record)))
          (Option.some.{0} next) →
        And
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic next))
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic record))
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record)))
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
            (HSub.hSub.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
    (And
      (∀ {record next : BridgeSpec.GlobalHistory.Record},
        Eq.{1}
            (BridgeSpec.GlobalHistory.applyRecord record
              (BridgeSpec.GlobalHistory.Event.mint (BridgeSpec.GlobalHistory.Record.id record)))
            (Option.some.{0} next) →
          And
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.baseSupply (BridgeSpec.GlobalHistory.Record.economic next))
              (HAdd.hAdd.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.baseSupply (BridgeSpec.GlobalHistory.Record.economic record))
                (BridgeSpec.GlobalHistory.Record.netAmount record)))
            (Eq.{1}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
                (BridgeSpec.GlobalHistory.Record.netAmount record))))
      (∀ {record next : BridgeSpec.GlobalHistory.Record} {amount : Nat},
        Eq.{1}
            (BridgeSpec.GlobalHistory.applyRecord record
              (BridgeSpec.GlobalHistory.Event.refund (BridgeSpec.GlobalHistory.Record.id record) amount))
            (Option.some.{0} next) →
          And
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic record)) amount))
            (Eq.{1}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
                amount)))))
```

### BridgeSpec.ClaimContracts.DepositIdentityPreflight

specification: The candidate ID has not been processed

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositIdentityPreflight : Prop
∀ {state next : BridgeSpec.IdentityHistory.State} {candidate : Nat},
  BridgeSpec.IdentityHistory.Reachable state →
    Eq.{1} (BridgeSpec.IdentityHistory.preflight state candidate) (Option.some.{0} next) →
      Eq.{1} (state candidate) Bool.false
```

### BridgeSpec.ClaimContracts.EpochInvalidation

specification: Rejects authorization reissuance and rejects the old signer even in future epochs

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.EpochInvalidation : Prop
And
  (∀ {state : BridgeSpec.MintAuthorization.DepositState}
    {current replacement : BridgeSpec.MintAuthorization.Authorization}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin},
    Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} current) →
      Eq.{1} (BridgeSpec.MintAuthorization.commitAuthorization state replacement origin) Option.none.{0})
  (And
    (∀ {authorizationEpoch currentEpoch retiredSigner replacementSigner : Nat},
      Ne.{1} retiredSigner replacementSigner →
        Eq.{1}
          (BridgeSpec.MintAuthorization.evmMintAuthorizationAccepted authorizationEpoch
            (HAdd.hAdd.{0, 0, 0} currentEpoch 1) retiredSigner replacementSigner)
          Bool.false)
    BridgeSpec.ClaimContracts.AuthorizationBinding)
```

### BridgeSpec.ClaimContracts.ExactMintFinalization

specification: The successful receipt is at or below finalized, and deposit, recipient, and digest match the authorization

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ExactMintFinalization : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {evidence : BridgeSpec.MintAuthorization.MintEvidence},
    Eq.{1} (BridgeSpec.MintAuthorization.completeMint state evidence) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptSucceeded evidence) Bool.true)
        (And
          (LE.le.{0} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlock evidence)
            (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlock evidence))
          (Exists.{1} fun authorization =>
            And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.depositId evidence)
                  (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.recipient evidence)
                    (BridgeSpec.MintAuthorization.Authorization.recipient authorization))
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.authorizationDigest evidence)
                    (BridgeSpec.MintAuthorization.Authorization.digest authorization)))))))
  BridgeSpec.ClaimContracts.DepositBacking
```

### BridgeSpec.ClaimContracts.ExpiryRefund

specification: Unprocessed, matching deposit/digest, strictly past the deadline; preserves accounting backing

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ExpiryRefund : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin}
    {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
    Eq.{1} (BridgeSpec.MintAuthorization.startExpiredRefund state origin evidence) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
        (Exists.{1} fun authorization =>
          And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
                (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
                  (BridgeSpec.MintAuthorization.Authorization.digest authorization))
                (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
                  (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))))
  BridgeSpec.ClaimContracts.DepositBacking
```

### BridgeSpec.ClaimContracts.FeeAccountingOnce

specification: At most one fee credit; the fee delta at signing equals the authorization value

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeeAccountingOnce : Prop
And
  (∀ {next : BridgeSpec.Protocol.Deposit.State} {events : List.{0} BridgeSpec.Protocol.Deposit.Event},
    Eq.{1} (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial events) (Option.some.{0} next) →
      LE.le.{0} (BridgeSpec.Protocol.Deposit.traceFeeCreditCount events) 1)
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {observedTimestamp : Nat},
    Eq.{1} (BridgeSpec.MintAuthorization.installSignature state observedTimestamp) (Option.some.{0} next) →
      Exists.{1} fun authorization =>
        And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeReserve next)
              (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.feeReserve state)
                (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization)))
            (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeCounted next) Bool.true)))
```

### BridgeSpec.ClaimContracts.FeePayout

specification: Pending and new debits fit within the reserve; debit is accounted for only on success

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeePayout : Prop
And
  (∀ {reserve pending amount fee : Nat},
    Eq.{1} (BridgeSpec.feePayoutAllowed reserve pending amount fee) Bool.true →
      And (LE.le.{0} pending reserve)
        (And (LE.le.{0} (HAdd.hAdd.{0, 0, 0} amount fee) (HSub.hSub.{0, 0, 0} reserve pending))
          (And (Eq.{1} (BridgeSpec.payoutDebit Bool.false amount fee) 0)
            (Eq.{1} (BridgeSpec.payoutDebit Bool.true amount fee) (HAdd.hAdd.{0, 0, 0} amount fee)))))
  BridgeSpec.ClaimContracts.SettlementBacking
```

### BridgeSpec.ClaimContracts.FeeRecipientRotation

specification: Pending payout is zero; updates the recipient while preserving balances and previously accounted fees

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeeRecipientRotation : Prop
And
  (∀ {state next : BridgeSpec.FeeState} {recipient : Nat},
    Eq.{1} (BridgeSpec.rotateFeeRecipient state recipient) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.FeeState.pendingPayout state) 0)
        (And (Eq.{1} (BridgeSpec.FeeState.reserve next) (BridgeSpec.FeeState.reserve state))
          (And (Eq.{1} (BridgeSpec.FeeState.confirmedDepositFees next) (BridgeSpec.FeeState.confirmedDepositFees state))
            (And
              (Eq.{1} (BridgeSpec.FeeState.confirmedWithdrawalFees next)
                (BridgeSpec.FeeState.confirmedWithdrawalFees state))
              (And (Eq.{1} (BridgeSpec.FeeState.pendingPayout next) 0)
                (Eq.{1} (BridgeSpec.FeeState.recipient next) recipient))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.FundingAttemptLifecycle

specification: Classifies success, duplicate, ambiguous, retryable, and definitive failure into their prescribed decisions

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FundingAttemptLifecycle : Prop
And
  (And
    (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.definitiveFailure)
      BridgeSpec.FundingAttemptDecision.release)
    (And
      (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.success)
        BridgeSpec.FundingAttemptDecision.promoteSuccess)
      (And
        (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.duplicate)
          BridgeSpec.FundingAttemptDecision.promoteSuccess)
        (And
          (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.ambiguous)
            BridgeSpec.FundingAttemptDecision.promoteAmbiguous)
          (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.retryableFailure)
            BridgeSpec.FundingAttemptDecision.retain)))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.FundingReconciliationFreshness

specification: Distinguishes conditions requiring a fresh scan from conditions permitting release

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FundingReconciliationFreshness : Prop
And
  (And
    (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.false Bool.false)
      BridgeSpec.FundingReconciliationDecision.wait)
    (And
      (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.false Bool.true)
        BridgeSpec.FundingReconciliationDecision.wait)
      (And
        (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.true Bool.false)
          BridgeSpec.FundingReconciliationDecision.wait)
        (And
          (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.true Bool.true)
            BridgeSpec.FundingReconciliationDecision.wait)
          (And
            (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.false Bool.false)
              BridgeSpec.FundingReconciliationDecision.restartFresh)
            (And
              (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.false Bool.true)
                BridgeSpec.FundingReconciliationDecision.restartFresh)
              (And
                (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.true Bool.false)
                  BridgeSpec.FundingReconciliationDecision.wait)
                (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.true Bool.true)
                  BridgeSpec.FundingReconciliationDecision.release))))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization

specification: Accepts only a nonzero caller matching one of the three authorized principals

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization : Prop
∀ (caller currentConfirmationRelayer currentGovernance currentPause : Nat),
  Iff
    (Eq.{1}
      (BridgeSpec.ClaimContracts.confirmationCallerAuthorized caller currentConfirmationRelayer currentGovernance
        currentPause)
      Bool.true)
    (And (Ne.{1} caller 0)
      (Or (Eq.{1} caller currentConfirmationRelayer)
        (Or (Eq.{1} caller currentGovernance) (Eq.{1} caller currentPause))))
```

### BridgeSpec.ClaimContracts.GovernanceNonceChainBinding

specification: The governance chain matches the configured chain

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceNonceChainBinding : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    ∀ (chainId : Nat),
      Eq.{1} (BridgeSpec.ControlPlane.State.lastGovernanceChain state) (Option.some.{0} chainId) →
        Eq.{1} chainId (BridgeSpec.ControlPlane.State.configuredChainId state)
```

### BridgeSpec.ClaimContracts.GovernanceTransactionAffordability

specification: The conservative balance decision reports the minimum and accepts exactly when both observed balances cover requiredWei

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceTransactionAffordability : Prop
∀ (finalized safe required : Nat),
  And
    (Eq.{1} (Prod.fst.{0, 0} (BridgeSpec.ClaimContracts.governanceAffordabilityDecision finalized safe required))
      (Min.min.{0} finalized safe))
    (Iff
      (Eq.{1} (Prod.snd.{0, 0} (BridgeSpec.ClaimContracts.governanceAffordabilityDecision finalized safe required))
        Bool.true)
      (And (LE.le.{0} required finalized) (LE.le.{0} required safe)))
```

### BridgeSpec.ClaimContracts.HoldResolution

specification: Exact success or complete absence exists

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.HoldResolution : Prop
And
  (∀ {success absence : Bool},
    Eq.{1} (BridgeSpec.holdRetryAllowed success absence) Bool.true →
      Or (Eq.{1} success Bool.true) (Eq.{1} absence Bool.true))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.InitialActivationAuthorization

specification: Specifies bootstrap authorization and consumption, seal caller conditions, and migration classification

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.InitialActivationAuthorization : Prop
And
  (∀ (bootstrapController governance sealed bootstrapActive validPhase : Bool),
    Iff
      (Eq.{1}
        (BridgeSpec.ClaimContracts.initialActivationAuthorized bootstrapController governance sealed bootstrapActive
          validPhase)
        Bool.true)
      (And (Eq.{1} validPhase Bool.true)
        (And (Eq.{1} sealed Bool.true)
          (ite.{1} (Eq.{1} bootstrapActive Bool.true) (Eq.{1} bootstrapController Bool.true)
            (Eq.{1} governance Bool.true)))))
  (And
    (∀ (authorityPresent confirmedExecute : Bool),
      Iff
        (Eq.{1}
          (BridgeSpec.ClaimContracts.bootstrapActivationAuthorityAfterTransition authorityPresent confirmedExecute)
          Bool.true)
        (And (Eq.{1} authorityPresent Bool.true) (Eq.{1} confirmedExecute Bool.false)))
    (And
      (∀ (controller bootstrap : Bool),
        Iff (Eq.{1} (BridgeSpec.ClaimContracts.operationalConfigSealCallerAuthorized controller bootstrap) Bool.true)
          (And (Eq.{1} controller Bool.true) (Eq.{1} bootstrap Bool.true)))
      (∀ (sealed paused pauseIsOld pauseIsNew markerUnbound markerIsNew rolesDistinct : Bool),
        Eq.{1}
          (BridgeSpec.ClaimContracts.bootstrapPausePrincipalMigrationCode sealed paused pauseIsOld pauseIsNew
            markerUnbound markerIsNew rolesDistinct)
          (ite.{1} (Eq.{1} sealed Bool.true) 2
            (ite.{1} (Eq.{1} (Bool.and pauseIsNew markerIsNew) Bool.true) 1
              (ite.{1} (Eq.{1} (Bool.and (Bool.and (Bool.and paused pauseIsNew) markerUnbound) rolesDistinct) Bool.true)
                4
                (ite.{1}
                  (Eq.{1} (Bool.and (Bool.and (Bool.and paused pauseIsOld) markerUnbound) rolesDistinct) Bool.true) 0
                  3)))))))
```

### BridgeSpec.ClaimContracts.LeaseLaneIsolation

specification: The target is inactive and below lane capacity

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LeaseLaneIsolation : Prop
And
  (∀ {targetActive targetAutomatic : Bool} {activeInLane capacity : Nat},
    Eq.{1} (BridgeSpec.decideLeaseLaneClaim targetActive targetAutomatic activeInLane capacity)
        BridgeSpec.LeaseLaneClaimDecision.allow →
      And (Eq.{1} targetActive Bool.false) (LT.lt.{0} activeInLane capacity))
  BridgeSpec.ClaimContracts.GlobalInterleavingSafety
```

### BridgeSpec.ClaimContracts.LeaseOutcome

specification: The lease is active with a matching generation; preserves accounting invariants and other records

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LeaseOutcome : Prop
And
  (∀ {active : Bool} {currentGeneration outcomeGeneration : Nat},
    Eq.{1} (BridgeSpec.leaseOutcomeCurrent active currentGeneration outcomeGeneration) Bool.true →
      And (Eq.{1} active Bool.true) (Eq.{1} currentGeneration outcomeGeneration))
  BridgeSpec.ClaimContracts.GlobalInterleavingSafety
```

### BridgeSpec.ClaimContracts.LedgerBlockProvenance

specification: Preserves existing indices, rejects conflicts, and requires a funding index for refunds

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LedgerBlockProvenance : Prop
BridgeSpec.LedgerBlockProvenance.ClaimContract
```

### BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency

specification: Only phases other than refunded, cancelled, and minted are indexed

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency : Prop
∀ (phase : BridgeSpec.MintAuthorization.DepositPhase),
  Iff (Eq.{1} (BridgeSpec.MintAuthorization.nonterminalDepositIndexed phase) Bool.true)
    (And (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.refunded)
      (And (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.cancelled)
        (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.minted)))
```

### BridgeSpec.ClaimContracts.NotificationQuotaIsolation

specification: Below global, caller, and ingestion limits; cooldown requires matching hashes and time before expiry

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.NotificationQuotaIsolation : Prop
And
  (∀ {globalCount callerCount globalLimit callerLimit ingestionCount ingestionLimit : Nat},
    Eq.{1} (BridgeSpec.notificationAdmissionAllowed globalCount callerCount globalLimit callerLimit) Bool.true →
      Eq.{1} (BridgeSpec.notificationIngestionAllowed ingestionCount ingestionLimit) Bool.true →
        And (LT.lt.{0} globalCount globalLimit)
          (And (LT.lt.{0} callerCount callerLimit) (LT.lt.{0} ingestionCount ingestionLimit)))
  (And
    (∀ {hashMatches : Bool} {nowNs retryAfterNs : Nat},
      Eq.{1} (BridgeSpec.notificationFailureCooldownActive hashMatches nowNs retryAfterNs) Bool.true →
        And (Eq.{1} hashMatches Bool.true) (LT.lt.{0} nowNs retryAfterNs))
    BridgeSpec.ClaimContracts.IntegratedProtocolReachability)
```

### BridgeSpec.ClaimContracts.OperationalConfigSeal

specification: Seals only when not already sealed and the candidate is valid; allows asset operations only when sealed

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.OperationalConfigSeal : Prop
And
  (∀ (sealed candidateValid : Bool),
    Iff (Eq.{1} (BridgeSpec.ClaimContracts.operationalConfigSealAllowed sealed candidateValid) Bool.true)
      (And (Eq.{1} sealed Bool.false) (Eq.{1} candidateValid Bool.true)))
  (∀ (sealed : Bool),
    Iff (Eq.{1} (BridgeSpec.ClaimContracts.assetOperationsAllowed sealed) Bool.true) (Eq.{1} sealed Bool.true))
```

### BridgeSpec.ClaimContracts.PaidCallCycleReserve

specification: Preserves the reserve after charging

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.PaidCallCycleReserve : Prop
∀ (liquid reserve attachedCycles callMargin charged : Nat),
  LE.le.{0} (HAdd.hAdd.{0, 0, 0} (HAdd.hAdd.{0, 0, 0} reserve attachedCycles) callMargin) liquid →
    LE.le.{0} charged (HAdd.hAdd.{0, 0, 0} attachedCycles callMargin) →
      LE.le.{0} reserve (HSub.hSub.{0, 0, 0} liquid charged)
```

### BridgeSpec.ClaimContracts.PaymentIdentity

specification: The payout net amount and recipient match the record; records with other IDs remain unchanged

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.PaymentIdentity : Prop
And
  (∀ {record next : BridgeSpec.GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.payout (BridgeSpec.GlobalHistory.Record.id record) ledgerFee transferAmount
            destination))
        (Option.some.{0} next) →
      And (Eq.{1} transferAmount (BridgeSpec.GlobalHistory.Record.netAmount record))
        (And (Eq.{1} destination (BridgeSpec.GlobalHistory.Record.paymentDestination record))
          (LE.le.{0} ledgerFee (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
  (∀ {state next : BridgeSpec.GlobalHistory.GlobalState} {event : BridgeSpec.GlobalHistory.Event} {other : Nat},
    Ne.{1} other (BridgeSpec.GlobalHistory.Event.id event) →
      Eq.{1} (BridgeSpec.GlobalHistory.step state event) (Option.some.{0} next) →
        Eq.{1} (BridgeSpec.GlobalHistory.findRecord? (BridgeSpec.GlobalHistory.GlobalState.records next) other)
          (BridgeSpec.GlobalHistory.findRecord? (BridgeSpec.GlobalHistory.GlobalState.records state) other))
```

### BridgeSpec.ClaimContracts.PendingQueue

specification: Preserves blocked retries; on write failure, retains the session with no durable result

Specification: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.PendingQueue : Prop
And
  (∀ {queue : BridgeSpec.PendingQueue} {existing incoming : BridgeSpec.PendingQueueEntry},
    Eq.{1} (BridgeSpec.PendingQueueEntry.blocked existing) Bool.true →
      Eq.{1} (queue (BridgeSpec.PendingQueueEntry.key incoming)) (Option.some.{0} existing) →
        Eq.{1}
          (Option.map.{0, 0} (fun entry => BridgeSpec.PendingQueueEntry.blocked entry)
            (BridgeSpec.restorePendingQueue queue incoming (BridgeSpec.PendingQueueEntry.key incoming)))
          (Option.some.{0} Bool.true))
  (∀ (queue : BridgeSpec.PendingQueue),
    And (Eq.{1} (BridgeSpec.PendingQueueWrite.session (BridgeSpec.recordPendingQueueWrite queue Bool.false)) queue)
      (Eq.{1} (BridgeSpec.PendingQueueWrite.durable (BridgeSpec.recordPendingQueueWrite queue Bool.false))
        Option.none.{0}))
```

### BridgeSpec.ClaimContracts.RefundEvidenceEnforcement

specification: Requires unprocessed status, matching deposit/digest, and strict expiry

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RefundEvidenceEnforcement : Prop
∀ {final : BridgeSpec.Protocol.Deposit.State} {historyPrefix suffix : List.{0} BridgeSpec.Protocol.Deposit.Event}
  {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin} {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
  Eq.{1}
      (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial
        (HAppend.hAppend.{0, 0, 0} historyPrefix
          (List.cons.{0} (BridgeSpec.Protocol.Deposit.Event.startExpiredRefund origin evidence) suffix)))
      (Option.some.{0} final) →
    And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
      (Exists.{1} fun authorization =>
        And
          (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
            (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
              (BridgeSpec.MintAuthorization.Authorization.digest authorization))
            (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
              (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))
```

### BridgeSpec.ClaimContracts.RefundRequestAuthorization

specification: Requires authenticated=true and depositProcessed=false

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RefundRequestAuthorization : Prop
∀ {state next : BridgeSpec.Protocol.Deposit.State} {historyPrefix : List.{0} BridgeSpec.Protocol.Deposit.Event}
  {authenticated : Bool} {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin}
  {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
  Eq.{1} (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial historyPrefix)
      (Option.some.{0} state) →
    Eq.{1} (BridgeSpec.MintAuthorization.requestExpiredRefund authenticated state origin evidence)
        (Option.some.{0} next) →
      And (Eq.{1} authenticated Bool.true)
        (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
```

### BridgeSpec.ClaimContracts.ReservationCommit

specification: Preserves the reservation-plus-candidate total, releases to exactly zero, and rejects double release

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ReservationCommit : Prop
And
  (∀ (reserved candidate : Nat),
    have next := BridgeSpec.commitMintReservation reserved candidate;
    Eq.{1} (HAdd.hAdd.{0, 0, 0} (Prod.fst.{0, 0} next) (Prod.snd.{0, 0} next)) (HAdd.hAdd.{0, 0, 0} reserved candidate))
  BridgeSpec.ClaimContracts.ReservationLifecycle
```

### BridgeSpec.ClaimContracts.ReservationLifecycle

specification: Sets the reservation to exactly zero and rejects double release

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ReservationLifecycle : Prop
And
  (∀ {record next : BridgeSpec.GlobalHistory.Record},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.releaseReservation (BridgeSpec.GlobalHistory.Record.id record)))
        (Option.some.{0} next) →
      Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic next)) 0)
  (∀ {record : BridgeSpec.GlobalHistory.Record},
    Eq.{1} (BridgeSpec.GlobalHistory.Record.reservationReleased record) Bool.true →
      Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal (BridgeSpec.GlobalHistory.Record.phase record)) Bool.false →
        Eq.{1}
          (BridgeSpec.GlobalHistory.applyRecord record
            (BridgeSpec.GlobalHistory.Event.releaseReservation (BridgeSpec.GlobalHistory.Record.id record)))
          Option.none.{0})
```

### BridgeSpec.ClaimContracts.RuntimeAttestationReuse

specification: The reuse domain matches the current install domain

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RuntimeAttestationReuse : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    ∀ (reusedDomain : BridgeSpec.ControlPlane.InstallDomain),
      Eq.{1} (BridgeSpec.ControlPlane.State.lastReusedDomain state) (Option.some.{0} reusedDomain) →
        Eq.{1} reusedDomain (BridgeSpec.ControlPlane.State.domain state)
```

### BridgeSpec.ClaimContracts.ServiceFeeMaximum

specification: Fee changes stay within fixed bounds; signing fees are accounted for once

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ServiceFeeMaximum : Prop
And
  (∀ (serviceFee minimumServiceFee maximumServiceFee : Nat),
    Iff (Eq.{1} (BridgeSpec.serviceFeeChangeAllowed serviceFee minimumServiceFee maximumServiceFee) Bool.true)
      (And (LE.le.{0} minimumServiceFee serviceFee) (LE.le.{0} serviceFee maximumServiceFee)))
  BridgeSpec.ClaimContracts.FeeAccountingOnce
```

### BridgeSpec.ClaimContracts.SettlementBacking

specification: Preserves backing and applies prescribed deltas to escrow, feeReserve, and outstanding liabilities

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.SettlementBacking : Prop
And
  (∀ {state final : BridgeSpec.GlobalHistory.GlobalState} {events : List.{0} BridgeSpec.GlobalHistory.Event},
    BridgeSpec.GlobalHistory.AccountingInvariant state →
      BridgeSpec.GlobalHistory.Runs state events final →
        BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting final))
  (∀ {record next : BridgeSpec.GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.payout (BridgeSpec.GlobalHistory.Record.id record) ledgerFee transferAmount
            destination))
        (Option.some.{0} next) →
      And
        (Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic next))
          (HSub.hSub.{0, 0, 0}
            (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic record))
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record) ledgerFee)))
        (And
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic next))
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic record))
              (HSub.hSub.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.chargedServiceFee record) ledgerFee)))
          (Eq.{1}
            (BridgeSpec.GlobalHistory.Economic.unreleasedLiability (BridgeSpec.GlobalHistory.Record.economic next))
            (HSub.hSub.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.unreleasedLiability (BridgeSpec.GlobalHistory.Record.economic record))
              (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record)
                (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))))
```

### BridgeSpec.ClaimContracts.SigningCycleReserve

specification: Preserves the reserve after billing

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.SigningCycleReserve : Prop
∀ (liquid reserve signingCost callMargin charged : Nat),
  LE.le.{0} (HAdd.hAdd.{0, 0, 0} (HAdd.hAdd.{0, 0, 0} reserve signingCost) callMargin) liquid →
    LE.le.{0} charged (HAdd.hAdd.{0, 0, 0} signingCost callMargin) →
      LE.le.{0} reserve (HSub.hSub.{0, 0, 0} liquid charged)
```

### BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary

specification: The minimum is nonzero and observed is at least the minimum

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary : Prop
∀ {observed minimum : Nat},
  Eq.{1} (BridgeSpec.withdrawalIdAdmissible observed minimum) Bool.true →
    And (Eq.{1} (bne.{0} minimum 0) Bool.true) (LE.le.{0} minimum observed)
```

### BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum

specification: Two providers attest the height; the identity variant requires agreement on both height and hash

Specification: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum : Prop
And
  (∀ {first second third : Option.{0} Nat} {checkpoint : Nat},
    Eq.{1} (BridgeSpec.withdrawalFinalizedCheckpoint first second third) (Option.some.{0} checkpoint) →
      BridgeSpec.twoFinalizedHeadsAttest first second third checkpoint)
  (∀ {first second third : Option.{0} BridgeSpec.FinalizedIdentity} {checkpoint : BridgeSpec.FinalizedIdentity},
    Eq.{1} (BridgeSpec.withdrawalFinalizedIdentityQuorum first second third) (Option.some.{0} checkpoint) →
      BridgeSpec.twoFinalizedIdentitiesAttest first second third checkpoint)
```

### BridgeSpec.ClaimContracts.WithdrawalFinalization

specification: Notify requires success, a block at or below finalized, and canonicality; missing finalized yields retry

Specification: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalFinalization : Prop
And
  (∀ {receiptSucceeded canonical : Bool} {receiptBlock finalizedBlock : Nat},
    Eq.{1}
        (BridgeSpec.decideWithdrawalFinalization receiptSucceeded receiptBlock (Option.some.{0} finalizedBlock)
          canonical)
        BridgeSpec.WithdrawalFinalizationDecision.notify →
      And (Eq.{1} receiptSucceeded Bool.true)
        (And (LE.le.{0} receiptBlock finalizedBlock) (Eq.{1} canonical Bool.true)))
  (∀ {receiptSucceeded canonical : Bool} {receiptBlock : Nat},
    Eq.{1} (BridgeSpec.decideWithdrawalFinalization receiptSucceeded receiptBlock Option.none.{0} canonical)
      BridgeSpec.WithdrawalFinalizationDecision.retry)
```

### BridgeSpec.ClaimContracts.governanceAffordabilityDecision

specification: Returns the smaller observed balance and the inclusive affordability comparison

Specification: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.governanceAffordabilityDecision : Nat → Nat → Nat → Prod.{0, 0} Nat Bool
fun finalized safe required =>
  have observed := Min.min.{0} finalized safe;
  Prod.mk.{0, 0} observed (Decidable.decide (LE.le.{0} required observed))
```

### BridgeSpec.GlobalHistory.AccountingInvariant

model-support: Constrains only ID uniqueness, aggregate agreement, backing, and the record types holding reservations

Specification: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.AccountingInvariant : BridgeSpec.GlobalHistory.GlobalState → Prop
fun state =>
  And (BridgeSpec.GlobalHistory.UniqueIds (BridgeSpec.GlobalHistory.GlobalState.records state))
    (And
      (Eq.{1} (BridgeSpec.GlobalHistory.GlobalState.accounting state)
        (BridgeSpec.GlobalHistory.summarize (BridgeSpec.GlobalHistory.GlobalState.records state)))
      (And (BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting state))
        (BridgeSpec.GlobalHistory.ReservationConsistent (BridgeSpec.GlobalHistory.GlobalState.records state))))
```

### BridgeSpec.GlobalHistory.Backed

specification: escrow=baseSupply+feeReserve+unmintedLiability+unreleasedLiability

Specification: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.Backed : BridgeSpec.GlobalHistory.Economic → Prop
fun economic =>
  Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow economic)
    (HAdd.hAdd.{0, 0, 0}
      (HAdd.hAdd.{0, 0, 0}
        (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Economic.baseSupply economic)
          (BridgeSpec.GlobalHistory.Economic.feeReserve economic))
        (BridgeSpec.GlobalHistory.Economic.unmintedLiability economic))
      (BridgeSpec.GlobalHistory.Economic.unreleasedLiability economic))
```

### BridgeSpec.GlobalHistory.eventDelta

model-support: Selection of accounting events. Callback phases do not authenticate actual Ledger transfers

Specification: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.eventDelta : BridgeSpec.GlobalHistory.Record → BridgeSpec.GlobalHistory.Event → Option.{0} BridgeSpec.GlobalHistory.Delta
fun record event =>
  ite.{1}
    (Or (Ne.{1} (BridgeSpec.GlobalHistory.Event.id event) (BridgeSpec.GlobalHistory.Record.id record))
      (Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal (BridgeSpec.GlobalHistory.Record.phase record)) Bool.true))
    Option.none.{0}
    (match event with
    | BridgeSpec.GlobalHistory.Event.installSignature id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.feeApplied record)) Bool.true)))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.signature (BridgeSpec.GlobalHistory.Record.chargedServiceFee record)))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.mint id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (And
              (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
                0)
              (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.mintApplied record)) Bool.true))))
        (Option.some.{0} (BridgeSpec.GlobalHistory.Delta.mint (BridgeSpec.GlobalHistory.Record.netAmount record)))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.refund id amount =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (And
              (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
                0)
              (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.refundApplied record)) Bool.true))))
        (Option.some.{0} (BridgeSpec.GlobalHistory.Delta.refund amount)) Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.cancel id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.pending)
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
              0)))
        (Option.some.{0} BridgeSpec.GlobalHistory.Delta.none) Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.payout id ledgerFee transferAmount destination =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.withdrawal)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.committed)
            (And (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.payoutApplied record)) Bool.true)
              (And (LE.le.{0} ledgerFee (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))
                (And (Eq.{1} transferAmount (BridgeSpec.GlobalHistory.Record.netAmount record))
                  (Eq.{1} destination (BridgeSpec.GlobalHistory.Record.paymentDestination record)))))))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.payout
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record) ledgerFee)
            (HSub.hSub.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.chargedServiceFee record) ledgerFee)
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record)
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.releaseReservation id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.reservationReleased record)) Bool.true))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.releaseReservation
            (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.callback id generation nextPhase =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.leaseGeneration record) (Option.some.{0} generation))
          (Or (Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal nextPhase) Bool.false)
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
              0)))
        (Option.some.{0} BridgeSpec.GlobalHistory.Delta.none) Option.none.{0})
```

### BridgeSpec.Liveness.AdmissibleUntilOccurs

model-support: Assumes the target terminal event remains admissible until selected

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.AdmissibleUntilOccurs : BridgeSpec.Liveness.Execution → BridgeSpec.GlobalHistory.Event → Nat → Prop
fun execution event start =>
  ∀ (time : Nat),
    LE.le.{0} start time →
      Not (BridgeSpec.Liveness.OccurredBefore execution event start time) →
        Exists.{1} fun next =>
          Eq.{1} (BridgeSpec.GlobalHistory.step (BridgeSpec.Liveness.Execution.state execution time) event)
            (Option.some.{0} next)
```

### BridgeSpec.Liveness.CommonOperationalAssumptions

model-support: Combines continuous availability from readyAt onward with weak fairness

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.CommonOperationalAssumptions : BridgeSpec.Liveness.Execution → Nat → Type
-- Structure or inductive type. Source digests also report field and other changes.
```

### BridgeSpec.Liveness.DepositTerminalProgressLemmas

model-support: Conjunction of conditional mint and refund implications, not an either/or guarantee for one shared execution

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.DepositTerminalProgressLemmas : Prop
And BridgeSpec.Liveness.FundedDepositEventuallyMinted BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded
```

### BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded

model-support: The target deposit reaches refunded

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id amount start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.KeeperActionAssumption execution
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.refund id amount)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.refunded)
        execution start
```

### BridgeSpec.Liveness.FundedDepositEventuallyMinted

model-support: The target deposit reaches minted

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.FundedDepositEventuallyMinted : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.UserActionAssumption execution (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.mint id)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.minted)
        execution start
```

### BridgeSpec.Liveness.FundingFailureEventuallyCancelled

model-support: The target deposit reaches cancelled

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.FundingFailureEventuallyCancelled : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.cancel id)
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.cancelled)
      execution start
```

### BridgeSpec.Liveness.WeakFair

model-support: Assumes a continuously enabled concrete event is eventually selected

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.WeakFair : BridgeSpec.Liveness.Execution → Prop
fun execution =>
  ∀ (event : BridgeSpec.GlobalHistory.Event) (needsUser : Bool) (start : Nat),
    BridgeSpec.Liveness.EnabledUntilOccurs execution event needsUser start →
      Exists.{1} fun time =>
        And (LE.le.{0} start time)
          (Eq.{1} (BridgeSpec.Liveness.Execution.action execution time) (Option.some.{0} event))
```

### BridgeSpec.Liveness.WithdrawalEventuallyPaid

model-support: The target withdrawal reaches paid

Specification: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.WithdrawalEventuallyPaid : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id ledgerFee transferAmount destination start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.KeeperActionAssumption execution
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution
        (BridgeSpec.GlobalHistory.Event.payout id ledgerFee transferAmount destination)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.paid) execution
        start
```

### BridgeSpec.MintAuthorization.Authorization.valid

specification: Matching authorization domain, epoch, amount, and IC-origin deadline. Cryptographic digest computation is excluded

Specification: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.Authorization.valid : BridgeSpec.MintAuthorization.Authorization → BridgeSpec.MintAuthorization.AuthorizationOrigin → Prop
fun authorization origin =>
  And (Ne.{1} (BridgeSpec.MintAuthorization.Authorization.recipient authorization) 0)
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.chainId authorization)
        (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedChainId origin))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization)
          (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedVerifyingContract origin))
        (And (Ne.{1} (BridgeSpec.MintAuthorization.Authorization.digest authorization) 0)
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.epoch authorization)
              (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedEpoch origin))
            (And (Ne.{1} (BridgeSpec.MintAuthorization.AuthorizationOrigin.finalizedHash origin) 0)
              (And
                (Eq.{1}
                  (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.Authorization.netAmount authorization)
                    (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                  (BridgeSpec.MintAuthorization.Authorization.grossAmount authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
                    (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin)
                      BridgeSpec.MintAuthorization.authorizationTtl))
                  (Eq.{1}
                    (BridgeSpec.MintAuthorization.deadlineFromIssuedAt
                      (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin))
                    (Option.some.{0} (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))))))))
```

### BridgeSpec.MintAuthorization.ExpiryEvidence.valid

specification: Matches decoded unprocessed evidence and strict expiry

Specification: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.ExpiryEvidence.valid : BridgeSpec.MintAuthorization.ExpiryEvidence →
  BridgeSpec.MintAuthorization.Authorization → BridgeSpec.MintAuthorization.AuthorizationOrigin → Prop
fun evidence authorization origin =>
  And
    (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
      (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
        (BridgeSpec.MintAuthorization.Authorization.digest authorization))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.chainId evidence)
          (BridgeSpec.MintAuthorization.Authorization.chainId authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.verifyingContract evidence)
            (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization))
          (And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
            (And
              (GE.ge.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedBlock evidence)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.finalizedBlock origin))
              (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedHash evidence) 0)
                (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.runtimeSha256 evidence) 0)
                  (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.rpcRequestDigest evidence) 0)
                    (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.rpcResponseDigest evidence) 0)
                      (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
                        (BridgeSpec.MintAuthorization.Authorization.deadline authorization)))))))))))
```

### BridgeSpec.MintAuthorization.MintEvidence.valid

specification: Matches decoded mint evidence fields. Authenticity of nonzero hashes is an external premise

Specification: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.MintEvidence.valid : BridgeSpec.MintAuthorization.MintEvidence → BridgeSpec.MintAuthorization.Authorization → Prop
fun evidence authorization =>
  And
    (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.depositId evidence)
      (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.recipient evidence)
        (BridgeSpec.MintAuthorization.Authorization.recipient authorization))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.authorizationDigest evidence)
          (BridgeSpec.MintAuthorization.Authorization.digest authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.chainId evidence)
            (BridgeSpec.MintAuthorization.Authorization.chainId authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.verifyingContract evidence)
              (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.grossAmount evidence)
                (BridgeSpec.MintAuthorization.Authorization.grossAmount authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.chargedServiceFee evidence)
                  (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.mintedAmount evidence)
                    (BridgeSpec.MintAuthorization.Authorization.netAmount authorization))
                  (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.transactionHash evidence) 0)
                    (And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptSucceeded evidence) Bool.true)
                      (And
                        (LE.le.{0} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlock evidence)
                          (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlock evidence))
                        (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlockHash evidence) 0)
                          (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlockHash evidence) 0)
                            (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.rpcRequestDigest evidence) 0)
                              (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.rpcResponseDigest evidence) 0)
                                (And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.exactEventCount evidence) 1)
                                  (Eq.{1}
                                    (HAdd.hAdd.{0, 0, 0}
                                      (BridgeSpec.MintAuthorization.Authorization.netAmount authorization)
                                      (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                                    (BridgeSpec.MintAuthorization.Authorization.grossAmount
                                      authorization)))))))))))))))))
```

### BridgeSpec.MintAuthorization.installSignature

specification: Abstract transition checking 300 seconds remaining at signing, finite-width bounds, and fee accounting once

Specification: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.installSignature : BridgeSpec.MintAuthorization.DepositState → Nat → Option.{0} BridgeSpec.MintAuthorization.DepositState
fun state observedTimestamp =>
  match BridgeSpec.MintAuthorization.DepositState.authorization state with
  | Option.none.{0} => Option.none.{0}
  | Option.some.{0} authorization =>
    ite.{1}
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.phase state)
          BridgeSpec.MintAuthorization.DepositPhase.authorizationPending)
        (And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeCounted state) Bool.false)
          (And
            (LE.le.{0} (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization)
              (BridgeSpec.MintAuthorization.DepositState.pendingDepositLiability state))
            (Eq.{1}
              (BridgeSpec.signatureTimeAllowed observedTimestamp
                (BridgeSpec.MintAuthorization.Authorization.deadline authorization))
              Bool.true))))
      (Option.some.{0}
        { phase := BridgeSpec.MintAuthorization.DepositPhase.authorizationAvailable,
          authorization := BridgeSpec.MintAuthorization.DepositState.authorization state,
          escrow := BridgeSpec.MintAuthorization.DepositState.escrow state,
          baseSupply := BridgeSpec.MintAuthorization.DepositState.baseSupply state,
          feeReserve :=
            HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.feeReserve state)
              (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization),
          pendingDepositLiability :=
            HSub.hSub.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.pendingDepositLiability state)
              (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization),
          reservedMint := BridgeSpec.MintAuthorization.DepositState.reservedMint state, feeCounted := Bool.true,
          jobNextRun := BridgeSpec.MintAuthorization.DepositState.jobNextRun state,
          leaseGeneration := BridgeSpec.MintAuthorization.DepositState.leaseGeneration state })
      Option.none.{0}
```

### BridgeSpec.Protocol.Safe

model-support: Accounting, quote, and lease conditions of the integrated single-record model, not overall runtime safety

Specification: `verification/README.md`

```lean
BridgeSpec.Protocol.Safe : BridgeSpec.Protocol.ProtocolState → Prop
fun state =>
  And (BridgeSpec.Backed (BridgeSpec.Protocol.ProtocolState.economic state))
    (And
      (Eq.{1} (BridgeSpec.FeeState.reserve (BridgeSpec.Protocol.ProtocolState.fee state))
        (BridgeSpec.EconomicState.feeReserve (BridgeSpec.Protocol.ProtocolState.economic state)))
      (And
        (LE.le.{0} (BridgeSpec.FeeState.pendingPayout (BridgeSpec.Protocol.ProtocolState.fee state))
          (BridgeSpec.FeeState.reserve (BridgeSpec.Protocol.ProtocolState.fee state)))
        (And
          (LE.le.{0}
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.Protocol.WindowState.consumed (BridgeSpec.Protocol.ProtocolState.window state))
              (BridgeSpec.Protocol.WindowState.reserved (BridgeSpec.Protocol.ProtocolState.window state)))
            BridgeSpec.FiniteWidthModel.maxU128)
          (And
            (Eq.{1}
              (HAdd.hAdd.{0, 0, 0}
                (BridgeSpec.Protocol.DepositTrace.reserved (BridgeSpec.Protocol.ProtocolState.deposit state))
                (BridgeSpec.Protocol.DepositTrace.candidate (BridgeSpec.Protocol.ProtocolState.deposit state)))
              (BridgeSpec.Protocol.DepositTrace.requirement (BridgeSpec.Protocol.ProtocolState.deposit state)))
            (And
              (Eq.{1} (BridgeSpec.Withdrawal.destination (BridgeSpec.Protocol.ProtocolState.withdrawal state))
                (BridgeSpec.Protocol.ProtocolState.committedDestination state))
              (And
                (Eq.{1} (BridgeSpec.Withdrawal.amountOut (BridgeSpec.Protocol.ProtocolState.withdrawal state))
                  (BridgeSpec.Protocol.ProtocolState.committedAmountOut state))
                (And
                  (Eq.{1} (BridgeSpec.Protocol.ProtocolState.withdrawalFeeCounted state)
                    (BridgeSpec.Withdrawal.paid (BridgeSpec.Protocol.ProtocolState.withdrawal state)))
                  (BridgeSpec.Protocol.leaseBounded (BridgeSpec.Protocol.ProtocolState.lease state)))))))))
```

### BridgeSpec.Protocol.filterSafeStoredState

model-support: Abstract filter accepting states under the Safe condition, not production decoding or migration

Specification: `verification/README.md`

```lean
BridgeSpec.Protocol.filterSafeStoredState : BridgeSpec.Protocol.ProtocolState → Option.{0} BridgeSpec.Protocol.ProtocolState
fun stored => ite.{1} (BridgeSpec.Protocol.Safe stored) (Option.some.{0} stored) Option.none.{0}
```

### BridgeSpec.signatureTimeAllowed

specification: Predicate for a u64 deadline, rejection of addition overflow, and at least 300 seconds remaining

Specification: `verification/README.md`

```lean
BridgeSpec.signatureTimeAllowed : Nat → Nat → Bool
fun observedTimestamp deadline =>
  Decidable.decide
    (And (LE.le.{0} deadline (HSub.hSub.{0, 0, 0} (HPow.hPow.{0, 0, 0} 2 64) 1))
      (And (LE.le.{0} observedTimestamp (HSub.hSub.{0, 0, 0} (HSub.hSub.{0, 0, 0} (HPow.hPow.{0, 0, 0} 2 64) 1) 300))
        (LE.le.{0} (HAdd.hAdd.{0, 0, 0} observedTimestamp 300) deadline)))
```

## Source inventory (conservative change detection)

| Source | Lines | Declarations | SHA-256 |
|---|---:|---:|---|
| verification/lean/BridgeSpec/AuditExport.lean | 18 | 1 | 71f528152dcd1c0a250ae213e0d250e8c4a46d42002c38d579b13679dd7bf501 |
| verification/lean/BridgeSpec/ClaimContracts.lean | 639 | 107 | c9b78c3d0f847ebfdcb06b013805eada5964bed831c23eefe7128c1f9abd36c3 |
| verification/lean/BridgeSpec/Claims.lean | 226 | 28 | f74ffe05f86fbdfa4e94095bdc2dc64a1dd3ed94c947a4b800af6433dada3430 |
| verification/lean/BridgeSpec/ControlPlane.lean | 323 | 32 | 57e6656b42d4da34726d8e69bfe129939d7649070923beddc9c5fdce8876936c |
| verification/lean/BridgeSpec/DepositAuthorization.lean | 586 | 48 | 737633200787c8db2275d4a8408e8bad75754e04415c9cbc435b934660ca001e |
| verification/lean/BridgeSpec/DepositHistory.lean | 561 | 27 | 51536b20ba0fd5da8a26491d13faf56b5e38a3c300f16e319e99ee5c8dec6620 |
| verification/lean/BridgeSpec/FiniteWidthModel.lean | 137 | 31 | 0345c19c9df5a982a4f896205cbde5d01a40dd68a8bd1f4e3c58e9923e3cf0ea |
| verification/lean/BridgeSpec/GlobalHistory.lean | 889 | 67 | a1b6668db4ec35f997042da7fbfb73285d8606a2891084b4645542e2e8b7ea37 |
| verification/lean/BridgeSpec/LedgerBlockProvenance.lean | 337 | 28 | 23421e6d692d4651eb98d658e501e918e185f36d378f72ed5115fbaf06ba69c4 |
| verification/lean/BridgeSpec/Liveness.lean | 209 | 26 | 72402ad8473fc2e294a3725535ade0153ed8bd51270ebb22660d75d8706aeff0 |
| verification/lean/BridgeSpec/Model.lean | 323 | 60 | a9f07f9643e72cfb41bf9c1d14a34c5c6ab5108fc3cd54886e31216c0889ecd3 |
| verification/lean/BridgeSpec/ModelBoundaries.lean | 42 | 4 | 67b361fa752dfd66e101553b9a0ed6b49d177d49866b94239c8a6c214eba7d13 |
| verification/lean/BridgeSpec/ModelRefinement.lean | 161 | 24 | 101d62bf7e751cdcc14e0bccf61b9d6ba12326fa8cccdb70df63a51338649342 |
| verification/lean/BridgeSpec/Protocol.lean | 809 | 34 | 41678966a02226b6930bac1f74881d49b6d20f13c60671d7354da0bd90571043 |
| verification/lean/BridgeSpec/ProtocolPolicies.lean | 420 | 52 | a2702b5b210970ee5ac01e5c3109697c902775610de2e8982da60195d132f71c |
| verification/lean/BridgeSpec/Theorems.lean | 204 | 19 | 848b4f6361b2f097725a3ab084b9e7344d03c906030ca8ad004bb19095e6c4f4 |
| verification/lean/BridgeSpec/Vectors.lean | 439 | 41 | 00c33dad5b79d86ac6c9147f33352a412bc20f672e9c7ad33e25961a0a7e9325 |
| verification/lean/Main.lean | 10 | 1 | 286cd7fcd66afc4e7532fb8f2f0d7e0e15f858ffdad3de716c9f83bfc6b42a05 |
| verification/lean/fail/AccountingDeltaViolation.lean | 24 | 1 | 638f8837d8c221d828fed5162973b4215af39c61984c6aba3220f8b8b3dd7a08 |
| verification/lean/fail/AnonymousRefundRequest.lean | 8 | 0 | beae263600cfc9307feadae225b64983f34e11bfdfa04adae6316ec7a1869552 |
| verification/lean/fail/AuthorizationReissue.lean | 25 | 4 | ee67fee642f47c1d116fc94f8a5f9387e96525dfa5397646063d6fbc6451d508 |
| verification/lean/fail/BackingViolation.lean | 12 | 0 | 0ad4d6a1424518a8b8f9968310d0a88619133f86d4ce877fa7134a0f22b206d2 |
| verification/lean/fail/ConflictingFundingReplay.lean | 9 | 0 | fd5f6abf7f8812e64bf2cbf457991ca46fcea6cdcc71acd074a6b162317c7b96 |
| verification/lean/fail/DeadlineOverflow.lean | 6 | 0 | 864692b1294fc6b36f398fc91ea14695d07fea1c3defc14708eccc329aa74d5c |
| verification/lean/fail/DestinationMutation.lean | 20 | 3 | a3992470b4340e01eb81291efe4172fb9b3269141e987beea8238f02fded0cae |
| verification/lean/fail/DoubleDepositFeeTrace.lean | 7 | 0 | 9f00413132deded07cee5385e9a56384775d7b6b869bfad08b31c384059605ba |
| verification/lean/fail/DoubleFee.lean | 6 | 0 | 076049e9a5e13cd6c1e4ebab231ed452dffc9fcac994b7aa15bdd8645e304851 |
| verification/lean/fail/DriftedConfirmedActivationEvidence.lean | 6 | 0 | cce72323186747a19ebd7bd19393b75624c5b5f5a330eebb6a24b6c75aa1df5a |
| verification/lean/fail/EvidencelessMint.lean | 19 | 2 | 7d248eb1e8b57cfd2e9f2c3d3892e03b495934fbc920278146a056cca36b2c96 |
| verification/lean/fail/FinalizedTimestampDeadline.lean | 7 | 0 | 614a8705cfd818db1749f072723d51e18f07d03b750a882a3f8d1fdb2935a212 |
| verification/lean/fail/GovernanceLargerBalance.lean | 4 | 0 | 58f4ebfcedb3e3bfb34a83bb6f55d603d5a9b5ac1f7c6f9ca01ff0d0a557f1d1 |
| verification/lean/fail/IncompleteAbsence.lean | 15 | 0 | 22ed9af027b854b148ed57fd103aa4cf9707a4366943ce43b4276e3ca4cbad62 |
| verification/lean/fail/IncompleteExpiryAudit.lean | 22 | 2 | 4c30cb6604648725bea1d9a376cd758efedc2bbbc029c26088ed070229be55bd |
| verification/lean/fail/IncompleteMintAudit.lean | 21 | 2 | 34a54671a3271bf8efb75d8ffa94483006cb795bca44a73eeccd29384ed3ec1c |
| verification/lean/fail/InvalidExecutionStep.lean | 10 | 1 | a6b48a8e639af5578312a150535c1b778d45877f353b449eca9559558c156213 |
| verification/lean/fail/ManualActiveLeaseBypass.lean | 6 | 0 | 05b63de3fa0532e200bdd0c2f8e75495f96b40683b11177827e16335ee5fa7a7 |
| verification/lean/fail/ManualClaimEconomicMutation.lean | 13 | 1 | 9b60fdf900a577eea35aa64585347ed44b629cceae79b2dbbc0ede270ad7722d |
| verification/lean/fail/MintFeeAlreadyCounted.lean | 19 | 2 | 1506eb9046d1280a73afc6a2e0203ad2ebc477b2eb62afc1e8360bbd6bbb4a68 |
| verification/lean/fail/MissingUserLiveness.lean | 16 | 1 | 285ee5520011c8603f7db684c1a6365bef69da98d9e38c6ed60a4f0b69bbe07e |
| verification/lean/fail/ProcessedExpiryRefund.lean | 21 | 2 | 4dcff1642961a5f622a762ac1b4748b990a5b4ee6a480d4e6a42abf76f7e0773 |
| verification/lean/fail/QuoteChangedDuringTrace.lean | 8 | 0 | 9a520139c90b0736b11e639ee117cd5e8d2b58e7f36ef7fe039ebbb7b72fa645 |
| verification/lean/fail/RefundAfterReservationReplay.lean | 6 | 0 | 398008e4f37597fc27cdc61909bfe32312895afa6bb84128a5d01a252b164afd |
| verification/lean/fail/RefundBeforeFunding.lean | 9 | 0 | 47ac92b520aef0fc257bf1a3fa956af2ce6130b9bf676b56d8936e62c2bd56f0 |
| verification/lean/fail/SignatureTooLate.lean | 4 | 0 | bdd74ab81077e94e7f16dedce8b2dd0bd5db6a71c6d31e57e6fae72dc9fbc106 |
| verification/lean/fail/StaleCrossRecordCallback.lean | 27 | 2 | 0394bec7a1db4833940a74de7386b6150db5753b09b2cae5b8d5ebb8c64330b1 |
| verification/lean/fail/StaleLeaseCallback.lean | 9 | 0 | 060d4a6c2845d57c76dc64427eca5be79f8444c0472b7191a2ffffe4f34c2dae |
| verification/lean/fail/TerminalAuthorizationReopen.lean | 12 | 1 | 33f763ac7516e6a73762e978616d1d89063aac597834af2f954f90446805c4b1 |
| verification/lean/fail/TerminalDepositIndexed.lean | 6 | 0 | 5a740f2449b4ebd506b9d3095378f2226f253b9fd4ec96442422a0adbcbd91d7 |
| verification/lean/fail/UnauthorizedConfirmationCaller.lean | 8 | 0 | ebb7016ca30fe6b8c28c6dc6533da01331be76d3cc479a886866213d52ce80d0 |
| verification/lean/fail/UnauthorizedOperationalConfigSeal.lean | 6 | 0 | 8d45c7d35059439e55d1e8b7eaec11c842f9918084a8d4410c70dc986f43423e |
| verification/lean/fail/UnfairLiveness.lean | 16 | 1 | e346efb2083754bc8793d27a176243cc79c29f55a54eed0af4778e85faff6eca |
| verification/lean/fail/UnsafeBootstrapPausePrincipalMigration.lean | 6 | 0 | ddd05423bdf6e17a1ca3f0be7a2eae1694f5e331b638d5939fef44dafaf485eb |
| verification/lean/lakefile.lean | 11 | 0 | ea0d55c075440c9541d28b7802430abfe5e99b6a2e5a0abb561b76963055950c |
