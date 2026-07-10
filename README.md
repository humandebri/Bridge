# SNS–Base Bridge

SNSトークンをICPとBaseの間で1:1に裏付けるBridge。本リポジトリはPhase 1BのbSNS・Deposit mint実装段階にある。

現在の`bridge-canister`は空のCandid serviceである。Base側はERC-20 bSNS、EIP-3009署名送金、Bridge SignerによるDeposit mint、fee控除、重複防止、fixed-window流量制限までを実装している。Withdrawal、pause操作、limit・fee変更、role rotationは未実装であり、資産を受け付けず、本番deployしない。

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

`contracts`はPhase 1A interfaceのfunction/error selector、event topic、型順序に加え、bSNS、EIP-3009、単体・batch Deposit、fixed-window境界を検証する。`proofs`はproductionと共有するDeposit算術をSMTCheckerで証明し、意図的に境界検査を欠くfixtureが拒否されることも確認する。証明範囲と外部仮定は[verification/README.md](verification/README.md)に記録する。

## ローカルdeploy

`smoke`は次を自動実行する。

1. 新規networkの起動時だけ、port 8000が使用中なら`gateway.port`を一時的に空きportへ変更する。
2. ICP CLI内蔵のローカルPocketIC networkを起動する。
3. `bridge-canister`をdeployし、`Running`を確認する。
4. Anvilをchain ID 31337で起動する。
5. unlock済みローカルaccountから`Bridge`をdeployし、constructorが生成したbSNSのruntime bytecode、相互参照、metadataを確認する。
6. Bridge Signerからsmoke用Depositをmintし、net残高とprocessed状態を確認する。
7. 本スクリプトが起動したprocessだけを終了し、一時変更した`icp.yaml`を復元する。

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
