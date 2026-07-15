---
status: accepted
---

# ICP ReleaseをBase contractへ記録する

> Withdrawal開始方法と確定条件はADR 0018でsupersedeされた。現行実装と運用判断にはADR 0018だけを使用する。

> Confirmation後の自動進行に関する判断はADR 0017でsupersedeされた。

Withdrawalの開始・確認方法に関する旧方式は廃止した。現在はユーザーの`createWithdrawal`がburnと`Releasing`化を同一transactionで行い、Canisterはcanonical Safe blockで確認する。Release成功後のacknowledgement、確定的未送金時だけ許可するcancel、`Pending`だけに許可するrefundという排他条件は維持する。

## Considered Options

- Bridge canisterのstable stateだけで排他性を管理する案は、Base contractがRelease済みを判定できず、将来の誤操作や再試行による二重支払を防げないため不採用とする。
- ICP Releaseの成功後にBase transactionを追加する案はgasと遅延が増えるが、contract状態としてrefund不能を確定できるため採用する。

## Consequences

- Bridge canisterはユーザーの`createWithdrawal`を同一canonical Safe blockへ束縛して確認するまでICRC transferを呼ばない。ICRC transferの成功、`Duplicate`、または完全な履歴照合を確定してからRelease acknowledgementを送る。
- releaseとacknowledgementは明示的な`notify_withdrawal`または障害復旧用`continue_withdrawal`から開始し、EVM transactionを送信した後のconfirmation確認と次の正常段階はADR 0017のone-shot timerで進める。
- Release acknowledgementはwithdrawal IDで冪等にし、同一内容の再実行を成功扱いにする。
- 同じICP ledger block indexを別WithdrawalのRelease acknowledgementへ再利用することをBase contractでも拒否する。
- ICP Release開始後からBaseで`Released`がSafe確認されるまで、自動refundへ遷移させない。
- Walletを閉じてもschedule済みのconfirmation確認は継続する。自動処理が停止理由を保存した場合だけ、利用者のHistory操作またはGovernance/pause administratorのRetryで再開する。
- Withdrawal settlement用のBase gasを新規Deposit処理とは別に確保する。
- Verusで、1件のWithdrawalが`Released`と`Refunded`の両方へ到達しないことを証明する。
- Base acknowledgementはICP Releaseの暗号学的proofではなく、信頼されたBridge signerによる記録である。悪意あるsignerに対するtrustless保証にはならない。
