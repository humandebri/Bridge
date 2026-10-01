# Third-party dependencies

Project-authored code uses Apache-2.0. Dependencies and vendored sources retain their upstream licenses, copyright notices, and attribution requirements.

OpenZeppelin Contracts is pinned as a Git submodule under `contracts/lib/openzeppelin-contracts`; its upstream [license](contracts/lib/openzeppelin-contracts/LICENSE) applies to those sources. Nested upstream test libraries retain the license files distributed with them.

The October 1, 2026 [dependency audit](docs/licenses/README.md) records 319 UI package versions and 177 external Rust package versions from the locked source revision. Distribute the [UI notices](ui/public/THIRD_PARTY_NOTICES.txt) with frontend assets and the [Canister notices](docs/licenses/canister-notices.txt) alongside the corresponding Wasm. The [inventory](docs/licenses/dependency-inventory.json) records versions, declared licenses, text hashes, and upstream sources. These are conservative dependency closures, not measurements of which code survives compilation.

## Outstanding conditions

Reown AppKit and WalletConnect components use the Reown Community License, with separate branding, network, usage, and commercial conditions. The required notice is included in the UI notice file:

> Portions © 2025 Reown, Inc. All Rights Reserved

That notice alone does not establish compliance with the other conditions. Project Apache-2.0 licensing does not change these dependency terms.

Five UI package versions declare MIT but lack a collected license body at their pinned upstream revision: `encode-utf8@1.0.3`, `react-remove-scroll-bar@2.3.8`, `tr46@0.0.3`, and `uint8arrays@3.1.0` / `3.1.1`. Their inventory status remains `metadata-only`; notice completeness is unresolved. Do not describe this audit as complete distribution clearance.
