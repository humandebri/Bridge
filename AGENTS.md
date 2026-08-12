# AGENTS.md

## Pre-deployment compatibility policy

- This repository has not been deployed to production yet. Until the first production deployment, do not preserve backward compatibility for obsolete public APIs, configuration shapes, stable-memory schemas, fixtures, or upgrade paths.
- Prefer replacing pre-deployment formats directly and updating all callers, tests, fixtures, and documentation in the same change. Do not add legacy migrations, compatibility shims, dual-read paths, or fallbacks unless the user explicitly requests them.
- Unknown or obsolete stable schema versions must fail closed. Test upgrades and stable-memory reopen behavior only for the current schema unless an earlier schema has actually been deployed.
- Revisit and explicitly tighten this policy when the first production deployment is approved.

## RPC chain binding review policy

- Follow `docs/adr/0024-validate-rpc-chain-binding-before-runtime.md` when reviewing EVM RPC chain binding. Under its fixed-provider assumptions, the absence of runtime `eth_chainId` calls is not a finding; all three reviewed Custom RPC endpoints are checked before deployment and activation.
- Do not treat configured chain IDs as RPC-observed Finalized response fields. `FinalizedObservation` is block evidence, RPC audit requests bind the configured chain ID, quorum response digests omit it, and stable record chain IDs bind observations to the install domain.
- Runtime 2-of-3 quorum handles response disagreement and provider failure, not provider chain switching. Reassess this policy before making an RPC URL, configured chain ID, or endpoint upstream chain mutable during runtime.

## Logic changes and proof impact

- Treat production state transitions, admission and rejection conditions, authorization, deadlines, epochs, replay protection, accounting deltas, scheduler or lease decisions, storage commit effects, and Solidity policy or kernel code as safety-related logic.
- Before changing safety-related logic, identify the affected claim IDs and their abstract theorem, production kernel, proof obligation, negative fixture, refinement or adapter test, transaction test, vector consumer, and external assumptions.
- Use `verification/proof-impact.tsv` as the source-to-claim and source-to-stage ownership manifest. Use `verification/claims.tsv` as the typed evidence ledger.
- Keep safety decisions in production-shared kernels. Do not treat model-only proofs, prose, symbol-name checks, or an unrelated passing test as production implementation proof.
- Register every new safety-related source file, kernel, state, event, reject reason, fixture, and vector consumer in the applicable manifest in the same change. Watched source roots fail closed when a source is unregistered.
- A logic change does not require a cosmetic proof-file edit when the existing theorem still applies. It does require rerunning all impacted proof stages against the current source and preserving the implementation refinement link.
- Keep claims that depend on external assumptions or unsupported prover boundaries at `partial`. Record the reason instead of weakening or bypassing the proof gate.
- A safety-related logic change is not complete until `python3 scripts/check_proof_impact.py`, `python3 scripts/check_claim_manifest.py`, `scripts/ci-local.sh proofs`, and the applicable unit, negative, refinement, and transaction tests pass.
- The proof receipt must contain a source fingerprint matching the current checkout. Missing stages, stale fingerprints, unregistered safety sources, unregistered fixtures, model-only implementation claims, and production kernels without claim ownership must fail closed.
- In the final report for a safety-related change, list the affected claims, executed proof stages, and remaining external assumptions.

## Deployment validation efficiency

- Run deployment and staging promotion gates from the current clean checkout by default. Do not create an isolated Git worktree merely to satisfy a clean-tree check; fail before expensive validation when tracked or untracked changes are present.
- When isolation is explicitly necessary because concurrent build-input changes must be preserved, reuse the existing Cargo target cache, pnpm store, pinned tools, and initialized submodules. Set non-interactive package-manager mode before starting the gate.
- Reuse a proof receipt only after the repository verifier confirms its source fingerprint, tool versions, submodule revisions, required stages, and completeness against the current checkout. File existence or a matching commit name alone is not sufficient.
- Before waiting for chain finality, inspect an available transaction receipt for terminal failure. A reverted receipt must stop the driver immediately and must never be reported as a finality timeout.
