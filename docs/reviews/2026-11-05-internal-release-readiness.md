# Internal production readiness review — 2026-11-05

This review preserves the v36 API, stable schema, fee policy, asset transitions and existing wallet choices. Ordinary wallet behavior is accepted as user-confirmed. It does not authorize a Canister upgrade or UI publication.

## Dependency reachability and receiver boundary

`ui/scripts/check-wallet-dependency-boundary.mjs OUTPUT_DIRECTORY` builds the UI with the same public switches as `production-assets.mjs`: generic production assets, no embedded runtime profile, and WalletConnect enabled with a dummy 32-hex project ID. It saves emitted module identities, rendered lengths and importer edges. It never publishes or contacts a wallet. `run_ui_fast` now requires this check, including the WalletConnect-enabled build, and propagates its failure to CI. Diagnostic output is temporary; the gate log records the result.

| Dependency | Finding for the reviewed browser build | Disposition |
| --- | --- | --- |
| Zod 3.22.4 / Reown wallet | WalletConnect imports `@reown/appkit/core`, whose factory forces `basic:true`. The full auth `W3mFrame` parser and Zod 3.22.4 have no rendered modules, even with WalletConnect enabled. Some non-parser Reown wallet helpers remain. | No demonstrated production UI path to this parser. Keep the receiver defense and tests for direct use or future graph changes; the build check fails if the parser enters the emitted graph. |
| Zod 3.25.76 / Coinbase CDP | Base Account's Node payment helpers import CDP. Its browser entry excludes those helpers; neither CDP nor Zod 3.25.76 has rendered modules in the reviewed UI. The Coinbase Wallet connector remains available. | No demonstrated browser input path. The build check fails if either parser enters the emitted graph. |
| Zod peer snapshots in viem / ox / appkit-common | Lockfile peer resolution alone does not imply an executable Zod import. | Classify using actual rendered modules, not package-name occurrence in a lockfile or parent directory. |
| minimatch 9.0.9 / Jest reporters through glob 10.5.0 | `glob` uses optimization level 2, which meets the advisory condition. Its patterns come from executable Jest/project configuration, not production requests. Root scripts, UI source and operations tools have no direct glob/minimatch imports. | Retain as a development residual, without dismissing the alert. A contributor who changes these patterns already controls executable CI input. Do not force minimatch 11 into glob 10 without API compatibility evidence. |
| deepmerge 4.3.1 / Jest config | Development-only. Jest's merge-with-preset path requires both preset globals and user globals; the fixed integration config supplies neither. The other exposed merge API has no installed Jest caller. | Retain as a documented development dependency. No production request/asset input reaches it. A PR author who changes executable test configuration already supplies code executed inside the trusted CI container. |

The Reown patch checks both the configured secure origin and the exact iframe `WindowProxy` before either dapp-side frame receiver parses data. Its reverse-direction `onAppEvent` API has no installed dapp caller and is not covered by this patch. The configured secure iframe remains a trust assumption if the auth path is introduced later; this patch is not a generic resource bound for trusted-frame messages or a blanket fix for all Zod versions. Dependency alerts are not dismissed as a side effect of this review.

The browser regression uses real Chromium `postMessage` delivery, with locally fulfilled iframe content. It rejects a different iframe at the correct secure origin, a separate window, and the actual target iframe after navigation to a wrong origin. Oversized messages do not invoke the schema parser. Normal READY responses reach both persistent and one-shot receivers, including after iframe recreation. This is browser-boundary verification, not a live Reown service or wallet transaction.

