---
status: accepted
---

# BaseのService Feeを正本にする

運用中のService FeeはBase contractの状態を正本とし、Bridge canisterはFinalized blockで読み取る。Runtime Administratorはimmutableな`MAX_SERVICE_FEE`以内でだけ変更でき、DepositとWithdrawalの`maxServiceFee`が変更中の利用者を保護するため、Base Adminのtimelock対象にはしない。本決定はADR 0009のRuntime Administratorに関するBase側権限一覧へ、上限内Service Fee変更を追加する。Withdrawalの`amountOut`はBaseでの実行時に固定される。
