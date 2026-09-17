# Base interface specification

This document records the concrete Base ABI and interfaces frozen in Phase 1E. Solidity declarations in `contracts/src/interfaces/` and `contracts/src/` are authoritative; concrete ABI snapshots and selector fixtures detect drift.

## Deployment

`Bridge` has the following constructor and creates `BSNS` internally.

```solidity
constructor(
    address initialBridgeSigner,
    address initialRuntimeAdministrator,
    address initialBaseAdminTimelock,
    bytes32 initialApprovedTimelockRuntimeCodeHash,
    uint256 initialPerDepositLimit,
    uint256 initialMintWindowLimit,
    uint64 initialMintWindowDuration,
    uint256 minServiceFee,
    uint256 maxServiceFee,
    uint256 initialServiceFee
)
```

The contract fixes ERC-20 metadata for the `BSNS` created by `Bridge` to `name = "KINIC"`, `symbol = "KINIC"`, and `decimals = 8`. The constructor accepts no metadata and cannot deploy different metadata. Do not add a `b` prefix such as `bKINIC`. `bSNS` is an internal generic term for Bridgeable SNS Token and is not used in token metadata.

The three authority addresses must be nonzero and distinct. Limits and window duration must be nonzero, and `0 < minServiceFee <= initialServiceFee <= maxServiceFee` is required. Fixed decimals are 8, matching KINIC Ledger `73mez-iiaaa-aaaaq-aaasq-cai`. `initialApprovedTimelockRuntimeCodeHash` is the OpenZeppelin Timelock runtime code hash checked at deployment; the Timelock address is immutable.

## EIP-3009 authorized transfers

In addition to standard ERC-20, bSNS provides the following EIP-3009 interface.

```solidity
function version() external pure returns (string memory); // "1"
function authorizationState(address authorizer, bytes32 nonce) external view returns (bool);
function transferWithAuthorization(
    address from,
    address to,
    uint256 value,
    uint256 validAfter,
    uint256 validBefore,
    bytes32 nonce,
    uint8 v,
    bytes32 r,
    bytes32 s
) external;
function receiveWithAuthorization(
    address from,
    address to,
    uint256 value,
    uint256 validAfter,
    uint256 validBefore,
    bytes32 nonce,
    uint8 v,
    bytes32 r,
    bytes32 s
) external;
function cancelAuthorization(address authorizer, bytes32 nonce, uint8 v, bytes32 r, bytes32 s) external;

event AuthorizationUsed(address indexed authorizer, bytes32 indexed nonce);
event AuthorizationCanceled(address indexed authorizer, bytes32 indexed nonce);
```

`validAfter` and `validBefore` are Unix times; an authorization is usable only while `block.timestamp > validAfter && block.timestamp < validBefore`. Used and cancelled nonces share one namespace per authorizer and cannot be reused by either authorized transfer function. `receiveWithAuthorization` requires caller equality with `to`. The EIP-712 domain binds the token name, fixed version `"1"`, execution chain ID, and bSNS contract address and is exposed through EIP-5267 `eip712Domain()`.

## Roles

| Role | Direct operations | Prohibited operations |
|---|---|---|
| Bridge Signer | Sign EIP-712 Mint Authorizations | Submit Base transactions, pause, change limits/fees, rotate roles, operate Withdrawals |
| Runtime Administrator | Pause Deposits/Withdrawals, change Service Fee within the cap | Unpause, change limits, rotate roles, mint |
| Base Admin Timelock | Unpause, rotate Bridge Signer and Runtime Administrator | Change limits, mint directly, operate Withdrawals |

Expose no generic grant API for adding arbitrary role members. Bridge Signer and Runtime Administrator each remain a single address.
Rotation also rejects zero addresses and overlap among the three authority addresses, preserving separation after initial deployment.

Use OpenZeppelin 5.6.1 `TimelockController` for the Base Admin Timelock.
Deploy it before the Bridge with a 24-hour minimum delay, the Canister-derived Governance Operator as proposer/executor, the separately derived Independent Canceller as canceller, and the zero address as additional admin. Grant no roles to human EVM administration wallets.
The Timelock itself is the sole admin. Generic `grantRole`, `revokeRole`, and `renounceRole` calls remain forbidden, including self-calls. Only a delayed Timelock self-call to `rotateOperationalMembers` atomically replaces the proposer/executor pair and independent canceller, preserving nonzero, distinct operational identities. The Bridge's Timelock address is immutable; there is no Timelock contract replacement API.
At construction, the Bridge verifies the Timelock runtime code, delay of at least 24 hours, and self-held admin role. Check role separation in the deployment profile and deployment preflight.

## Deposit mint

