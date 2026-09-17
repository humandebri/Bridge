# Architecture decision records

Accepted ADRs record design decisions. Read them alongside the current [state machine](../canister-state-machine.md), [bridge flows](../bridge-flow.md), and [operations runbook](../runbooks/operations.md). Superseded records preserve historical reasoning and are not current implementation guidance.

## Accepted decisions

| ADR | Decision |
|---|---|
| 0001 | [Allow full-supply transfers with per-Deposit and mint throughput limits](0001-allow-full-supply-with-bounded-mint-throughput.md) |
| 0002 | [Give bSNS no SNS Governance rights](0002-bsns-has-no-sns-governance-rights.md) |
| 0004 | [Charge a fixed SNS-token fee at Bridge request finalization](0004-charge-a-fixed-sns-token-service-fee.md) |
| 0005 | [Logically reserve settlement cycles and mint capacity](0005-use-a-logical-settlement-reserve.md) |
| 0006 | [Keep ambiguous ledger transfers in Reconciliation Hold](0006-hold-ambiguous-ledger-transfers-indefinitely.md) |
| 0008 | [Keep the Bridge canister upgradeable and hand it over to SNS control](0008-handover-bridge-upgrades-to-sns-control.md) |
| 0010 | [Deploy each Bridge for a single SNS token](0010-deploy-one-bridge-pair-per-sns-token.md) |
| 0011 | [Ingest Withdrawals through browser notification and a single synchronous verification](0011-confirm-withdrawals-by-finalized-state-reads.md) |
| 0012 | [Implement the Mint Throughput Limit with fixed windows](0012-use-fixed-windows-for-mint-throughput.md) |
| 0013 | [Use the Base Service Fee as the canonical quote](0013-use-base-service-fee-as-canonical-quote.md) |
| 0014 | [Create bSNS in the Bridge constructor](0014-bind-bsns-to-bridge-at-construction.md) |
| 0015 | [Support EIP-3009 authorized transfers in bSNS](0015-support-eip-3009-authorized-transfers.md) |
| 0016 | [Use OpenZeppelin TimelockController as the Base Admin execution boundary](0016-use-openzeppelin-timelock-for-base-admin.md) |
| 0018 | [Treat Withdrawals as irreversible Committed burns](0018-user-executed-withdrawals-and-safe-settlement.md) |
| 0019 | [Centralize settlement execution in a stable SQLite job queue](0019-use-a-stable-settlement-executor.md) |
| 0022 | [Create formal Deposits only after successful Ledger funding](0022-fund-before-formal-deposit.md) |
| 0023 | [Use wallet-submitted EIP-712 Mint Authorizations](0023-use-wallet-funded-eip712-mint-authorization.md) |
| 0024 | [Validate RPC chain binding before runtime](0024-validate-rpc-chain-binding-before-runtime.md) |
| 0025 | [Prohibit Canister reinstall and fix the Withdrawal history boundary](0025-prohibit-canister-reinstall-and-bind-withdrawal-history.md) |
| 0026 | [Replace the staging OnFinality provider with dRPC](0026-replace-staging-onfinality-with-drpc.md) |
| 0027 | [Use a dedicated EOA for initial Base deployment](0027-use-dedicated-eoa-for-initial-base-deployment.md) |
| 0028 | [Confirm individual transactions and list Canister-recorded history](0028-confirm-transactions-and-list-recorded-history.md) |

## Superseded decisions

| ADR | Historical decision |
|---|---|
| 0003 | [Acknowledge ICP Release on Base](0003-acknowledge-icp-release-on-base.md) |
| 0007 | [Make SNS Governance the Base admin authority](0007-use-sns-governance-as-the-base-admin-authority.md) |
| 0009 | [Split Base admin authority into immediate pause and delayed recovery](0009-split-base-admin-into-safety-and-danger-authorities.md) |
| 0021 | [Separate Deposit admission and Ledger pull with a stable executor](0021-pull-before-quote-and-refund-rejected-deposits.md) |

See the [ADR archive](archive/README.md) for superseded Deposit confirmation decisions 0017 and 0020. Existing paths are retained so references and historical evidence remain readable.
