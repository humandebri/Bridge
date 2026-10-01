# Dependency licenses and distribution notices

This audit covers source revision `9d6f3fae67bc3b18de13a8bfd64b1fbf24aa252c` on October 1, 2026. Its lockfiles contain 319 unique UI production dependency versions and 177 external Rust dependency versions in the Canister closure. Project crates are covered by the root Apache-2.0 license.

## Scope and artifacts

- [Dependency inventory](dependency-inventory.json): package names, versions, license declarations, notice status, SHA-256 hashes of collected UTF-8 license text, and provenance.
- [UI notices](../../ui/public/THIRD_PARTY_NOTICES.txt): original upstream license bodies and package attribution. Vite copies this public asset to `dist/THIRD_PARTY_NOTICES.txt`; retain it when publishing frontend assets.
- [Canister notices](canister-notices.txt): distribute this companion file with the audited Wasm. The deployment scripts do not automatically attach it to every release artifact.
- [Root notices](../../THIRD_PARTY_NOTICES.md): vendored contract licenses and outstanding conditions.

The UI inventory comes from `pnpm --dir ui licenses list --prod --json` after frozen-lockfile installation with Node 24.14.0 and pnpm 11.0.8. The Rust inventory comes from `cargo metadata --format-version 1 --locked --filter-platform wasm32-unknown-unknown`, following normal and build dependency edges from `bridge-canister` and excluding development edges. This deliberately includes build tools and dependencies that may be eliminated from the final artifact; it does not prove byte-level inclusion or cover arbitrary development-tool redistributions.

License bodies were collected from published packages. Where absent, the inventory records a retrieved upstream license or README section at an immutable commit or version tag. License wording is preserved with UTF-8/newline normalization, trailing whitespace removal, and a single final newline. `upstream_text_sha256` records the collected source text before normalization; `sha256` identifies the distributed text. The published `@phosphor-icons/webcomponents@2.1.5` LICENSE contained merge markers around two identical `SOFTWARE.` lines; the distributed notice keeps one identical line and removes those markers. This repair changes no license wording. Different bodies have separate hashes even when their declared license names match. Alternative-license expressions retain their upstream meaning.

When a lockfile or submodule changes, repeat dependency enumeration, inspect each package's license and notice files, retrieve missing text from the matching upstream revision, and update the inventory and both notice files together. Review custom terms before distribution. Do not substitute a generic license template for missing upstream copyright information.

## Reown community terms

Pinned Reown and WalletConnect packages supply AppKit and WalletKit Community License texts dated August 25, 2025. These custom terms are included verbatim in the UI notices, together with the required Reown copyright notice. They include conditions beyond notice retention, including branding, gateway/network use, ownership changes, and commercial thresholds. The pinned texts describe thresholds of 500 monthly active users or 2.5 million monthly RPC requests. The operator must determine applicable terms and actual usage before asserting compliance; this audit does not accept a commercial agreement or verify deployed branding or usage.

Refer to the collected version-specific license bodies and [upstream AppKit license](https://github.com/reown-com/appkit/blob/main/LICENSE.md); the upstream main branch may change. These dependencies do not become Apache-2.0 when distributed with Bridge. A claim that every component uses a standard open-source license would be inaccurate.

## Missing license bodies

| Package | Version | Observed declaration | Status |
|---|---|---|---|
| encode-utf8 | 1.0.3 | MIT | metadata-only |
| react-remove-scroll-bar | 2.3.8 | MIT | metadata-only |
| tr46 | 0.0.3 | MIT | metadata-only |
| uint8arrays | 3.1.0, 3.1.1 | MIT | metadata-only |

No standalone license body was found in these published packages or at the inspected matching upstream revision. Package author metadata is retained where available; it is not an invented copyright attribution. Obtain the applicable upstream notices or resolve these dependency choices before declaring complete notice coverage. All inspected Rust package versions have a collected notice body; collection alone does not establish that every license obligation has been satisfied.
