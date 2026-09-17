# Base Sepolia contract experiment

This runbook defines how to resume or rerun the contract-only Timelock, Bridge, and bSNS experiment on Base Sepolia.
Use the state machine in `scripts/base-sepolia-experiment/` for actual transaction submission and manifest updates.
Do not connect an IC canister or KINIC Ledger.

## Experiment boundaries

- Network: Base Sepolia; chain ID `84532`.
- Default public RPC: `https://base-sepolia-rpc.publicnode.com`.
- Fix RPC URLs, chain ID, and each URL's upstream chain throughout the rehearsal. Individually check `eth_chainId` on all three providers before deployment/activation; do not start on any failure or mismatch.
- Do not use credential-bearing RPC URLs because they may be saved in the public manifest.
- Use test-only wallets, never reusing them for production keys or ceremonies.
- This experiment's deployer doubles as Base Admin wallet and Runtime Administrator.
- Use a separate Bridge Signer wallet.
- This configuration does not satisfy production-profile role separation.

## Public values

- **Deployer, Base Admin, Runtime Administrator**: `0x7F4743128368CdeD5413E8c42C9Bd689ea64D192`
- **Bridge signer**: `0xF96808b465638E88Ed4602b3852Ce7AC92E57721`
- **Timelock delay**: `259200` seconds (72 hours).
- **Per-Deposit Limit**: `1000000000` raw
- **Mint Window Limit**: `10000000000` raw
- **Mint Window Duration**: `3600` seconds.
- **MAX_SERVICE_FEE**: `10000000` raw
- **Initial Service Fee**: `1000000` raw

These are historical experiment values deployed on July 13, 2026; preserve them as evidence.
For the next redeployment, set **Per-Deposit Limit** and **Mint Window Limit** each to `15000000000000` raw (150,000 KINIC, approximately 2.5% of total supply), **MAX_SERVICE_FEE** to `1000000000` raw (10 KINIC), and **Initial Service Fee** to `50000000` raw (0.5 KINIC).

July 13, 2026 preflight observed chain ID `84532`, deployer balance `99000000000000000` wei, and nonce `0`.
Save the observed block and timestamp in the dated manifest.

## Key preparation

Never save private keys, seeds, or keystore passwords in the repository, shell arguments, or shell history.
Enter keys interactively into encrypted Foundry keystores and passwords interactively into macOS Keychain.

```sh
cast wallet import kinic-base-sepolia-experiment --interactive
cast wallet import kinic-base-sepolia-bridge-signer --interactive

security add-generic-password -U \
  -a "$USER" \
  -s kinic-base-sepolia-experiment-keystore \
  -w

security add-generic-password -U \
  -a "$USER" \
  -s kinic-base-sepolia-bridge-signer-keystore \
  -w
```

Place `-w` last.
`security` saves the interactively entered value to Keychain without displaying it.

## Execution stages

Each stage checks current manifest state and never resubmits completed transactions.
Run stages requiring signatures through the Keychain wrapper.

```sh
scripts/base-sepolia-experiment/run-with-keychain.sh preflight
scripts/base-sepolia-experiment/run-with-keychain.sh deploy
scripts/base-sepolia-experiment/run-with-keychain.sh flow
scripts/base-sepolia-experiment/run-with-keychain.sh schedule
```

Proceed in this order:

```text
PREFLIGHT
  -> READY_TO_DEPLOY
  -> DEPLOYED
  -> FLOW_COMPLETE
  -> WAITING_TIMELOCK
  -> COMPLETE
```

`preflight` checks chain ID, wallet addresses, Foundry tests, ABI drift, absence of mutable-limit selectors, deployment gas, and maximum experiment cost.
Do not broadcast if estimated maximum cost exceeds `0.02 ETH`.

`deploy` first sends test ETH to the Bridge Signer, then deploys the 72-hour Timelock and Bridge.
The Bridge creates bSNS in its constructor.
Confirm each transaction through a Finalized block. If unconfirmed after 30 minutes, stop without submitting a replacement at the same nonce.

`flow` performs Deposit minting, Withdrawal creation, Service Fee changes, and pauses Deposits/Withdrawals. Also verify that no additional Base transaction follows a Withdrawal.

`schedule` schedules Deposit/Withdrawal unpause as a Timelock batch.
Immediately submit execute as a real transaction, verifying its reverted receipt and the 72-hour delay.

## Resume after 72 hours

Run `resume` at or after `timelock_operation.ready_timestamp` in a newly generated manifest for an EIP-712-compatible Bridge. Never reuse old-ABI manifests.
`resume` executes the unchanged scheduled payload, verifies unpause, then pauses both directions again and restores the initial Service Fee.

```sh
jq '.timelock_operation.ready_timestamp' deployments/base-sepolia-contract-experiment.json
scripts/base-sepolia-experiment/run-with-keychain.sh resume
scripts/base-sepolia-experiment/experiment.sh verify
```

`verify` is read-only, rereading contract code, roles, fixed limits, asset state, all receipts, Finalized blocks, and final pause state through RPC.

## Manifest handling

`deployments/base-sepolia-contract-experiment.json` is the working state-machine manifest created when execution starts.
Scripts update addresses, nonces, transaction hashes, receipt blocks, confirmations, runtime bytecode hashes, and check results.

Save dated public records to `deployments/base-sepolia/YYYY-MM-DD/manifest.json`.
Leave unexecuted items `pending`; never invent addresses or transaction hashes.
At experiment completion, copy verified public values from the working manifest into the dated manifest.

Store none of the following in either manifest:

- Private keys or seeds.
- Keystore passwords or password files.
- hardware wallet backup
- Credential-bearing RPC URLs.
- Secret shell environment values.

## Completion criteria

Record the Bridge's test-only status in the manifest at completion.
For the historical July 13, 2026 experiment, Deposit minting and Withdrawals remain paused and Service Fee is `1000000` raw.
For the next redeployment, initial Service Fee is `50000000` raw. Test administrator fee changes during asset flows and restore `50000000` raw before completion.
Use `verify` to recheck Timelock, Bridge, and bSNS addresses/runtime bytecode hashes and every transaction confirmation.
