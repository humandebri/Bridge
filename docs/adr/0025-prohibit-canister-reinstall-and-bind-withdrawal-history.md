# ADR 0025: Canister reinstallを禁止しWithdrawal履歴境界を固定する

## 状態

採用

## 文脈

Bridge Canisterのreinstallはstable stateを失う一方、Base Bridge contractのWithdrawal event履歴は残る。同じBase contractへ空のCanisterを接続すると、過去に処理済みのWithdrawal IDを新規通知として再受理できる。deployment instanceによるIC側identity分離だけでは、Base上の既存eventを未処理に戻す問題を防げない。

## 決定

初期化済みの永続Canisterは、同じdeployment instanceを保つreview済みstable schema v34／record wire v29から同じ現行形式へのupgradeだけで更新する。唯一の例外として、既にdeploy済みのstaging v33／wire v28は、固定ID `bridge-staging-v33-to-v34` とreview済みimmutable設定をupgrade引数へ束縛してv34へ一度だけ移行できる。reinstall、instance変更、その他の旧schema、未知schema、未登録wireはdeployment gateとstorage reopenで拒否する。新しいCanister IDへの初回installは許可する。

初回install時には、非ゼロ32-byteのinclusive `minimum_withdrawal_id`をimmutable configへ設定する。通常の新規deploymentでは1を使う。test-deploymentの現行schemaにはstaging boundaryを空のliability stateで一度だけ設定する経路を残すが、旧schema migrationや履歴を失ったreinstallの復旧には使用しない。同じ値の再適用以外は拒否する。

Canisterはcanonical Withdrawal eventを確認した後、record作成、Ledger call、liability変更より前に、event IDが境界以上かを256-bit big-endian比較する。境界未満は型付きエラーでfail closedにする。

## 帰結

- reinstallによる履歴消失を通常運用の選択肢から除外する。
- v33 staging migration以外の旧schemaや履歴を失ったstateは復旧対象にせず、現行schemaで検証stateを作り直す。
- 一度設定した境界は同一値だけを冪等に受理し、別値への変更を拒否する。
