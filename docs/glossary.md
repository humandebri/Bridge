# KINIC–Base Bridge

The context for moving KINIC tokens between ICP and Base and managing backing liabilities across both chains.

This Bridge is dedicated to KINIC. The terms `SNS token` and `bSNS` below are generic design terms; the actual asset is KINIC, with Base ERC-20 metadata `name = "KINIC"` and `symbol = "KINIC"`.

## Language

**Deposit**:
A request to lock SNS tokens on ICP and mint the corresponding amount of bSNS on Base after deducting the Service Fee.
_Avoid_: Bridge transaction, transfer

**Escrowed Unquoted**:
A state in which a Deposit's Ledger pull is confirmed, but its quote against Finalized Base state and mint reservation are not yet finalized. Transient RPC failures or observation disagreement leave it in this state; no zero-valued quote is stored.
_Avoid_: Zero quote, failed deposit, reserved mint

**Deposit Refund**:
A compensating transfer returning the gross amount minus the fixed Ledger fee to the original IC account when fresh Base validation after the Ledger pull definitively rejects a Deposit due to pause, fee, limit, or reserve conditions. No Service Fee is finalized; the refund amount plus Ledger fee is fixed to the gross amount.
_Avoid_: Service Fee refund, Base refund, arbitrary payout

**Withdrawal**:
A request to burn bridged tokens on Base and release SNS tokens on ICP. No Base refund is provided.
_Avoid_: Return transfer, redeem transaction

**Bridge Exposure**:
The sum of outstanding bridged tokens on Base and Withdrawals already burned but not yet confirmed as released on ICP. It represents the total liability backed by the Bridge.
_Avoid_: Outstanding supply, moved amount

**Per-Deposit Limit**:
The maximum amount accepted for a single Deposit. It is not a cap on cumulative transfers or Bridge Exposure.
_Avoid_: Supply cap, bridge cap

**Mint Throughput Limit**:
A rate limit on the amount that can be minted through Deposits within a given period. It limits the spread of damage over a short period without prohibiting movement of the entire supply.
_Avoid_: Daily supply cap, outstanding cap

**Bridgeable SNS Token**:
SNS tokens held in a transferable user ledger account and not staked in a neuron.
_Avoid_: Total supply, staked token

**bSNS**:
An ERC-20 on Base backed 1:1 by Bridgeable SNS Tokens. It carries no SNS Governance voting rights or neuron permissions.
This is an internal generic term and does not imply adding a `b` prefix to ERC-20 metadata. For the KINIC deployment, both the token name and symbol are `KINIC`.
_Avoid_: Cross-chain governance token, voting token

**Withdrawal Settlement**:
The state in which a Withdrawal's ICP Release is confirmed by Ledger success or history reconciliation and becomes `Paid`. There is no Base refund, and the Base state is terminal after `Committed`.
_Avoid_: Withdrawal completion, payout status

**Service Fee**:
A fixed fee charged in SNS tokens for each successful Deposit or Withdrawal. It is not proportional to the transfer amount and cannot exceed the cap fixed at deployment.
_Avoid_: Gas fee, percentage fee, spread

**Fee Recipient**:
The current SNS ledger account receiving finalized Service Fees. When an administrator changes it, the destination for the entire unpaid fee reserve also changes to the new account.
_Avoid_: Treasury owner, escrow owner

**Settlement Reserve**:
The portion of a single Bridge resource balance logically reserved to prioritize completion of existing Withdrawal Settlements.
_Avoid_: Settlement wallet, separate treasury

**Asset Safety**:
The property that the Bridge does not treat ambiguous external outcomes as success and prevents double minting, double release, and fee spending beyond backing. Formal proofs and tests guarantee only what falls within their explicit models and external assumptions.
_Avoid_: Guaranteed recovery, guaranteed availability

**Settlement Liveness**:
The property that an accepted Deposit or Committed Withdrawal can reach its final state. It depends on RPC, Ledger, threshold signing, cycles, wallet consent, and operational replenishment; this Bridge does not guarantee eventual completion.
_Avoid_: Asset Safety, automatic recovery

**Reconciliation Hold**:
A state in which an external transfer's outcome cannot be established and compensating operations are prohibited to avoid duplicate processing. Time alone does not clear it.
_Avoid_: Timed out, failed, retryable

**Refund Reconciliation Hold**:
A state in which a Deposit Refund's Ledger outcome cannot be established, retaining the same refund identity and evidence search to prevent double refunds. Success evidence advances it to Refunded; only a complete proof of absence permits a new attempt with the same economic payload.
_Avoid_: Refund retry, timed-out refund, manual refund

**Deposit Cancellation**:
A terminal outcome that makes a Deposit ID permanently unusable after a complete history scan proves its ICP pull absent. It involves no token movement or Service Fee finalization.
_Avoid_: Retry, timeout, reopen

**Withdrawal Transfer Attempt**:
A specific ICRC release identity for one Withdrawal. Only a complete proof of absence permits incrementing the attempt number and assigning a new identity while preserving the economic payload.
_Avoid_: Withdrawal retry, replacement withdrawal

**Ledger History Watermark**:
The inclusive ledger block index through which reconciliation has established completeness. It is evidence of absence only when the first unscanned block exceeds the ledger tip and the exact transfer search, including archives and index, is complete.
_Avoid_: Timestamp, timeout, last attempted block

**Upgrade Authority**:
The SNS Governance authority that approves Bridge canister code upgrades. SNS Root, as sole controller, executes adopted upgrades.
_Avoid_: Runtime administrator, developer controller

**Governance Principal**:
The same SNS Governance principal as the Upgrade Authority. It controls Canister administration and the Governance Operator lane for submitting the closed set of Base administration operations.
_Avoid_: developer controller, human EVM wallet

**Pause Principal**:
A single IC principal authorized only to pause IC/Base, cancel recorded pending Timelock operations, and advance permitted Settlements. It cannot unpause, rotate roles, manage fees, or upgrade.
_Avoid_: Governance Principal, canister controller

**Governance Operator**:
A threshold address derived by the Bridge Canister on a path separate from the Mint Signer, submitting only delayed unpause and role rotation through Timelock propose/execute.
_Avoid_: human wallet, Mint Signer, canister controller

**Runtime Administrator**:
A threshold address derived by the Bridge Canister on a path separate from the Governance Operator, submitting only immediate Base pauses and Service Fee changes within the cap.
_Avoid_: Governance Operator, Independent Canceller, human wallet

**Independent Canceller**:
A threshold address derived by the Bridge Canister on a path separate from the Governance Operator, submitting only cancellations of recorded pending Timelock operations.
_Avoid_: Governance Operator, Runtime Administrator, human wallet

**Bridge Signer**:
A single address managed through the Bridge canister's threshold ECDSA and used only for Base Deposit minting. Users execute Withdrawal burns.
_Avoid_: Base Admin, Runtime Administrator, owner

**Base Admin Timelock**:
An OpenZeppelin Timelock with the Governance Operator as sole proposer/executor and the separately derived Independent Canceller as sole canceller, retaining self-administration and a 24-hour minimum delay. No roles are granted to human wallets.
_Avoid_: human wallet, external DEFAULT_ADMIN_ROLE holder
