# Plan 001: Phase 2の決定的Bridge状態機械を実装する

> **履歴資料**：この本文はPlan 001の実装時点における前提と完了条件を記録している。現行実装はCanister timerを使わない明示操作型Settlementである。
> 現行仕様はリポジトリ直下の`README.md`と`docs/`を参照する。

> **実行者向け指示**: この計画を上から順に実行し、各ステップの検証結果を確認してから次へ進むこと。`STOP条件`に該当した場合は実装を続けず、差分と判断材料を報告すること。完了時は`plans/README.md`の状態を更新する。

> **ドリフト確認（最初に実行）**: `git diff --stat 5fc223c..HEAD -- Cargo.toml canister/bridge-core canister/bridge-canister scripts/ci-local.sh docs/adr/0008-handover-bridge-upgrades-to-sns-control.md docs/implementation-plan.md`。対象ファイルにPhase 2の意図と異なる変更がある場合、下記Current stateを現行コードと照合し、不一致ならSTOPする。

## Status

- **Priority**: P1
- **Effort**: L（複数日。core、stable schema、Candid境界、回帰テストを含む）
- **Risk**: HIGH（asset-moving処理の前提となる永続状態と公開Candid境界を初めて導入する）
- **Depends on**: なし（Base contractのABI凍結済みが前提）
- **Category**: tech-debt / tests / direction
- **Planned at**: commit `5fc223c`, 2026-07-13

## なぜ必要か

Base側のbSNS、Deposit、Withdrawal、pause、Timelock、ABI snapshotはPhase 1Eで検証済みだが、ICP側のBridge canisterはまだ空のCandid serviceで、pure coreにも業務ロジックがない。現在のままではDepositのescrow、WithdrawalのRelease/Refund、EVM transaction、Reconciliation Hold、upgrade後の再開を表現できず、Base contractを安全に呼び出す主体が存在しない。

この計画では、外部ledger・EVM・threshold ECDSAを呼ばない決定的coreと、IC stable memoryへ直接保存するcanister stateを先に作る。外部I/Oを後続Plan 002へ分離することで、リトライ、冪等性、rollback、terminal stateを単体テストとVerusの対象にできる。

## Current state

- `canister/bridge-core/src/lib.rs:1-4` — dependency-free Rust crateだが、Phase 0の説明だけで、型・状態・遷移・テストがない。
- `canister/bridge-canister/src/lib.rs:1-6` — `ic_cdk::export_candid!()`だけを公開し、asset-movingまたは管理update methodが存在しない。
- `canister/bridge-canister/bridge.did:1` — Candid serviceは`service : () -> {};`で空。
- `Cargo.toml:1-17` — workspaceは`bridge-core`と`bridge-canister`、Rust 1.97.0、`candid 0.10.32`、`ic-cdk 0.20.2`を固定している。`ic-stable-structures`はまだ依存していない。
- `scripts/ci-local.sh:53-63` — Rust gateはfmt、clippy、workspace test、Wasm build、local-network preparationを実行する。core testとschema testはこのgateに載せる。
- `scripts/ci-local.sh:138-141` — ICP build gateは`icp project show`と`icp build bridge-canister`だけで、Candidの業務APIやupgrade互換性は未検査である。
- `docs/adr/0008-handover-bridge-upgrades-to-sns-control.md` — stable structuresへ直接保存し、全stateを`pre_upgrade`でserializeしないこと、未完了Deposit/Withdrawal/EVM transaction/Reconciliation Holdをupgrade後に再開できることを要求している。
- `docs/implementation-plan.md:110-127` — Phase 2はstate設計、Settlement Reserveを侵食しないDeposit受付、Service Fee保護、Deposit flowを定義する。Phase 3の外部連携より先にpure logicを作る方針である。
- `docs/parameters.md:16-57` — Mint Throughput Limit、Per-Deposit Limit、`MAX_SERVICE_FEE`、Settlement Reserveの値はTBDである。Plan 001では値を埋めず、raw unitとchecked arithmeticの契約だけを持つ。

### 守るべき設計語彙と制約

