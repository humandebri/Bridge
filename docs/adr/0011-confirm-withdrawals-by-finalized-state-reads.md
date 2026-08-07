---
status: accepted
---

# Withdrawalをブラウザ通知と同期一回検証で取り込む

ブラウザはFinalizedな`WithdrawalCommitted` eventを発見し、deployment単位で永続化した通知専用Identityから`notify_withdrawal`へtransaction hashを送る。Canisterは同じcanonical Finalized block hashへreceipt、event、`getWithdrawal`、Bridge snapshotを束縛し、固定quoteとIC Accountの完全一致を検証してからLedger送金を開始する。

定期的な全block discoveryは行わない。retry可能な通知失敗は保存済みtransaction hashから再実行し、Historyの`Check status`でも明示再実行できる。通知は任意の非anonymous Principalが実行でき、callerは送金先やamountを変更できない。Finalized headやcanonical hashが2-of-3で収束しない場合は停止し、Safeへfallbackしない。
