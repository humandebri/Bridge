# Base Sepolia contract-only experiment

This directory drives the test-only Base Sepolia experiment as an idempotent
state machine:

```text
PREFLIGHT -> READY_TO_DEPLOY -> DEPLOYED -> WAITING_TIMELOCK
          -> ACTIVE -> COMPLETE
```

The default RPC is `https://base-sepolia-rpc.publicnode.com` and every stage
rejects any chain other than `84532`. The public manifest is written to
`deployments/base-sepolia-contract-experiment.json`. It contains addresses,
transaction identities, receipt/confirmation evidence, constructor values, and
source hashes, but no key material or RPC credentials. A manifest from the
obsolete canister-sent Mint ABI must not be reused; start from `preflight`.

Wallets are encrypted Foundry keystores. Their passwords live in macOS
Keychain and are copied to mode-600 temporary files only for the lifetime of a
stage:

```sh
scripts/base-sepolia-experiment/run-with-keychain.sh preflight
scripts/base-sepolia-experiment/run-with-keychain.sh deploy
scripts/base-sepolia-experiment/run-with-keychain.sh schedule

# Run after the manifest's timelock_operation.ready_timestamp.
scripts/base-sepolia-experiment/run-with-keychain.sh resume
scripts/base-sepolia-experiment/run-with-keychain.sh flow
scripts/base-sepolia-experiment/experiment.sh verify
```

Fresh staging uses the same state machine with a repository-external manifest and
the explicitly test-only five-minute policy:

```sh
BASE_SEPOLIA_MANIFEST=/secure/evidence/contracts.json \
BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS=300 \
BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true \
BASE_SEPOLIA_EXTERNAL_BRIDGE_SIGNER=0x... \
BASE_SEPOLIA_EXTERNAL_GOVERNANCE_OPERATOR=0x... \
BASE_SEPOLIA_EXTERNAL_RUNTIME_ADMINISTRATOR=0x... \
BASE_SEPOLIA_EXTERNAL_INDEPENDENT_CANCELLER=0x... \
scripts/base-sepolia-experiment/run-with-keychain.sh preflight
```

The 300-second override is rejected unless the test-only acknowledgement is set.
It must never be used for production or Gate B evidence.
The four external addresses must be derived from the fresh IC Canister's
`key_1` control plane. In this mode only the deployer keystore is loaded, the
deployer receives no Timelock or Bridge role, and no ETH is sent to the signer.

`preflight` runs Foundry, ABI drift, formatting, and diff checks. It estimates
both deployments and adds a conservative five-million-gas call budget. No
broadcast is permitted when the resulting cost exceeds `0.02 ETH`.

Bridgeは両asset flowをpauseした状態で配置される。`schedule`と72時間後の
`resume`がTimelock経由で両方を有効化するまで、`flow`は実行できない。
`flow`完了時には両方を再度pauseする。

Every successful transaction must reach the RPC's `safe` block within 30
minutes. A timeout changes the manifest to `PENDING_CONFIRMATION` and stops without
submitting a replacement nonce. Completed transaction names are never resent;
the script rereads their receipts instead.

The funded wallet is intentionally both the Base Admin wallet and Runtime
Administrator for this experiment. This does not satisfy the complete
production role-separation profile. The Bridge signer and Timelock canceller
are separate, independently encrypted test wallets; the canceller is distinct
from the proposer and executor even in rehearsal. This contract-only rehearsal
does not create an IC ledger block or a release acknowledgement.
