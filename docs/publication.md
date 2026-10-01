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

## Publication checks still to complete

- Verify a fresh clone using pinned tools, lockfiles, initialized submodules, and executable test assets. This documentation change does not supply a new full validation receipt.
- Establish and test a confidential vulnerability-reporting channel. The security guide does not claim that GitHub private reporting is enabled.
- Review licenses and required notices for dependencies included in distributed Wasm and UI bundles.
- Review public evidence for unnecessary account information and operational metadata. Preserve public principals, addresses, and hashes needed for independent verification.
- Perform a dedicated credential review before publishing Git history. The limited scan below is not a comprehensive secret audit.
- Review GitHub repository settings and existing trusted-PR instructions for contributions from outside the project. No repository visibility or settings were changed during documentation preparation.

## Limited credential review

The current tracked text files were checked for private-key headers, GitHub token formats, AWS access-key formats, and credential-bearing HTTP URLs. The only candidate was a deliberately invalid `https://user:secret@two.example/rpc` URL in `scripts/evm-rpc-rehearsal/test_rehearsal.py`, used to test rejection of authenticated URLs.

Reachable local Git history was checked for changes matching private-key headers, GitHub token formats, and AWS access-key formats; those patterns returned no matching commits. This does not cover every credential format, encoded values, arbitrary RPC path tokens, unreferenced objects, or remote-only branches.

Local ignored operational artifacts and credentials are not publication inputs. Verify what Git actually tracks and what the hosting service will expose before changing visibility. Do not rewrite history or delete evidence automatically on the basis of a pattern match.
