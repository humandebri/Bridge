---
status: accepted
---

# Withdrawalを不可逆なCommitted burnとして扱う

ユーザーは`createWithdrawal(amount, maxServiceFee, owner, subaccount)`を送信する。Contractはburn前に実行時Service Feeが上限以下かつ`amount > serviceFee`であることを検証し、`transferFrom`、burn、固定`amountOut = amount - chargedServiceFee`を持つ`Committed`化を原子的に行う。

`Committed`はBase上の終端状態であり、Base refund、release acknowledgement、cancelは提供しない。Canisterはcanonical Finalized receipt、event、state、snapshotを同一block hashで検証し、固定額、固定IC Account、transfer identityを債務として保存する。Ledger送金と照合は任意の非anonymous Principalによる`continue_withdrawal`ごとに最大1 external stepだけ進み、timerでは再試行しない。Ledger FeeはBridge負担とし、100,000 rawで不変であることを外部仮定とする。固定Ledger Feeがcharged Service Feeを超えた場合はreleaseを作らず、Observed record、固定fee guard、監査eventを保存する。運用者はBase withdrawalをpauseして設定を確認し、`continue_withdrawal`の再検証に成功した場合だけ同じrecordを再開する。runtime settlementは`icrc1_fee()`を照会しない。WithdrawalとDeposit refundの`BadFee`ではamount、fee、transfer identityを変更しない。

## 結果

- 正常WithdrawalのBase transaction、Finalized確認、ユーザー意思確認は1回だけとなる。
- Withdrawalごとのthreshold ECDSA署名、2回目のgas、nonce、confirmation job、EVM recoveryを削除する。
- Ledger障害時は同じWithdrawal ID・IC Account・transfer identityを使う再試行と履歴照合で解消する。管理者による送金先変更や任意送金は認めない。
- Withdrawal reviewはamount、fee、IC recipientを表示し、fee、残高、wallet、chainを署名前に再検証する。deployment単位の初回利用時にはunaudited bridgeに関する一般risk acknowledgementを要求し、最終的な`createWithdrawal`はwallet署名で承認する。withdrawal固有のburn・Base refund不在の警告またはcheckboxは要求しない。この簡略化は、Base refund、release acknowledgement、cancelを追加するものではなく、不可逆な`Committed`設計を変更しない製品判断として受容する。
- Finalized headとcanonical hashが2-of-3で収束しない場合は停止し、Safeや固定confirmation数へfallbackしない。
