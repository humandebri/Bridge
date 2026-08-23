# KINIC–Base Bridge

KINICトークンをICPとBaseの間で1:1に裏付けるBridge。

## 現在の状態

[実装計画の索引](plans/README.md)を進捗の正本とする。

| 対象 | 状態 | 残作業 |
|---|---|---|
| Plan 001〜004 | 完了 | 履歴資料として保持 |
| Plan 005 | 進行中 | 10回・7日外部計測、固定limit承認、pause/cancel経路演習 |
| Plan 006 | リポジトリ実装済み | SNS handover、本番preflight、mainnet evidence |
| Plan 007 | Local完了 / External待ち | 非blockingのwallet互換性・追加障害シナリオ |
| Production | 未デプロイ | 最小Gate A/Bと本番activation完了まで資産受付禁止 |

`bridge-core`はDeposit、Withdrawal、Mint Authorization、Reconciliation Hold、Settlement Reserve、会計の決定的な遷移を担う。
`bridge-canister`はstable schema v35・record wire v29の単一SQLite DBへ状態を保存し、owner sequence型Deposit API、状態照会、ICRC Ledger、EVM RPC、threshold ECDSA、運用管理APIを接続する。
ICP→BaseではCanisterはFinalized Base timestampから固定期限のEIP-712 Mint Authorizationへ署名するだけで、Base transactionを生成・送信しない。任意のBase walletが`mintDepositWithAuthorization`を送り、そのwalletがgasを支払う。
期限後、既存のBase Finalized snapshotを使うdeadline順の上限付きローカル走査でmint予約だけを解放する。Depositごとのtimerや個別Base照合、自動返金は行わない。任意の非anonymous Principalが`request_deposit_refund`を明示実行すると、同じcanonical Finalized blockで期限超過と`isDepositProcessed`を照合し、未処理ならrecordに固定された元account・金額・transfer identityでLedger refund、処理済みならexact `DepositMinted` eventとcanonical receiptを保存して`Minted`へ進む。RPC不一致、event欠落、digest不一致では資金を動かさない。
Mint用ETH reserve、gas見積り、nonce、raw transaction、rebroadcast、replacementは存在しない。Base governanceではCanisterがGovernance Operatorのtransactionをthreshold署名し、外部`governance-relayer` CLIだけがbroadcast、Finalized待機、確定通知を行う。自動replacementはなく、Governanceの明示要求時だけ同一nonceを最大3回、12.5%以上fee bumpして再署名する。
Base側はKINICを表すERC-20（`name = "KINIC"`、`symbol = "KINIC"`）、EIP-3009、DepositとWithdrawal、独立pause、固定limit、上限内Service Fee変更、role rotationを実装し、危険方向の操作をOpenZeppelinの24時間Timelockへ接続している。

Base→ICP Withdrawalはユーザーが`createWithdrawal`を送信し、その同一transactionでbSNSの`transferFrom`、burn、固定受取額を持つ`Committed`化を原子的に実行する。Canisterは同じcanonical Finalized block hashへ束縛したreceipt、event、Withdrawal state、Bridge snapshotをquorumで検証し、固定IC Accountへの債務とtransfer identityを保存する。通知成功後にUIがbrowser identityで`continue_withdrawal`を1回実行し、未完了ならHistoryの明示操作ごとにLedger送金または照合を最大1 external step進める。Canister timerによるWithdrawal再試行、Base refund、release acknowledgementはない。Finalized headまたはcanonical hashが2-of-3で収束しない場合はfail closedとし、Safeへfallbackしない。

本番Bridgeは未デプロイであり、Plan 005の10回・7日外部計測と単一emergency pause経路演習、Plan 006の主要5 scenario、SNS handover、Canister操作型production preflightが完了するまで本番資産を受け付けない。Plan 007の追加wallet互換性と追加5 scenarioは非blockingで継続する。

Base ABIは[docs/base-interface.md](docs/base-interface.md)、ブリッジの実行フローは[docs/bridge-flow.md](docs/bridge-flow.md)、実装計画は[docs/implementation-plan.md](docs/implementation-plan.md)、用語は[docs/glossary.md](docs/glossary.md)、安全上の決定は[docs/adr](docs/adr)を参照する。RPC providerのchain bindingとruntime quorumの保証境界は[ADR 0024](docs/adr/0024-validate-rpc-chain-binding-before-runtime.md)を正本とする。

