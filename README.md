# SNS–Base Bridge

SNSトークンをICPとBaseの間で1:1に裏付けるBridge。
本リポジトリはPhase 1Eのcontract検証とABI凍結段階にある。

現在の`bridge-canister`は空のCandid serviceである。
Base側はERC-20 bSNS、EIP-3009、DepositとWithdrawal、独立pause、limitとfeeの変更、role rotationを実装し、危険方向の操作をOpenZeppelinの72時間Timelockへ接続している。
concrete BridgeとBSNSのABI snapshot、selector・topic fixture、stateful invariant、SMT gateを閉じる段階であり、資産を受け付けず、本番deployしない。

Base ABIは[docs/base-interface.md](docs/base-interface.md)、設計判断は[plan.md](plan.md)、用語は[CONTEXT.md](CONTEXT.md)、安全上の決定は[docs/adr](docs/adr)を参照する。

## 固定ツール

| Tool | Version |
|---|---:|
| Rust | 1.97.0 |
| ICP CLI | 1.0.2 |
| ICP Rust recipe | `@dfinity/rust@v3.3.0` |
| ICP local network launcher | `v15.0.0-2026-07-02-07-40` |
| Foundry / Anvil | 1.7.1 |
| Solidity | 0.8.36 |
| OpenZeppelin Contracts | 5.6.1 (`5fd1781b1454fd1ef8e722282f86f9293cacf256`) |
| Z3 | 4.16.0 |
| Verus | 0.2026.07.05.49b8806 |

Rustは`rust-toolchain.toml`、Rust依存は`Cargo.lock`、Solidity compilerとEVM targetは`contracts/foundry.toml`、OpenZeppelinはgit submoduleのcommitで固定する。Verusが内部で要求するRust 1.96.0はCIのVerus導入stepで別途固定する。

## 検証

全検証とローカルdeploy smokeを実行する。

```bash
scripts/ci-local.sh all
```

deployを除く検証だけを実行する。

```bash
scripts/ci-local.sh checks
```

個別実行:

```bash
scripts/ci-local.sh versions
scripts/ci-local.sh rust
scripts/ci-local.sh contracts
scripts/ci-local.sh proofs
scripts/ci-local.sh icp
scripts/ci-local.sh smoke
```

`contracts`はPhase 1A interfaceのselectorと型順序に加え、concrete ABI snapshot、bSNS、EIP-3009、Deposit、Withdrawal、管理権限、Timelock、stateful invariant、coverage summaryを検証する。
`proofs`はproductionと共有するDeposit、Withdrawal、管理判定coreをSMTCheckerで証明し、意図的に制約を欠くfixtureが拒否されることも確認する。
証明範囲と外部仮定は[verification/README.md](verification/README.md)と[verification/obligations.md](verification/obligations.md)に記録する。

ABI snapshotは次で明示的に更新し、通常のCIは更新を行わず差分だけを検出する。

```bash
python3 scripts/abi_snapshot.py --update
python3 scripts/abi_snapshot.py --check
```

## ローカルdeploy

`smoke`は次を自動実行する。

1. 新規networkの起動時だけ、port 8000が使用中なら`gateway.port`を一時的に空きportへ変更する。
2. ICP CLI内蔵のローカルPocketIC networkを起動する。
3. `bridge-canister`をdeployし、`Running`を確認する。
4. Anvilをchain ID 31337で起動する。
5. 72時間delay、Safe限定proposer・canceller・executor、自己adminでOpenZeppelin `TimelockController`をdeployする。
6. Timelock addressをBase Adminとして`Bridge`をdeployし、constructorが生成したbSNSのruntime bytecode、相互参照、metadataを確認する。
7. Bridge Signerからsmoke用Depositをmintし、Withdrawalの`Pending → Released`、同一acknowledgement、別Withdrawalの`Pending → Refunded`を確認する。
8. Runtime AdministratorのService Fee変更と独立pause、Safeの直接unpause拒否、72時間前のTimelock execute拒否、経過後のunpauseを確認する。
9. burn・refund後の残高、supply、mint window、Withdrawal連番を確認する。
10. 本スクリプトが起動したprocessだけを終了し、一時変更した`icp.yaml`を復元する。

既に起動中の当該ICP project networkは設定を変更せず再利用し、停止しない。実行中に`icp.yaml`が別途変更された場合、その変更を上書きしない。port 8545に別EVM nodeが存在する場合は再利用せず停止する。

手動確認:

```bash
scripts/prepare_local_network.py --project-root . --write
icp network start -d --project-root-override .
icp network status --json --project-root-override .
icp deploy -e local --project-root-override .
icp canister status bridge-canister -e local --json --project-root-override .
icp network stop --project-root-override .
```

手動実行の`prepare_local_network.py --write`は`icp.yaml`を永続的に変更する。必要なら停止後に利用者が元のportへ戻す。

`--mode reinstall`はstateを削除するため使用しない。
