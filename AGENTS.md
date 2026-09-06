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

## Long-running validation coordination

- A validation input is any tracked or untracked file covered by the gate's source fingerprint or consumed by its build and test commands. A writer is any agent, process, formatter, generator, or user action that can change a validation input. An expensive gate is a full proof, PocketIC, integration, deployment, staging, coverage, or similarly long-running validation command.
- Use one writer for a shared working tree. While that writer is implementing, other agents and background tasks must remain read-only unless their writes are isolated in a separately authorized working tree. Do not let multiple writers repair or regenerate the same checkout concurrently.
- Before starting an expensive gate, finish the applicable lightweight checks: `git diff --check`, `cargo fmt --all -- --check`, `python3 scripts/check_schema_consistency.py`, `pnpm --dir ui run codegen:abi:check`, `pnpm --dir ui run codegen:candid:check`, `python3 scripts/check_proof_impact.py`, `python3 scripts/check_claim_manifest.py`, `python3 scripts/check_claim_test_manifest.py --validate-only`, and focused unit or stage-specific tests. Resolve their failures first; do not start a full gate while any lightweight check fails, and do not use a full gate to discover claim/test registration drift that `--validate-only` reports without building or executing transaction tests.
- Ensure no equivalent expensive gate is already running for the checkout. Do not start duplicate `scripts/ci-local.sh proofs`, PocketIC, Jest, Cargo, or deployment-driver suites. If another valid run already owns the checkout, wait for it and verify its receipt or result instead of competing for locks and temporary space.
- If an expensive gate can exceed the command runner's approximately 15-minute session limit, start it in an approved persistent terminal session such as `tmux` from the outset and monitor that session. Do not let the runner timeout terminate a valid gate, and do not treat timeout exit 143 or a partial receipt as a code failure or completed evidence.
- Freeze validation inputs for the duration of an expensive gate. If an input changes after the gate starts, treat that run as invalid, identify and stop the writer through the applicable coordination or approval path, and wait for a stable checkout before rerunning. Do not repeatedly restart a full gate while writes are still possible.
- Diagnose a failing proof stage with that stage's direct command or focused fixture first. After the final source edit, run the required full proof gate once to produce a complete current-fingerprint receipt. Reuse prior results only through the repository verifier; never infer stage reuse from partial console output.
- Put large temporary artifacts on a repository-external directory on a volume with adequate free space. Do not place a copied Cargo source tree below this repository, because Cargo can misclassify it as a workspace member. Check free space before long Jest, PocketIC, SMT, Halmos, Verus, coverage, or deployment-validation runs.
- When a long-running command exceeds its expected duration or produces no progress for two minutes, inspect its child process, lock contention, concurrent writers, and disk space before waiting longer. Do not kill an unknown process or delete caches until ownership and recoverability are established.
- Record the command, checkout fingerprint, start time, active owner, temporary directory, and final receipt or failure stage for an expensive gate. The final report must distinguish code failures from invalidated runs, resource exhaustion, sandbox restrictions, and concurrent-process interference.

## Dedicated trusted-CI verification procedure

- Before opening or updating a PR that changes CI, trusted-pr files, release scripts, or validation policy, create a clean external verification checkout. Record `BASE_SHA`, `HEAD_SHA`, tool versions, and the checkout path; do not run an expensive gate in a dirty shared checkout.
- Install dependencies from the lockfiles before running JavaScript-backed checks. Use the pinned Node.js release from `.node-version` (for example, `fnm exec --using v$(<.node-version) pnpm install --frozen-lockfile --ignore-scripts` and the equivalent `ui` install). A host Node.js mismatch or missing `node_modules` is an environment failure, not evidence of a source regression.
- Complete lightweight checks first, then run `scripts/ci-local.sh versions` once with the pinned Node.js release and `BRIDGE_EXPECTED_HEAD_SHA` set to the checkout `HEAD_SHA`. Do not bypass the version re-exec merely to make a local run green.
- Reproduce the trusted production-install boundary in a Linux container when the container runtime is available. The wrapper passes the runner UID/GID and uses read-only input mounts; provide writable dedicated tmpfs paths for `/tmp` and the test `TMPDIR`. The image must contain a passwd entry for the runner UID because production installation resolves `pwd.getpwuid(os.getuid())`. Verify both the image account binding and the production-install test before pushing.
- For the Apple `container` runtime, first check `container system status`, then use a pinned `linux/amd64` image with `container run --platform linux/amd64 --read-only --tmpfs /tmp --tmpfs /scratch/tmp --user 1001:1001 --mount source=<checkout>,target=/workspace,readonly --env TMPDIR=/scratch/tmp --workdir /workspace <trusted-image> bash scripts/test_production_canister_install.sh`. Treat a run without writable tmpfs or with host-mapped ownership as an invalid harness result, not as a code failure.
- Treat `BRIDGE_CANDIDATE_SCRIPTS` as a trusted-container-only input. Tests that create temporary repository fixtures must clear it (or point it at a complete fixture tree); otherwise the classifier can report a false missing-helper failure. The trusted wrapper overlays policy scripts over candidate scripts, so candidate edits to masked policy files cannot repair the trusted base.
- If the change is in the trusted base itself, expect a bootstrap limitation: `pull_request_target` checks out the current `main` as `base_sha`, so the PR cannot validate a base-policy change until it is landed. Record local evidence, obtain the required exact-head review, and use the approved bootstrap merge procedure; then synchronize and rerun dependent PRs against the new base.
- After the final source edit, run the expensive trusted gate exactly once, preserve its complete log and receipt, and report whether any failure was code, environment, resource, stale-base, or invalidated-input related. Never interpret a partial timeout or an old-base result as proof that the final checkout passed.

## Cycles handling safety

- Before minting, transferring, or topping up cycles, apply the `cycles-management` skill and verify whether the destination is a cycles-ledger account or a canister execution balance.
- Never use `icp cycles transfer <amount> <canister-principal>` to top up a canister. It credits the cycles-ledger account owned by that principal and does not increase the canister execution balance.
- Use `icp canister top-up <canister> --amount <amount>` when increasing a canister execution balance, and verify both the canister status balance and the cycles-ledger transaction result after the operation.