## KINIC mainnet canister

| Role | Canister ID |
|---|---|
| Ledger | `73mez-iiaaa-aaaaq-aaasq-cai` |
| Index | `7vojr-tyaaa-aaaaq-aaatq-cai` |

Bridge canisterはこのLedgerとIndexだけを対象とする。Ledger metadataは`name = "KINIC"`、`symbol = "KINIC"`、`decimals = 8`である。Archive canisterは増設され得るためIDを固定せず、LedgerのICRC-3 archive discovery結果を使用する。

通常の`bridge-canister` artifactはBase mainnet（chain ID `8453`）と上記Ledger/Indexを初期化時に必須とする。PocketIC・Anvil向けの任意bindingはdefault無効の`test-deployment` featureだけが受理し、`target/test-deployment/`へ分離してbuildする。本番artifactへこのfeatureを付けない。

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
| Lean | 4.30.0 |
| Node.js | 24.14.0 |
| pnpm | 11.0.8 |

Rustは`rust-toolchain.toml`、Rust依存は`Cargo.lock`、Leanは`lean-toolchain`、Solidity compilerとEVM targetは`contracts/foundry.toml`、OpenZeppelinはgit submoduleのcommitで固定する。
Verusが内部で要求するRust 1.96.0はCIのVerus導入stepで別途固定する。

## 新規cloneの準備

```bash
git submodule update --init --recursive
pnpm install --frozen-lockfile
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui exec playwright install chromium
```

上記の固定ツールを導入したうえで`scripts/ci-local.sh versions`を実行する。
CIでの固定ツール導入手順は[`.github/workflows/ci.yml`](.github/workflows/ci.yml)を参照する。

## 検証

開発中は変更領域に対応するfast modeを実行する。

```bash
scripts/ci-local.sh rust-fast
scripts/ci-local.sh contracts-fast
scripts/ci-local.sh ui-fast
```

Wasm・PocketIC統合、coverage、ブラウザE2Eは必要に応じて個別に実行する。

```bash
scripts/ci-local.sh rust-integration
scripts/ci-local.sh contracts-coverage
scripts/ci-local.sh ui-e2e
```

PR前はdeployと実Ledger統合を除く全検証を実行する。

```bash
scripts/ci-local.sh checks
```

main更新時、夜間、リリース前は全検証とローカルdeploy smokeを実行する。

```bash
scripts/ci-local.sh all
```

既存の集約modeとその他の個別実行:

```bash
scripts/ci-local.sh versions
scripts/ci-local.sh rust
scripts/ci-local.sh contracts
scripts/ci-local.sh proofs
scripts/ci-local.sh ui
scripts/ci-local.sh icp
scripts/ci-local.sh smoke
scripts/ci-local.sh real
```

GitHub Actionsの`trusted-pr-gate`は`pull_request_target`でbase branch版classifierだけを実行し、PRの正確なhead SHAをread-only・secretなしのephemeral runnerで検証する。未知pathとCI関連変更は全suiteへfail closedする。
bootstrap merge後にBranch ProtectionまたはRulesetで`trusted-pr-gate`をrequiredかつstrictに設定する必要がある。旧`pr-gate`はrequired判定から外し、`main`へのpush、夜間schedule、手動実行では完全な`all` gateを実行する。

