---
status: accepted
---

# 不明なledger transferをReconciliation Holdへ留める

ICRC transferの結果がdeduplication期間後も不明で、履歴から成否を完全に確定できない場合、対象要求をReconciliation Holdへ無期限に留める。時間経過だけを理由にした再送、Deposit返金、Base Refundを禁止する。

完全な不存在証明後、Depositは再利用不能なCancelledへ終端する。Withdrawalだけはattempt番号を増やし、created-at timeとmemo以外の経済的payloadを保存した新しいTransfer Attemptを作成できる。

## Considered Options

- timeout後に新規transferまたは補償を実行する案は、元のtransferが成功済みだった場合に二重pullまたは二重支払を起こすため不採用とする。
- 成否を証明できるまで停止する案は可用性を失う可能性があるが、資産安全を優先して採用する。

## Consequences

- deduplication期間内は、同一`created_at_time`、memo、amount、fee、from、to、spenderでだけ再試行する。
- 期間経過後はICRC-3とindex履歴を使用し、archiveを含む検索範囲の完全性と同期済みwatermarkを確認する。
- memoだけで判定せず、operation、from、to、spender、amount、fee、created_at_timeを照合する。
- 履歴サービスの遅延、欠落、archive障害がある間は「存在しない」と判定しない。
- Governanceは証拠に基づく成否確定を実行できるが、証拠なしに再送・返金を強制できない。
- Verusで、Reconciliation Holdから新規transferまたは補償状態へ直接遷移しないことを証明する。
- `next_block`は最初の未走査block、`ledger_tip`はinclusiveとし、不在には`next_block > ledger_tip`、index watermark到達、archive完全性、exact transfer不一致を要求する。
- 外部履歴を永久に検証できない場合、対象資産が永久停止する残存リスクを受け入れる。
