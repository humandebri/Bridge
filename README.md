# KINIC–Base Bridge

A bridge that maintains 1:1 backing between KINIC on the Internet Computer and its ERC-20 representation on Base.

Users deposit KINIC into the ICP Ledger escrow and mint on Base with a signed authorization. In the other direction, they burn tokens on Base and receive KINIC on ICP after finalized verification. The repository includes Rust Canisters, Solidity contracts, a React UI, operational tools, and formal verification.

## Production status

On October 1, 2026 at 09:03 JST, a certified IC `read_state` observation returned Kinic SNS Root as the sole controller of the production Bridge. The individual production controller is no longer in the controller set.

| Role | Principal |
|---|---|
| Bridge | `lb5i5-ziaaa-aaaar-qcgwq-cai` |
| Kinic SNS Root — sole Bridge controller | `7jkta-eyaaa-aaaaq-aaarq-cai` |
| Kinic SNS Governance | `74ncn-fqaaa-aaaaq-aaasa-cai` |
| KINIC Ledger | `73mez-iiaaa-aaaaq-aaasq-cai` |
| KINIC Index | `7vojr-tyaaa-aaaaq-aaatq-cai` |

The observed module SHA-256 was `0c63f4646fd2a6b1cfb3a2835b284641e4f794a1627cf982b6fd4ab225aa15e4`. This is a dated observation, not deployment authorization or proof of runtime readiness.

A same-Wasm upgrade through SNS Governance is deferred until an upgrade is needed. Its successful SNS Root `post_upgrade` observation and state-preservation checks remain unverified in this documentation. Root-only control alone does not establish completion of the full handover validation procedure.

The production compatibility baseline is stable schema v36 / record wire v30. This controller observation did not recheck schema, activation, pause state, reserves, storage integrity, history indexes, Root dapp registration, or UI readiness. Verify current authenticated state using the [operations runbook](docs/runbooks/operations.md) before any production action.

## Repository layout

| Directory | Contents |
|---|---|
| `canister/` | Deterministic Rust core, IC adapter, and test Canisters |
| `contracts/` | Base Bridge, token, Timelock integration, and Solidity tests |
| `ui/` | React/Vite frontend and browser tests |
| `tools/` | Release-profile validation and Governance proposal/relay tools |
| `verification/` | Claims, proofs, implementation links, and external assumptions |
| `docs/` | Architecture, interfaces, decisions, and operations |
| `deployments/` | Deployment formats and dated public evidence |
| `plans/` | Development history and future proposals |

## Get started

Install the [pinned tools](docs/development.md#pinned-tools), then prepare a clone:

```sh
git submodule update --init --recursive
pnpm install --frozen-lockfile
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui exec playwright install chromium
scripts/ci-local.sh versions
```

Run the focused checks for the component you are changing:

```sh
scripts/ci-local.sh rust-fast
scripts/ci-local.sh contracts-fast
scripts/ci-local.sh ui-fast
```

The local deployment smoke test expects schema version 36. The [development guide](docs/development.md) explains integration tests, proof gates, and local deployment. Production artifacts must use the reviewed production build; the `test-deployment` feature is reserved for test environments.

## Documentation

- [Documentation index](docs/README.md)
- [Architecture](docs/architecture.md) and [Bridge flows](docs/bridge-flow.md)
- [Verification scope and assumptions](verification/README.md)
- [Development and validation](docs/development.md)
- [Production operations](docs/runbooks/operations.md) and [DAO handover](docs/runbooks/dao-reactivation.md)
- [Contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md)

Maintained documentation and user-facing messages are written in English. Dated evidence and implementation plans describe their recorded context and do not authorize current production operations.

## License

Project-authored code is licensed under [Apache-2.0](LICENSE), as declared by the Rust workspace. Third-party dependencies retain their own licenses and notices; see [third-party notices](THIRD_PARTY_NOTICES.md).
