# Bridge documentation

The KINIC–Base Bridge maintains 1:1 backing between KINIC on ICP and its ERC-20 representation on Base. Use this index to find the current design, operating procedures, implementation history, and verification evidence.

## Start here

| Document | Purpose |
|---|---|
| [Repository overview](../README.md) | Current status, pinned tools, setup, and validation commands |
| [Bridge flows](bridge-flow.md) | Deposit, mint authorization, refund, and Withdrawal execution |
| [Glossary](glossary.md) | Shared terminology and distinctions between safety and liveness |
| [Canister state machine](canister-state-machine.md) | State transitions, persistence, and external-call boundaries |
| [Base interface](base-interface.md) | Contract interfaces and ABI behavior |
| [Parameters](parameters.md) | Limits, fees, timing, and operating parameters |
| [UI development](../ui/README.md) | Frontend setup, runtime profiles, and checks |

## Design and decisions

- [Implementation phases](implementation-plan.md) describe the implementation structure.
- [Architecture decision records](adr/README.md) explain accepted decisions and link to superseded history.
- [Implementation plan index](../plans/README.md) is the source of truth for plan progress. Completed plans are historical records; use the current design documents for current behavior.
- [Deposit and Timelock audit](audits/2026-09-08-deposit-timelock.md) records the dated review.

## Operations

| Runbook | Use it for |
|---|---|
| [Operations](runbooks/operations.md) | Production gates, activation, upgrades, monitoring, and incident handling |
| [DAO reactivation](runbooks/dao-reactivation.md) | Governance-controlled reactivation |
| [Emergency pause principal](runbooks/emergency-pause-principal.md) | The emergency principal's authority and operating boundaries |
| [Token publication](runbooks/token-publication.md) | Publishing token metadata and related artifacts |
| [Bridge error messages](runbooks/bridge-error-messages.md) | Interpreting errors and choosing recovery actions |
| [UI wallet compatibility](runbooks/ui-wallet-compatibility.md) | Wallet compatibility checks |
| [Base Sepolia rehearsal](runbooks/base-sepolia-rehearsal.md) | Contract and governance rehearsal |
| [EVM RPC Canister rehearsal](runbooks/evm-rpc-canister-rehearsal.md) | RPC integration rehearsal |
| [Sepolia staging E2E](runbooks/sepolia-staging-e2e.md) | End-to-end staging and promotion evidence |
| [Governance relayer](../tools/governance-relayer/README.md) | Broadcasting and confirming signed governance transactions |

## Deployment and evidence

- [Deployment artifacts](../deployments/README.md): artifact layout and production evidence requirements.
- [Evidence v1](../deployments/evidence-v1/README.md): evidence format and retained records.
- [Sepolia staging evidence](../deployments/sepolia-staging/evidence/README.md): staging evidence layout.
- [KINIC mainnet observations](evidence/kinic-mainnet-2026-07-13.md): dated Ledger and Index observations.
- [Base Sepolia experiment](../scripts/base-sepolia-experiment/README.md): experiment commands and outputs.

Dated evidence and archived decisions describe their recorded state. They do not override the current production baseline or authorize deployment. Follow the current operations runbook and repository policy for production gates.

## Verification

- [Verification overview](../verification/README.md): proof stages, claims, and external assumptions.
- [Proof outline](../verification/proof-outline.md): the structure of the safety argument.
- [Proof obligations](../verification/obligations.md): implementation obligations and evidence boundaries.
- [Conditional liveness](../verification/conditional-liveness.md): progress properties and their assumptions.
- [Audit evidence](../verification/audit-evidence.md): reviewable evidence and interpretation limits.
- [Generated claim statements](../verification/generated/claim-statements.md): exported Lean statements and specification correspondence. Update the source registries and regenerate; do not edit this artifact directly.
- [Certora](../verification/certora/README.md): supplementary verification setup.
- [Contract ABI snapshots](../contracts/abi/README.md): snapshot maintenance.

## Maintaining these documents

Write maintained documentation in English, keeping API names, claim IDs, commands, addresses, hashes, and evidence identifiers exact. Keep historical decisions and evidence explicitly historical. When a heading or path changes, update its references in the same change. Prefer linking to the authoritative document over copying operational requirements into multiple pages.
