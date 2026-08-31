# ADR 0025: Canister reinstallを禁止しWithdrawal履歴境界を固定する

## 状態

採用

## 文脈

Bridge Canisterのreinstallはstable stateを失う一方、Base Bridge contractのWithdrawal event履歴は残る。同じBase contractへ空のCanisterを接続すると、過去に処理済みのWithdrawal IDを新規通知として再受理できる。deployment instanceによるIC側identity分離だけでは、Base上の既存eventを未処理に戻す問題を防げない。

## 決定

初期化済みの永続Canisterは、同じdeployment instanceを保つreview済みstable schema v35／record wire v30から同じ現行形式へのupgradeだけで更新する。reinstall、instance変更、旧schema、未知schema、未登録wireはdeployment gateとstorage reopenで拒否する。

現在のBase Sepolia stagingは、2026-08-27/28に明示承認され完了した一度限りのdestructive reinstallとfresh-stack作成を履歴証跡として固定する。そのCanister ID、deployment instance、minimum Withdrawal ID、Base contracts、signerをactive stackとして維持し、この履歴を再実行、resume、別stackの認可に使わない。staging evidence schema v8の`bootstrap_attestation`は履歴artifactのhashとactive bindingの一致だけを検証する。今後の更新にも上記same-instance current-schema upgrade規則を適用する。

初回install時には、非ゼロ32-byteのinclusive `minimum_withdrawal_id`をimmutable configへ設定する。通常の新規deploymentでは1を使う。test-deploymentの現行schemaにはstaging boundaryを空のliability stateで一度だけ設定する経路を残すが、旧schema migrationや履歴を失ったreinstallの復旧には使用しない。同じ値の再適用以外は拒否する。

Canisterはcanonical Withdrawal eventを確認した後、record作成、Ledger call、liability変更より前に、event IDが境界以上かを256-bit big-endian比較する。境界未満は型付きエラーでfail closedにする。

## 帰結

- reinstallによる履歴消失を通常運用の選択肢から除外する。
- production未配置であっても、active staging stackの履歴を再び失うreinstallや、別instanceへの置換を通常運用へ戻さない。
- 一度限りの過去reinstallはimmutableな監査証跡としてのみ保持し、v7 evidenceのresumeやv8へのmigrationを行わない。
- 一度設定した境界は同一値だけを冪等に受理し、別値への変更を拒否する。
