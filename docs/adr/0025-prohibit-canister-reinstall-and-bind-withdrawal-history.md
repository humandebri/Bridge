# ADR 0025: Canister reinstallを禁止しWithdrawal履歴境界を固定する

## 状態

採用

## 文脈

Bridge Canisterのreinstallはstable stateを失う一方、Base Bridge contractのWithdrawal event履歴は残る。同じBase contractへ空のCanisterを接続すると、過去に処理済みのWithdrawal IDを新規通知として再受理できる。deployment instanceによるIC側identity分離だけでは、Base上の既存eventを未処理に戻す問題を防げない。

## 決定

初期化済みの永続Canisterは、同じdeployment instanceを保つreview済みstable schema v35／record wire v30から同じ現行形式へのupgradeだけで更新する。reinstall、instance変更、旧schema、未知schema、未登録wireはdeployment gateとstorage reopenで拒否する。

production未配置のBase Sepolia stagingだけは、`deployments/sepolia-staging/evidence/reinstall-decision-2026-08-27.json`で識別した既存test Canister principalを一度だけ破壊的reinstallする例外とする。この例外は`test-deployment` Wasm、固定Canister ID `rlhjx-iyaaa-aaaaf-qcnyq-cai`、旧Base stackの`abandoned-test-only`記録、fresh Timelock／Bridge／bSNS／signer／deployment instance、空のliability stateをすべて要求する。旧Base contract、旧record、旧deployment instanceを新stackへ接続または移送してはならない。reinstall後の現行schema Canisterには通常規則を適用し、再reinstallを許可しない。

初回install時には、非ゼロ32-byteのinclusive `minimum_withdrawal_id`をimmutable configへ設定する。通常の新規deploymentと、旧Base eventへ到達不能な上記fresh staging stackでは1を使う。test-deploymentの現行schemaにはstaging boundaryを空のliability stateで一度だけ設定する経路を残すが、同じBase contractへ再接続するreinstallの復旧には使用しない。同じ値の再適用以外は拒否する。

Canisterはcanonical Withdrawal eventを確認した後、record作成、Ledger call、liability変更より前に、event IDが境界以上かを256-bit big-endian比較する。境界未満は型付きエラーでfail closedにする。

## 帰結

- reinstallによる履歴消失を通常運用の選択肢から除外する。
- production未配置の旧staging schemaや履歴を失ったstateはmigrationせず、承認済みtest-only例外で既存Canister principalだけを再利用する。Base側identityと全recordは再利用せず、旧stackを`abandoned-test-only`としてactive profileから除外する。
- 一度設定した境界は同一値だけを冪等に受理し、別値への変更を拒否する。
