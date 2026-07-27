---
status: accepted
---

# Withdrawalを不可逆なCommitted burnとして扱う

ユーザーは`createWithdrawal(amount, maxServiceFee, owner, subaccount)`を送信する。Contractはburn前に実行時Service Feeが上限以下かつ`amount > serviceFee`であることを検証し、`transferFrom`、burn、固定`amountOut = amount - chargedServiceFee`を持つ`Committed`化を原子的に行う。

`Committed`はBase上の終端状態であり、Base refund、release acknowledgement、cancelは提供しない。Canisterはcanonical Finalized receipt、event、state、snapshotを同一block hashで検証し、固定額を固定IC Accountへ送る。Ledger FeeはBridge負担とする。Ledger Feeがcharged Service Feeを超えた場合はreleaseを作らず、Observed record、runtime guard、監査eventを保存する。運用者はBase withdrawalをpauseしてfeeを同期し、`continue_withdrawal`の再検証に成功した場合だけ同じrecordを再開する。WithdrawalとDeposit refundの`BadFee`ではamount、fee、transfer identityを変更しない。

## 結果

- 正常WithdrawalのBase transaction、Finalized確認、ユーザー意思確認は1回だけとなる。
- Withdrawalごとのthreshold ECDSA署名、2回目のgas、nonce、confirmation job、EVM recoveryを削除する。
- Ledger障害時は同じWithdrawal ID・IC Account・transfer identityを使う再試行と履歴照合で解消する。管理者による送金先変更や任意送金は認めない。
- burn後にBaseへ資産を戻せないため、UIは不可逆性を署名前に表示し、fee、残高、wallet、chainを直前に再検証する。
- Finalized headとcanonical hashが2-of-3で収束しない場合は停止し、Safeや固定confirmation数へfallbackしない。