- `docs/glossary.md`のDeposit、Withdrawal、Bridge Exposure、Service Fee、Settlement Reserve、Reconciliation Holdをそのまま状態名・コメント・テスト名に使う。`Withdrawal Settlement`はBaseの`Pending → Released`または`Pending → Refunded`の一方だけで終端する。
- ADR 0001/0004/0005/0006/0008の決定を変更しない。特に、refundは新規Deposit mint throughputを消費せず、Service Feeは成功確定時だけ確定し、不明なledger transferは時間経過だけで再送・返金しない。
- Base contractのABIは`docs/base-interface.md`と`contracts/abi/*.json`が正本であり、Plan 001ではSolidity ABIを変更しない。
- 外部I/Oはcoreから呼ばない。ICRC ledger、EVM RPC、threshold ECDSA、timer、management canister、HTTPを導入するのはPlan 002以後とする。

## Commands you will need

| 目的 | コマンド | 成功条件 |
|---|---|---|
| Drift check | `git diff --stat 5fc223c..HEAD -- Cargo.toml canister/bridge-core canister/bridge-canister scripts/ci-local.sh docs/adr/0008-handover-bridge-upgrades-to-sns-control.md docs/implementation-plan.md` | Phase 2の未計画差分がない |
| Rust format | `cargo fmt --manifest-path Cargo.toml --all --check` | exit 0 |
| Rust lint | `cargo clippy --manifest-path Cargo.toml --workspace --all-targets -- -D warnings` | exit 0、warningなし |
| Unit/property tests | `cargo test --manifest-path Cargo.toml --workspace` | coreとcanisterの全testがpass |
| Wasm build | `cargo build --manifest-path Cargo.toml --target wasm32-unknown-unknown --release -p bridge-canister` | exit 0 |
| ICP build | `scripts/ci-local.sh icp` | Candid生成を含むcanister buildがpass |
| Full regression | `scripts/ci-local.sh checks` | Rust、contract、SMT、Verus、ICP buildがpass |

## Scope

**In scope（変更してよいファイル）**:

- `Cargo.toml`、`Cargo.lock` — stable structures等の必要な依存を固定する。
- `canister/bridge-core/src/lib.rs`、`canister/bridge-core/tests/*.rs` — pure domain types、遷移、会計、不変条件、unit/property tests。
- `canister/bridge-canister/src/lib.rs`、`canister/bridge-canister/bridge.did` — stable memory adapterとasset-movingを行わないread-only Candid境界。
- `canister/bridge-canister/tests/*.rs` — stable schemaの再オープン・upgrade相当テスト。実際のPocketIC upgrade testを追加する場合もこのディレクトリに限定する。
- `docs/`のPhase 2状態遷移・stable schema文書、および`verification/README.md`の証明境界追記。
- Plan 001に必要なCI test invocationの最小変更（既存の`contracts` gateやBase ABIを変更しない）。

**Out of scope（触らない）**:

- `contracts/src/**`、`contracts/test/**`、`contracts/abi/**` — Phase 1Eで凍結済みのBase contractとABI。
- ICRC ledger transfer、EVM RPC送信、threshold ECDSA、nonce queue、Settlement Reserveの実コスト計算、Runtime Administrator、Fee Recipient運用 — Plan 002/003へ延期する。
- `docs/parameters.md`のTBD数値、Base Admin wallet、SNS Root handover、mainnet/testnet deploy — Plan 005/006へ延期する。KINIC LedgerとIndexの本番識別子は確定済みである。x402 facilitatorはBridgeの配置・activation範囲外とする。
- `pre_upgrade`で全stateを一括serializeする実装。stable structuresのmemory layoutを正本にする。

## Steps

### Step 1: 状態遷移表と数値境界を先に固定する

`docs/`にPhase 2の状態遷移表を追加し、Deposit、Withdrawal、EVM transaction、Reconciliation Holdの各状態、許可遷移、入力、成功後のstorage変更、失敗時の不変条件、idempotent retry、conflicting retryを明記する。Baseの`WithdrawalStatus`はBase側のrecordと一致させ、ICP側の実行状態は別enumにして混同しない。

Depositでは`grossAmount`、利用者の`maxServiceFee`、実行時Service Fee、net mint量、Settlement Reserve予約の関係を定義する。WithdrawalではBase burn量、`minAmountOut`、ledger fee、Service Fee、Release/Refund結果、同一要求の再試行を定義する。`u128`へ縮小するか、Candid `Nat`を保持するかは、対象SNS ledgerの最大値を文書化してから決める。根拠なしに`as u128`やunchecked castを置かない。

