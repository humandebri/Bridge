---
status: accepted
---

# Withdrawalをブラウザ通知と同期一回検証で取り込む

ブラウザはFinalizedな`WithdrawalCommitted` eventを発見し、認証付き`notify_withdrawal`へtransaction hashを送る。Canisterは同じcanonical Finalized block hashへreceipt、event、`getWithdrawal`、Bridge snapshotを束縛し、固定quoteとIC Accountの完全一致を検証してからLedger送金を開始する。

定期的な全block discoveryは行わない。通知失敗時はHistoryの`Check and notify`から再実行する。通知権限はevent owner、Governance、pause administratorに限定する。Finalized headやcanonical hashが2-of-3で収束しない場合は停止し、Safeへfallbackしない。