`contracts`はPhase 1A interfaceのselectorと型順序に加え、concrete ABI snapshot、bSNS、EIP-3009、Deposit、Withdrawal、管理権限、Timelock、stateful invariant、coverage summaryを検証する。
`proofs`はLeanをcross-chain protocolの正式な抽象仕様としてビルドし、`sorry`・`admit`を拒否する。
Leanから生成した追跡対象のconformance vectorをRust、Solidity、TypeScriptの実装に適用し、各vector sectionについてmanifestにない仕様・定理・consumerのdriftを拒否する。
manifestに登録したconsumerはsectionとproduction symbolへの構造的な結合を検査してから許可済みrunnerで個別実行し、対象testが正確に1件成功した場合だけ対応済みと判定する。
この照合は列挙した境界値に対する限定的なconformanceであり、各言語実装全体の完全なsemantic refinementではない。
productionと共有するDeposit、Withdrawal、管理判定coreはSMTCheckerとVerusでも証明し、意図的に制約を欠くfixtureが拒否されることを確認する。Verus proofは非executableでは登録specを`ensures`へ、executableでは登録kernelの戻り式をnamed returnと`ensures`へ結合し、obligation側の支援claim集合、claim側の参照集合、production-bound evidence必須集合をそれぞれ完全一致させる。Solidityのproof linkはcompiler AST上の完全なcontract・overload signatureと直接call graphへ解決し、Bridge wrapperでは`digest`の生成元、署名回復、`evaluateMint`入力全フィールドと`effects`生成元、再代入の不在、commit引数の宣言IDと順序を結合する。Halmosは署名検証後の`_commitAuthorizedMint`境界についてstate反映、mint量、外部call失敗時のrollbackをsymbolic検査する。SMTとHalmosはともに`supporting`であり、Verusの`derived`義務と同様にclaimの部分証拠として扱い、単独ではproduction実装済みのclaimへ昇格しない。production transaction testはwrapper、認証、event、永続化を含む具体的なend-to-end挙動を担う。
`ui`はABI/Candid drift、typecheck、lint、unit test、build、desktop/mobile Playwrightを実行する。`real`は実Ledger suiteとAnvilを使うPlaywright統合テストを実行し、`all`にも含まれるが短時間用の`checks`には含まれない。
証明範囲と外部仮定は[verification/README.md](verification/README.md)と[verification/obligations.md](verification/obligations.md)に記録する。

ABI snapshotは次で明示的に更新し、通常のCIは更新を行わず差分だけを検出する。

```bash
python3 scripts/abi_snapshot.py --update
python3 scripts/abi_snapshot.py --check
```

Leanの仕様変更後はconformance vectorを明示的に更新し、通常のCIは生成結果との差分だけを検出する。

```bash
python3 scripts/protocol_vectors.py --update
python3 scripts/protocol_vectors.py --check
```

42件のrelease対象claim、抽象・有限幅・trace定理、型付きVerus/SMT/Halmos obligation、明示的なimplementation basis、production link、transaction test、外部仮定は[verification/claims.tsv](verification/claims.tsv)で統一管理し、証拠statusはgateが算出する。vector consumerは[verification/refinement-manifest.tsv](verification/refinement-manifest.tsv)で管理する。不可逆なproduction操作の直前にはproof gateに続いてWasmとcontract runtimeをclean sourceから二回buildし、release manifestとのhash完全一致を要求する。

## ローカルdeploy

`smoke`は次を自動実行する。

1. 新規networkの起動時だけ、port 8000が使用中なら`gateway.port`を一時的に空きportへ変更する。
2. ICP CLI内蔵のローカルPocketIC networkを起動する。
3. `bridge-canister`をdeployし、`Running`と`get_bridge_status`のschema version 35、全count 0を確認する。
4. Anvilをchain ID 31337で起動する。
5. 24時間delay、Canister由来Governance Operator限定のproposer/executor/canceller、自己adminでOpenZeppelin `TimelockController`をdeployする。
6. Timelock addressをBase Adminとして`Bridge`をdeployし、constructorが生成したbSNSのruntime bytecode、相互参照、metadataを確認する。
7. Bridge Signerからsmoke用Depositをmintし、ユーザーの`createWithdrawal`によるatomic burnと`Committed`固定quoteを確認する。Withdrawal用の追加Base transactionと再mint selectorが存在しないことも確認する。
8. Canister由来Governance OperatorのService Fee変更とpause、外部EOAからの直接unpause拒否、24時間前のTimelock execute拒否、経過後のCanister実行によるunpauseを確認する。
9. Withdrawalのburn後の残高・supply、mint window、Withdrawal連番を確認する。
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

本番初回deployまではstable schemaを直接置換し、production向けの旧schema migration、dual-read、fallbackを追加しない。現行v35以外・未知schema・未知wireはfail closedとする。唯一の例外として`test-deployment` staging Wasmだけがreview済みv34／wire v29からv35／wire v29への一方向migrationを持つ。`reinstall`はstate破棄を明示承認した使い捨て環境に限定する。
