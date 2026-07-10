# SNS–Base Bridge

SNSトークンをICPとBaseの間で1:1に裏付けるBridge。本リポジトリはPhase 0の基盤段階にある。

現在の`bridge-canister`は空のCandid service、`BSNS`と`Bridge`は外部操作関数を持たない空contractである。資産を受け付けず、本番deployしない。

設計判断は[plan.md](plan.md)、用語は[CONTEXT.md](CONTEXT.md)、安全上の決定は[docs/adr](docs/adr)を参照する。

## 固定ツール

| Tool | Version |
|---|---:|
| Rust | 1.93.0 |
| ICP CLI | 0.2.7 |
| ICP Rust recipe | `@dfinity/rust@v3.2.0` |
| ICP local network launcher | `v15.0.0-2026-07-02-07-40` |
| Foundry / Anvil | 1.7.1 |
| Solidity | 0.8.35 |
| Z3 | 4.15.4 |
| Verus | 0.2026.05.05.d03e906 |

Rustは`rust-toolchain.toml`、Rust依存は`Cargo.lock`、Solidity compilerとEVM targetは`contracts/foundry.toml`で固定する。

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

`proofs`は成功fixtureだけでなく、意図的に誤ったSMTChecker/Verus fixtureが拒否されることも確認する。Phase 0ではBridge固有の性質をまだ証明しない。証明範囲と未検証事項は[verification/README.md](verification/README.md)に記録する。

## ローカルdeploy

`smoke`は次を自動実行する。

1. 新規networkの起動時だけ、port 8000が使用中なら`gateway.port`を一時的に空きportへ変更する。
2. ICP CLI内蔵のローカルPocketIC networkを起動する。
3. `bridge-canister`をdeployし、`Running`を確認する。
4. Anvilをchain ID 31337で起動する。
5. unlock済みローカルaccountから`BSNS`と`Bridge`をdeployし、runtime bytecodeを確認する。
6. 本スクリプトが起動したprocessだけを終了し、一時変更した`icp.yaml`を復元する。

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
