# Third-party dependencies

Project-authored code uses Apache-2.0. Dependencies and vendored sources retain their upstream licenses, copyright notices, and attribution requirements.

OpenZeppelin Contracts is pinned as a Git submodule under `contracts/lib/openzeppelin-contracts`; its upstream [license](contracts/lib/openzeppelin-contracts/LICENSE) applies to those sources. Nested upstream test libraries retain the license files distributed with them.

Rust and JavaScript dependency revisions are recorded in `Cargo.lock`, `pnpm-lock.yaml`, and `ui/pnpm-lock.yaml`. These dependencies do not inherit the project's license. When distributing compiled Wasm, frontend assets, or other bundles, review the dependencies actually included and supply their required notices. This file is an inventory entry point, not a completed license audit of every transitive dependency.
