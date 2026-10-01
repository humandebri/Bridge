# Public repository preparation

This record describes documentation preparation on October 1, 2026. It is not a production release receipt or a declaration that all publication checks have passed.

## Completed documentation work

- The repository overview separates the certified Root-only controller observation from unverified runtime readiness and deferred same-Wasm upgrade validation.
- Architecture and detailed development instructions have their own entry points.
- Implementation plans and dated observations are identified as historical context.
- Initial-install and joint-control runbooks identify their lifecycle scope after Root-only handover.
- Maintained prose and user-facing messages use English. The non-ASCII URL rejection fixture retains the same Unicode input using a Rust escape.
- Apache-2.0 license text, contribution guidance, security-reporting guidance, and a third-party dependency inventory entry point are present.

## Validation of this cleanup

The documentation link check found no missing local targets, and a scan of tracked and new project text found no Japanese prose. `git diff --check`, Rust formatting, schema consistency, proof ownership, the claim manifest, and claim/test registration validation passed. The impact manifest selected no safety claims or proof stages for these changes.

Using Node 24.14.0 and pnpm 11.0.8, the History component suite passed all nine tests; UI typechecking, focused lint/format checks, and ABI/Candid checks passed. The Unicode URL rejection test passed with `test-deployment` enabled in a dedicated Cargo target directory.

The browser fixture initially failed because its notification-client alias did not export newly consumed APIs. It now re-exports the real notification client while retaining its explicit test identity override. With vlmkit 0.23.0, the fixture passed integrity checks at widths 1280, 768, and 375. A scripted polling flow verified the English service-unavailable message and that no additional wallet request was made. A static manifest check before the polling interval could not observe that dynamic message; the timed flow supplies the relevant evidence.

No full proof gate, fresh-clone end-to-end run, production operation, publication, or repository visibility change was performed.

## Follow-up publication audit

The follow-up inspected merged source `9d6f3fae67bc3b18de13a8bfd64b1fbf24aa252c` from an independent remote clone with recursive submodules. Its frozen Git archive SHA-256 was `333e1b6d8d6d3ecbf34fd99cb1d5b696356548c3742228edbbd1e999b0ca41bf`, unchanged after validation. The [machine-readable record](evidence/publication-audit-20261001.json) separates completed checks from unresolved conditions. New notices and documentation are follow-up artifacts; tests of the merged source are not a complete validation receipt for this later change.

### Credential and public evidence review

Gitleaks 8.30.1 found no matches in the tracked source archive or fetched reachable remote history. The history scan included merge diffs (`--all --full-history --diff-merges=first-parent`), archive depth 2, decode depth 5, and disabled inline ignores. Reports were fully redacted. Git listed 829 reachable commits; Gitleaks scanned 802 patch-bearing commits and about 38.79 MB. These counts differ because patchless commits have no content diff. Deleted or unreachable references, ignored local artifacts, arbitrary unsupported formats, and unknown credentials remain outside this evidence.

The earlier limited pattern scan is superseded by this broader dated result; neither scan guarantees absence of secrets. No history was rewritten. Public evidence and deployment records were checked for credential-bearing URLs, personal host paths, and contact metadata. Public principals, addresses, transaction/module hashes, anonymous query arguments, and operational binding identifiers remain as verification evidence. Commit author metadata remains public Git metadata. Upstream copyright/contact text in license bodies is preserved as attribution. Local audit reports and host paths are excluded from committed audit artifacts.

### Dependency notices

The [license audit](licenses/README.md) supplies an inventory of 319 UI and 177 external Rust package versions, frontend notices, and a Wasm companion notice file. Reown custom community terms require operator review beyond retaining notices. Five UI package versions still have `metadata-only` notices. These gaps prevent a claim of complete distribution-license clearance.

### GitHub settings and reporting

The repository was already public; visibility was not changed. Private vulnerability reporting, secret scanning, and secret scanning push protection are now enabled. The [security guide](../SECURITY.md) links to the confidential report entry point. The endpoint and enabled setting were checked without submitting a report; delivery to maintainers and response handling remain untested.

Main requires the strict `trusted-pr-gate` check bound to GitHub App 15368, with force pushes and deletion disabled. Administrator enforcement is disabled, and there is no branch-level required PR review rule. Repository rulesets are empty. External-contributor Actions approval is configured for first-time contributors. These observations describe the actual boundaries; this audit does not tighten administrator/review settings or replace the trusted-PR policy.

### Fresh-clone validation

The independent clone used pinned Node 24.14.0, pnpm 11.0.8, Rust 1.97.0, initialized submodules, frozen lockfiles, and executable installed dependency assets. Lightweight formatting, schema, ABI/Candid generation, proof ownership, claim registration, and claim/test manifest checks passed.

- UI: 61 suites / 509 tests passed; typecheck, lint, and production build passed.
- Rust: 236 Canister and 8 core unit tests passed with locked dependencies and an isolated Cargo target directory.
- `cargo build -p bridge-canister --release --target wasm32-unknown-unknown --locked --offline` passed. This raw Cargo Wasm is a build result, not an installable production release receipt or a twice-reproduced production artifact.
- The focused production-install fixture passed in the existing trusted Linux/amd64 image under UID/GID 1001, with read-only input mounts and writable `/tmp` and test temporary tmpfs paths.

The host `scripts/ci-local.sh versions` run stopped because the production-install fixture correctly rejected an operating-system-resolved Cargo home configuration. This is a host environment limitation, not a passing full versions run. Personal Cargo configuration was preserved; the Linux result is reported independently and is not stitched into a complete receipt.

No full proof, PocketIC, browser E2E, staging, production deployment, or complete `scripts/ci-local.sh all` gate was run by this audit. The existing main CI run remains a separate source of evidence. Full validation and deployment requirements in repository policy remain in force.

## Remaining publication conditions

- Resolve Reown custom terms against actual deployed branding, network use, usage, and commercial arrangements, or change dependencies through a separately reviewed implementation.
- Obtain the five missing upstream license bodies or resolve the corresponding dependencies before asserting complete notice coverage.
- Confirm real private-report notification delivery and maintainer handling when a legitimate report arrives; no synthetic report was sent.
- Retain a complete current-source validation receipt for a release. Focused fresh-clone checks and dated settings observations do not authorize production.