**Verify**: `rg -n "Pending|Released|Refunded|Reconciliation Hold|Service Fee|Settlement Reserve" docs/` → 4つの状態機械と数値境界の記述が見つかり、各状態に許可遷移と拒否遷移がある。

### Step 2: dependency-freeなpure coreを実装する

`bridge-core`に、checked amount arithmetic、request identity、Deposit/Withdrawal/EVM/Reconciliationの状態型、`CoreError`、決定的なtransition関数を追加する。transitionは新しいstateまたは明示的なerrorを返し、error時に入力stateを変更しない。外部呼び出しは`Command`または副作用のないdecisionとして返し、core自身はledger・EVM・IC runtimeへアクセスしない。

最低限、次を実装する。

- Deposit受付前に、Service Fee上限、`maxServiceFee`、net mint量、Per-Deposit/throughputの入力を検査し、Settlement Reserve予約が不足する場合は受付を拒否するdecision。
- Depositのpull、Base mint送信、成功確定、失敗、Reconciliation Hold、refund可能状態をID付きで冪等に管理するdecision。
- WithdrawalのBase observed、Release送信、Release確定、Base Refund、terminal状態を排他的に管理するdecision。
- Service Feeは成功確定までfee reserveへ計上せず、Base Refundとcancelでは計上しない。
- 不明なledger結果はReconciliation Holdに固定し、同じtransfer identity以外の再送と証拠なし補償を拒否する。

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-core` → pure core testがpassし、`cargo clippy --manifest-path Cargo.toml --workspace --all-targets -- -D warnings` → warningなし。

### Step 3: coreの不変条件・冪等性・境界テストを追加する

既存のSolidity invariantの考え方をRust coreにも適用し、入力stateとcommand列を生成するtest helperを作る。少なくとも次をテストする。

- Deposit成功のnet amountとfee reserveの保存、失敗時のstate不変、同一IDの再実行、異なるpayloadのconflict拒否。
- Withdrawalの`Pending → Released`と`Pending → Refunded`の排他、terminal retryの同一内容成功、異なる内容拒否、Release後のRefund拒否。
- `totalSupply + Pending + Released`相当のBridge Exposure保存、refundがDeposit throughputを消費しないこと、Service Fee変更が既存pending settlementを書き換えないこと。
- `ReconciliationHold`から新しいtransfer、refund、Base補償へ直接遷移しないこと。
- 0、最大値、feeがamountを超える場合、ID重複、空payload、unknown ID、算術overflow/underflow。

property test dependencyを追加する場合は、Rust 1.97.0で維持でき、テスト専用であることを確認する。追加不要なら決定的な複数case table testを優先し、検証対象を曖昧にしない。

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-core` →上記ケースを含む全testがpass。`cargo test --manifest-path Cargo.toml --workspace` →他crateを含め全pass。

### Step 4: stable structures adapterとschema versionを実装する

`bridge-canister`はstable SQLiteへcore stateを直接保存し、`pre_upgrade`で全stateをblob化しない。各recordのkey、stable value encoding、schema version、migration方針を文書化する。

adapterはcoreのtransitionを呼び出し、成功したdecisionだけをstable mapへ反映する。外部I/Oが未実装のPhase 2では、asset-moving update endpointを公開しない。読み取りqueryは、state version、pause/acceptance state、未完了件数、Reconciliation Hold件数など、秘密や署名materialを含まない最小情報に限定する。

