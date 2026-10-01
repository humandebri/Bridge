# Third-party dependencies

Project-authored code uses Apache-2.0. Dependencies and vendored sources retain their upstream licenses, copyright notices, and attribution requirements.

OpenZeppelin Contracts is pinned as a Git submodule under `contracts/lib/openzeppelin-contracts`; its upstream [license](contracts/lib/openzeppelin-contracts/LICENSE) applies to those sources. Nested upstream test libraries retain the license files distributed with them.

Rust and JavaScript dependency revisions are recorded in `Cargo.lock`, `pnpm-lock.yaml`, and `ui/pnpm-lock.yaml`. Generate notices for the components included in each distributed UI or Wasm release using the [release guidance](docs/licenses/README.md). This source document does not replace distribution notices.

The October 1, 2026 review identified Reown custom community terms and five UI package versions with missing license bodies. The release guidance records those findings for review when preparing the affected distribution. It does not claim complete license clearance.
