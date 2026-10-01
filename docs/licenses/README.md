# Dependency licenses for releases

The October 1, 2026 review of source `9d6f3fae67bc3b18de13a8bfd64b1fbf24aa252c` inspected 319 UI production package versions and 177 external Rust package versions. These conservative dependency closures include code that may not appear in distributed artifacts. Full generated inventories and license bodies are not maintained in Git.

## Generate notices when preparing a distribution

1. Build the intended UI or Wasm from the release revision with pinned tools and lockfiles.
2. Enumerate dependencies as review inputs:

   ```sh
   pnpm --dir ui licenses list --prod --json > /path/to/release/ui-dependencies.json
   cargo metadata --format-version 1 --locked --filter-platform wasm32-unknown-unknown > /path/to/release/rust-dependencies.json
   ```

3. Select the components included in that distribution using build output and bundler/module evidence. For Wasm, trace dependencies from `bridge-canister`; distinguish host build tools from runtime code. Dependency enumeration alone does not establish inclusion. When inclusion is uncertain, retain the corresponding notice conservatively.
4. Collect the selected versions' license and attribution text from their published packages or matching immutable upstream revisions. Preserve copyright and license wording; record package versions and provenance in the release output. Resolve missing text and custom terms for the selected components.
5. Generate `THIRD_PARTY_NOTICES.txt` in the release output directory. Include it with the frontend assets or alongside the Wasm download, and verify that the published release retains it. The commands above produce inventories, not complete notices; current build/deployment scripts do not automate these steps.

Review notices again when the release dependencies or included components change. Release artifacts belong with their release, rather than as a second dependency database in the source tree.

## Findings to retain

Reown AppKit and WalletConnect packages in the audited lockfile use custom Reown Community License terms. Review their branding, network, usage, and commercial conditions for the distributed product; retaining a copyright notice alone does not establish compliance. Project Apache-2.0 licensing does not change upstream terms. Refer to the version-specific packaged license, since upstream main branches can change.

License text was not found in the inspected packages or matching upstream revisions for `encode-utf8@1.0.3`, `react-remove-scroll-bar@2.3.8`, `tr46@0.0.3`, and `uint8arrays@3.1.0` / `3.1.1`, although their metadata declares MIT. Resolve notice coverage if those components are included in a distribution. This dated review is not complete distribution clearance.

See the [root third-party guidance](../../THIRD_PARTY_NOTICES.md) and [publication audit](../publication.md) for scope and validation evidence.
