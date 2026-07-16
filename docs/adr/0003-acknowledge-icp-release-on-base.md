---
status: superseded
---

# ICP ReleaseのBase acknowledgement

ADR 0018により廃止した。Base burnを不可逆な`Committed`状態とし、以後のICP送金をCanisterの債務として扱うため、`acknowledgeRelease`、`cancelRelease`、`refundWithdrawal`はABIから削除する。

結果としてWithdrawalごとの2回目のEVM transaction、threshold ECDSA署名、gas、Finalized確認は不要になる。代わりにburn後のBase refundは提供せず、Canisterは固定されたWithdrawal ID・IC Account・amountOutでLedger送金を再試行・照合する。
