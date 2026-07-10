---
status: accepted
---

# ICP ReleaseをBase contractへ記録する

ICP Releaseが成功したWithdrawalはBase contractへacknowledgeし、Base上の状態を`Released`へ終端させる。Base Refundは`Pending`のWithdrawalだけに許可し、`Pending → Released`と`Pending → Refunded`を排他的にする。

## Considered Options

- Bridge canisterのstable stateだけで排他性を管理する案は、Base contractがRelease済みを判定できず、将来の誤操作や再試行による二重支払を防げないため不採用とする。
- ICP Releaseの成功後にBase transactionを追加する案はgasと遅延が増えるが、contract状態としてrefund不能を確定できるため採用する。

## Consequences

- Bridge canisterはICRC transferの成功、`Duplicate`、または完全な履歴照合を確定してからRelease acknowledgementを送る。
- Release acknowledgementはwithdrawal IDで冪等にし、同一内容の再実行を成功扱いにする。
- ICP Release開始後からBaseで`Released`がfinalizeするまで、自動refundへ遷移させない。
- Withdrawal settlement用のBase gasを新規Deposit処理とは別に確保する。
- Verusで、1件のWithdrawalが`Released`と`Refunded`の両方へ到達しないことを証明する。
- Base acknowledgementはICP Releaseの暗号学的proofではなく、信頼されたBridge signerによる記録である。悪意あるsignerに対するtrustless保証にはならない。