The minimatch assessment covers [GHSA-g3hv-75wg-m249](https://github.com/advisories/GHSA-g3hv-75wg-m249), including its optimization-level-2 condition. Development-only classification is based on the importing code and pattern source, rather than the dependency label alone.

## Asset safety test coverage

Existing registered tests cover the requested cases. A new actual consent-response regression also checks owner/subaccount, debit/refund amounts, the IC-time authorization lifetime, absence of a promised minimum signing window, and the retained service fee if a signed authorization expires unused. The consent text now matches the approved strictly-before-deadline signing policy; state transitions and fee rules are unchanged.

| Case | Existing executable evidence |
| --- | --- |
| Duplicate mint / wallet request | `does_not_timeout_or_duplicate_a_wallet_request`, shared modal/history preflight; production transition/refinement and replay tests |
| Duplicate refund | `refunds_unsigned_dust_without_service_fee_and_without_duplicate_transfer` |
| Expiry / refund evidence | `expired_signature_is_not_installed_or_charged`, `expired_authorization_refund_requires_persisted_finalized_unprocessed_evidence`, finalized Base-time refund tests |
| Fee changes | Deposit and withdrawal fee-increase rejection after review and during approval |
| Communication failure / retry | Explicit-rejection-only mint retry, RPC backoff, stopped-deposit continuation, reconciliation hold and notification recovery tests |
| Upgrade / restart recovery | Representative v36 same-Wasm upgrade, preserved funding/refund/release indexes, reopened funding attempt identity/deadlines, legacy consent migration |

`verification/claim-test-manifest.tsv` and its links identify the test selectors and owning claims. The full local gate also exercises real local Ledger flows. These tests do not establish availability of production providers, user actions or keeper behavior.

## Current production evidence and provenance

The policy profile was recovered from `Bridge-support/operations/bridge-proposals/195/reviewed-production-profile.json`. Its bytes match the reviewed profile preserved with the October UI publication records (SHA-256 `848cb3307b6a2fc66ddfbdaf9f3fecc154e7173dc5c79c538f530dc02b749157`). The reviewed runtime policy predates the upgrade; only the module field in the verification candidate is advanced to the hash independently recorded in proposal 195's review and its saved upgrade artifact. No live value is adopted as its own expected policy.

- Expected and certified current module: `6658e1a0ac42bfe50b61dd11cf1bdd0be484bcab983a328a552eb5bc3c676a53`.
- Controller: SNS Root `7jkta-eyaaa-aaaaq-aaarq-cai`, registered once.
- Proposal 195: signature-verified executed state, no failure; its sanitized Wasm description identifies the same submitted gzip hash. The returned description is not Wasm bytes and must not itself be hashed as the module.
- Reviewed UI RPC bytes: SHA-256 `a851c1e4c419ba3faa36b049a0e7e917b1d007d5b8c401169170dffc72ecd2c9`, matching the preserved October runtime records and current public RPC configuration. Key-bearing URLs are omitted here.
- On 2026-11-05, current-source `verify-production-current-state` and `verify-production-current-ui-live` passed with `root-registered`: authenticated v36, Activated/unpaused state, RuntimeBinding, reserves, operational configuration, storage integrity and complete history indexes agreed with certified module/controllers.
- The candidate's public rendering exactly matched the hosted deployment profile (SHA-256 `5ad140650e08c1532c4d9ba95da1b6df1e30f710466ff7e1c35652ed800d67e8`). Public rendering intentionally omits internal authorization metadata; absence of those fields in the served file is not a version mismatch.

These are time-bounded observations. Sequential before/after comparisons cannot rule out an intervening change that returns to the same value. Re-run the existing live authorization immediately before any future production submission/publication. An integrity error, unavailable evidence, unready index, insufficient reserve, or module/controller/configuration drift must stop that operation.

## Verification and operating decision

The receiver itself is tested, not formally proved. Direct ownership is `payment_identity` and `authorization_binding`; the workspace policy edit selects the complete 44-claim, 10-stage impacted union. Existing abstract theorems, production kernels, negative fixtures, refinement links and external assumptions remain in force. Do not promote partial claims based on this patch.

The final candidate must pass lightweight checks, the impacted union and `scripts/ci-local.sh all` in a clean pinned environment with recursively initialized submodules. Its complete receipt must pass `check_proof_impact.py --receipt` with the current fingerprint. Prior interrupted runs and the earlier runs with missing nested submodules are retained separately and cannot be combined into this receipt. The PR's validation record supplies final commit, fingerprint, full log and receipt paths. After a successful full CI gate, the workflow independently verifies the current receipt and exports it with execution reports and the three consumed Wasm artifacts for 14 days. Receipt verification remains bound to the original exact checkout, source fingerprint, pinned tools and submodule revisions; downloading an artifact is not authorization to deploy. The consent wording is covered by executable regression, not by a new formal theorem.

For operational recovery, preserve transaction identity and history; confirm terminal transaction failures before waiting for finality; keep uncertain Ledger sends in reconciliation hold; use the existing manual continuation and SNS Root-only recovery procedures. Monitor pause state, reserve sufficiency, RPC disagreement, storage/index failures and held records. RPC/chain canonicality, IC/ledger authenticity, wallet identity, SQLite atomicity and the pinned toolchains remain external assumptions. Conditional liveness additionally requires continuing external availability, time/cycles, admissibility, and the recorded user/keeper actions.

Internal verification can establish a reviewable implementation and a consistent current observation. It does not guarantee future provider behavior or authorize deployment. Finalized RPC policy is unchanged. The production release decision remains separate and requires fresh live authorization of the exact submitted artifacts.
