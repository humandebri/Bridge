---
status: accepted
---

# Settlement confirmationをCanisterで自動確認する

ADR 0003とADR 0011の「timerを使わず手動Continueでconfirmationを確認する」判断をsupersedeする。confirmation levelと確認間隔はADR 0018がsupersedeする。

EVM transactionのbroadcastと同じstable SQLite transactionでconfirmation scheduleを保存する。Canisterは最短scheduleだけをone-shot timerへ登録する。Mint、cancel、refund、acknowledgementはSafe headを2、5、10分後に確認する。

自動再試行の対象はreceiptがまだSafe headへ到達していない場合だけとする。RPC不一致・失敗・不正応答、署名、nonce、Ledger障害、Safe-confirmed revertはscheduleを解除し、停止理由を保存する。10分時点の3回目でも未確定なら`ConfirmationCheckExhausted`とし、失敗として表示する。

timerはupgradeで失われるため、`init`と`post_upgrade`でstable scheduleから復元する。手動Retryはschedule中に外部callを行わず、stable window quotaで連打を制限する。Fee payoutは対象外とする。