The Canister threshold-ECDSA-signs the following EIP-712 payload. The domain binds `name = "KINIC Bridge"`, `version = "1"`, the execution chain ID, and Bridge contract address.

```solidity
struct MintAuthorization {
    bytes32 depositId;
    address recipient;
    uint256 grossAmount;
    uint256 maxServiceFee;
    uint256 chargedServiceFee;
    uint256 deadline;
    uint256 authorizationEpoch;
}

function mintDepositWithAuthorization(
    MintAuthorization calldata authorization,
    bytes calldata signature
) external;
```

The caller is unrestricted and pays only gas; it cannot change the signed `recipient`. The contract verifies `block.timestamp <= deadline`, `authorizationEpoch == mintAuthorizationEpoch`, and that EIP-712 signature recovery yields the current `bridgeSigner`. OpenZeppelin `ECDSA.tryRecover` rejects invalid length, invalid `v`, and high-s signatures.

Verify `chargedServiceFee <= maxServiceFee` and `chargedServiceFee <= MAX_SERVICE_FEE`, applying the Per-Deposit Limit and Mint Throughput Limit to the actual mint amount `grossAmount - chargedServiceFee`. Changes to global `serviceFee` after admission do not affect existing Authorization mint amounts or event values. On success, record the EIP-712 digest as an indexed field in `DepositMinted`.

Mint each Deposit individually. Revert for a zero recipient, invalid amount, fee-protection violation, Per-Deposit Limit violation, or shared Mint Throughput Limit violation. A successful `depositId` cannot be reused; multiple mints accumulate against the same fixed-window throughput.

The fixed window begins at Bridge deployment time. Once `block.timestamp >= mintWindowStartedAt + mintWindowDuration`, the first successful mint starts the next window and resets consumption. Failed mints change neither the start nor consumption. Up to two windows' capacity can be minted immediately across a boundary, so derive limits using the factor of two in `docs/parameters.md`.

`mintAuthorizationEpoch` starts at 1. Increment it when Deposit minting changes from active to paused, from paused to active, or when the Bridge Signer actually rotates to a different address, invalidating all unexpired Authorizations created before the transition. Repeated pause, repeated unpause, and rotation to the same signer do not increment it.

## Withdrawal

Withdrawal IDs are contract-local `uint256` sequence numbers starting at 1; reserve 0 for `None`. `getWithdrawal` for a nonexistent ID returns a default struct with `status = None`. Store ICRC-1 Accounts as raw principal `bytes owner` and `bytes32 subaccount`; a zero subaccount means the default subaccount. Allow only 1–29 owner bytes, rejecting the empty management principal and anonymous principal `hex"04"`.

Before burn, `createWithdrawal(amount, maxServiceFee, owner, subaccount)` verifies current `serviceFee <= maxServiceFee` and `amount > serviceFee`. The caller first approves exactly the requested amount to the Bridge. Execution performs `transferFrom`, burns the Bridge balance, creates a `Committed` record with the following fixed quote, and emits `WithdrawalCommitted` in one transaction. Any intermediate failure reverts everything.

```text
chargedServiceFee = serviceFee at execution
chargedServiceFee <= maxServiceFee
amountOut = amount - chargedServiceFee
```

Withdrawal states are only `None | Committed`; Committed is irreversible and terminal on Base.
The ABI contains no `acknowledgeRelease`, `cancelRelease`, `refundWithdrawal`, Withdrawal-specific re-mint, or Ledger block data.
The Canister retries and reconciles the post-burn ICP liability while preserving the original Withdrawal ID and IC Account.
This constraint does not revoke the Bridge Signer's normal Deposit mint authority. Mint throughput limits and pause constrain the damage rate if a compromised Signer mints another unprocessed Deposit ID.

## Pause and fixed limits

Pause Deposit minting and Withdrawal creation independently. Pause does not stop transfer or reconciliation of Canister liabilities already Committed.

Fix the Per-Deposit Limit, Mint Throughput Limit, and window duration in the constructor. There are no functions, selectors, or administration paths to change them after deployment.

The Runtime Administrator may change `serviceFee` within immutable `MIN_SERVICE_FEE <= serviceFee <= MAX_SERVICE_FEE`.
Repeating pause, unpause, Service Fee changes, or role rotation with the same state/value succeeds without changing storage or events.
Successful role rotation immediately revokes the old address's authority.

## Phase boundaries

Phase 1D implements Service Fee changes, pause, fixed limits, role rotation, and 24-hour Timelock integration. Phase 1E closes the concrete ABI, stateful invariants, SMT proof obligations, and LCOV coverage thresholds.
Base has no fee reserve or Fee Recipient.
At Phase 1E completion, freeze the concrete Bridge and BSNS ABIs with snapshots and fixtures. Contracts at this stage must not accept production assets.
