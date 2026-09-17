---
status: accepted
---

# Validate RPC chain binding before runtime

The Bridge's configured chain ID is a configuration value defining the install domain, not a value observed in a Finalized block response. In environments using Custom RPC, operators fix the RPC URLs, expected chain ID, and each URL's upstream chain for the deployment lifetime. Preflight before deployment and activation calls `eth_chainId` on all three reviewed endpoints and fails closed if any endpoint is unreachable, returns an invalid response, or reports a different chain ID.

Production Base Mainnet Canister outcalls use the official EVM RPC Canister's built-in `BaseMainnet` providers, with `custom_evm_rpc_urls` fixed to an empty array. Gate A verifies only offline artifacts and constructor conditions. Post-deployment runtime, roles, pause, and chain binding use activation attestation obtained by the Canister through the official route as the source of truth. Direct checks against three Custom RPC endpoints are limited to staging monitor drills; do not inject URLs into production profiles, release bundles, or the UI. Base Sepolia staging and `test-deployment` builds use three reviewed Custom RPC endpoints. The staging Wasm's one-way provider replacement exception permits only replacement of the currently reviewed OnFinality set with the reviewed dRPC set. Fix the old/new URL arrays, order, both digests, Base Sepolia chain ID, and official EVM RPC Canister ID; trap any change except an equivalent rerun with the new set. Production Wasm does not compile this type, decoder, or replacement path. [ADR 0026](0026-replace-staging-onfinality-with-drpc.md) is authoritative for the replacement rationale and diagnostic evidence through IC.

Runtime `BaseMainnet(None)` selects three providers from the official EVM RPC Canister's default pool and requires two matching responses. This 2-of-3 quorum handles response disagreement and one provider failure for Finalized observations, canonical block hashes, receipts, contract state, and related data. Only `notify_withdrawal` Finalized observation selects the greatest checkpoint attested by at least two providers instead of requiring an exact latest-head match, then retrieves that height's canonical hash by exact 2-of-3. Other runtime snapshots and Deposit/refund Finalized head retrieval remain unchanged. This does not detect changes to the default provider registry, correlated compromise, or runtime switching of provider URLs or their upstream chains; these remain external assumptions. The fixed staging replacement preserves the saved Finalized watermark but invalidates the runtime attestation cache, requiring the next EVM observation to recheck runtime code through the new provider set. This design does not repeatedly validate `eth_chainId` at runtime or use expiring chain attestations.

## Record semantics

- `FinalizedObservation` stores only the Finalized block number, block hash, and observation time measured through RPC.
- RPC audit request digests bind the configured chain ID. Quorum response digests do not include the chain ID as an RPC-observed value.
- Stable `FinalizedObservationRecord.chain_id` is the configured chain ID binding a saved block observation to the install domain. It is not a chain ID obtained from an RPC response.
- Use the configured chain ID for chain binding in mint evidence, EIP-712 domains, Governance nonces, and similar data.
- Continue rejecting binding mismatches between stable records and current configuration and conflicts between records from different install domains.

## Rejected alternatives

- Reject calling `eth_chainId` for every runtime operation: it merely rechecks an upstream already verified at configuration time and assumed immutable during operation, adding no safety under this threat model.
- Reject refreshing expiring chain attestations because it introduces runtime provider-chain switching as a separate threat.
- Reject assigning the configured chain ID to `FinalizedObservation` and comparing it with the same configuration: that is a tautology and does not prove RPC observation.
- Reject including the configured chain ID in quorum response audits because it could be misread as a provider-returned value.

## Conditions for reconsideration

Reconsider this decision if RPC URLs, the configured chain ID, or each URL's upstream chain become mutable at runtime beyond the fixed one-way staging replacement. Redesign attestation invalidation, stable install-domain binding, audit semantics, handling of existing records, and runtime quorum responsibilities for the new threat model.

The machine-readable source of truth for claim dependencies on external assumptions and fail-closed behavior is `rpc_provider_chain_configuration` in `verification/assumptions.tsv`. Follow `docs/runbooks/operations.md` for operator procedures, `docs/runbooks/evm-rpc-canister-rehearsal.md` for rehearsal conditions, and `deployments/evidence-v1/README.md` for evidence requirements.

Direct Custom RPC rehearsals and monitor drills in staging are Gate C operational evidence after unpause; they do not authorize Gate B, activation, or controller handover. Production chain binding for Gate B uses `provider-independence.json`, the official EVM RPC Canister's default `BaseMainnet` pool, and fresh activation attestation as authoritative evidence.
