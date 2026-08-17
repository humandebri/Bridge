# ADR 0025: Canister reinstallを禁止しWithdrawal履歴境界を固定する

## 状態

採用

## 文脈

Bridge Canisterのreinstallはstable stateを失う一方、Base Bridge contractのWithdrawal event履歴は残る。同じBase contractへ空のCanisterを接続すると、過去に処理済みのWithdrawal IDを新規通知として再受理できる。deployment instanceによるIC側identity分離だけでは、Base上の既存eventを未処理に戻す問題を防げない。

## 決定

初期化済みの永続Canisterは、同じdeployment instanceを保つ現行schema upgradeだけで更新する。reinstall、instance変更、旧schemaからのupgradeはdeployment gateで拒否する。例外として、既存staging canisterに限り、`bridge-storage-v32-to-v33` を一度だけ、wire v28・対象module hash・state invariantが一致する場合に実行する。migration完了後はv32以下を再び拒否する。新しいCanister IDへの初回installは許可する。

初回install時には、非ゼロ32-byteのinclusive `minimum_withdrawal_id`をimmutable configへ設定する。通常の新規deploymentでは1を使う。既にBase履歴が存在するstagingを一度だけmigrationする場合は、Base Withdrawalをpauseし、3 providerのcanonical Finalized checkpointから2-of-3一致した`nextWithdrawalId()`を境界にする。既存Withdrawalはterminalかつ全IDが境界未満でなければならず、pending Ledger operation、hold、未払liabilityもゼロでなければならない。境界はupgrade argsの明示値だけを受け付け、同じ値の再適用以外は拒否する。

Canisterはcanonical Withdrawal eventを確認した後、record作成、Ledger call、liability変更より前に、event IDが境界以上かを256-bit big-endian比較する。境界未満は型付きエラーでfail closedにする。

## 帰結

- reinstallによる履歴消失を通常運用の選択肢から除外する。
- Base contractを変更しないstaging復旧でも、過去IDの再払出しを防げる。
- 境界captureのcanonicalityと、pause時の`nextWithdrawalId()`が最初の未発行IDであることは外部前提として証拠台帳へ記録する。
- 境界値を誤ると正当なWithdrawalを拒否し得るため、手入力や単一RPC観測を認めない。