同一のテストmemoryを閉じて再オープンし、schema versionを確認して、Deposit、Withdrawal、EVM transaction、Reconciliation Holdの未完了recordが同じ状態で読めることをテストする。旧schemaを読む必要がある場合は明示的なmigration関数とfixtureを追加し、暗黙のdefaultで欠損資産を作らない。

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-canister` → stable mapの書込み・再オープン・schema検査がpass。`cargo build --manifest-path Cargo.toml --target wasm32-unknown-unknown --release -p bridge-canister` → exit 0。

### Step 5: read-only Candid境界と回帰gateを固定する

`bridge.did`と`ic_cdk::export_candid!()`の生成結果を一致させる。Phase 2のCandidはread-only queryだけにし、callerが任意のDeposit/Withdrawal遷移を発動できるupdate methodを追加しない。queryのrecord/variant名はStep 1の状態語彙と一致させ、将来のasset-moving API用に予約した名前を安易に公開しない。

既存の`ci-local.sh`のRust/ICP gateへ必要最小限のCandid生成・schema testを接続する。Base contract、ABI snapshot、SMT negative fixture、Verus fixtureの判定を変更しない。

**Verify**: `scripts/ci-local.sh rust` → fmt、clippy、workspace test、Wasm build、local-network preparationがpass。`scripts/ci-local.sh icp` → ICP project show/buildがpass。`scripts/ci-local.sh checks` →全既存gateがpass。

## Test plan

- `canister/bridge-core/tests/`で状態遷移表の全許可・拒否遷移、terminal idempotency、fee・reserve・exposure算術をテーブルテストする。
- `canister/bridge-canister/tests/`でstable memoryの再オープン、schema version、旧fixtureからのmigration（採用した場合）、queryが副作用を持たないことを検証する。
- Rust testは本物のICRC ledgerやEVM RPCへ接続しない。外部adapterを追加するテストはPlan 002で行う。
- 構造上のパターンは`verification/smt/pass/WithdrawalState.sol`、`contracts/test/BridgeWithdrawal.t.sol`、`contracts/test/BridgeInvariant.t.sol`の不変条件・terminal state・idempotencyの考え方を参照する。ただしSolidity ABIや型を直接コピーしない。

## Done criteria

- [ ] `docs/`にDeposit、Withdrawal、EVM transaction、Reconciliation HoldのPhase 2状態遷移表がある。
- [ ] `bridge-core`が外部I/Oなしでchecked transition、error、idempotency、fee/exposure/reserve invariantsを実装している。
- [ ] `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`がexit 0。
- [ ] stable stateが`ic-stable-structures`へ直接保存され、全stateを`pre_upgrade`でserializeする実装がない。
- [ ] stable schemaの再オープンテストが、未完了Deposit、Withdrawal、EVM transaction、Reconciliation Holdを保持する。
- [ ] Phase 2のCandidはread-only queryだけで、任意callerがasset-moving遷移を起こせない。
- [ ] `cargo build --target wasm32-unknown-unknown --release -p bridge-canister`と`scripts/ci-local.sh checks`がpassする。
- [ ] `git status --short`がPlan 001のScope外ファイルを変更していない。
- [ ] `plans/README.md`の001行が更新されている。

## STOP conditions

- 対象SNS ledgerのamount上限が決まらず、`u128`とCandid `Nat`の選択を安全に根拠付けられない。
- core transitionが外部I/O、timer、caller、乱数、現在時刻に依存する必要が出た。
- stable schemaを変更しないと未完了stateを再オープンできない、または旧fixtureの意味を復元できない。
- Phase 2のread-only境界にasset-moving update methodを追加しないとテストできない。
- 既存のBase ABI、`contracts/`、`contracts/abi/`を変更する必要が出た。
- `cargo clippy`、workspace test、Wasm build、ICP buildのいずれかが2回の合理的な修正後も失敗する。
- 依存追加でRust 1.97.0、pinned lockfile、wasm32 buildが維持できない。

## Maintenance notes

- Plan 002のICRC/EVM adapterは、ここで固定したcore transitionとstable key/schemaを呼び出すだけにし、外部失敗を新しい状態へ変換する。core APIをadapter都合で緩めない。
- Plan 003のRuntime Administratorは、Phase 2のqueryで未完了件数、reserve、Reconciliation Holdを安全に観測できることを前提にする。
- Plan 004のVerusでは、pure coreのtransitionと不変条件をproductionと同じ関数から証明対象にする。`Nat`/`u128`境界を別の未検証変換として増やさない。
- Reviewでは、caller認証がquery/updateの境界にないこと、retryがconflicting payloadを受理しないこと、stable schemaの変更がupgradeを壊さないことを重点確認する。
- `docs/parameters.md`のTBD値はこの計画では埋めない。対象SNSと運用監視が確定したPlan 005で、導出式と外部仮定を一緒に更新する。
