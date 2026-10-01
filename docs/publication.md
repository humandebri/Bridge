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

The October 1 follow-up inspected merged source `9d6f3fae67bc3b18de13a8bfd64b1fbf24aa252c` from an independent remote clone with recursive submodules. The [audit record](evidence/publication-audit-20261001.json) contains the source fingerprint, scan scope, results, and settings. It is not a complete validation receipt for later revisions.

Gitleaks 8.30.1 found no matches in the tracked archive or fetched reachable history, including merge diffs. It scanned 802 patch-bearing commits out of 829 reachable commits. Reports were redacted; unreachable/deleted refs, ignored local artifacts, and unsupported credential formats remain outside the scan. History was not rewritten. Public evidence was reviewed for credential URLs, personal paths, and contact metadata; public verification identifiers were retained.

The repository was already public. Private vulnerability reporting, secret scanning, and push protection were enabled. The [security guide](../SECURITY.md) links to the report endpoint; no synthetic report was submitted, so notification delivery is untested. Main requires strict `trusted-pr-gate` checks and disallows force pushes/deletion; administrator enforcement and a required PR review rule are absent. These settings observations do not change the trusted-PR policy.

### Fresh-clone validation

With pinned Node 24.14.0, pnpm 11.0.8, Rust 1.97.0, locked dependencies, and initialized submodules, the independent clone passed:

- 509 UI tests, UI typecheck/lint/build, and 244 Rust unit tests.
- A locked release Wasm build and lightweight formatting, code-generation, schema, and manifest checks.
- The focused production-install fixture in the trusted Linux/amd64 container, using UID/GID 1001, read-only inputs, and writable temporary tmpfs paths.

The host `scripts/ci-local.sh versions` run stopped at the guard rejecting an operating-system-resolved Cargo home configuration. Personal configuration was preserved. The Linux fixture result is separate evidence, not a passing full versions run. No full proof, PocketIC, browser E2E, staging, production deployment, or complete `all` gate was run by this audit.

### Distribution follow-up

The [dependency review](licenses/README.md) records Reown custom terms and five package versions lacking license bodies. Prepare notices for the components included in the actual release, rather than maintaining generated dependency closures in Git. These release tasks do not require expanding this documentation cleanup into a dependency replacement or production release.

Complete current-source validation and distribution-specific notice review remain release requirements. This audit does not authorize production.
