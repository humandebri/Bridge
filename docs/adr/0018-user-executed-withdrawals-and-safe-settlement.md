---
status: accepted
---

# Withdrawalをユーザー実行txとsafe確定で開始する

ADR 0003、ADR 0011のWithdrawal開始方法と、ADR 0017の旧確認保証をsupersedeする。

Base→ICPは、ユーザーがBridgeへ要求額ちょうどのERC-20 allowanceを設定し、`createWithdrawal`を送信する。このtransactionは`transferFrom`、Bridge残高のburn、Withdrawal作成、`Releasing`化を原子的に実行する。CanisterがRelease開始用のBase transactionを追加送信する設計は採用しない。

Canisterは`createWithdrawal`のcanonical safe receipt、block hash、event、`Releasing`状態、ICP owner、Bridge signerをquorumで検証する。検証後は同じ`notify_withdrawal` callでLedger送金を開始する。最低受取額を満たせない場合はLedgerを呼ばず、`cancelRelease`をsafe確認してからrefundをsafe確認する。

Mint、cancel、refund、acknowledgementはすべてSafeを決済条件とし、送信後2、5、10、20、40分に自動確認する。

SafeはL1 settlement完了よりreorg耐性が弱いが、Base sequencerがSafeとした時点で利用者完了へ進める遅延短縮を選ぶ。UIはSafe headまでeventを走査し、保存済みSafe block hashが変わればキャッシュを破棄してdeployment blockから再走査する。通知失敗時はHistoryの`Check and notify`を回復経路とする。
