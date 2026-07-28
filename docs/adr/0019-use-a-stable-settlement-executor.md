---
status: accepted
---

# Settlement実行をstable SQLite job queueへ集約する

> **一部superseded:** Deposit Mintのraw transaction、confirmation job、専用confirmation APIに関する記述はADR 0023で置換済みであり、以下は採用時の履歴である。stable job queue、lease fencing、Ledger transfer identity、Withdrawal/Governance transaction処理は引き続き有効である。

ADR 0017のscheduler内部設計をsupersedeする。
ADR 0020はEVM confirmation jobの起動方法だけを変更し、ADR 0018のFinalized保証とユーザー実行Withdrawalは維持する。

`settlement_jobs`を自動Settlementの唯一の正本とする。one-shot timerはSQLiteから期限到来jobをclaimするための目覚ましであり、timer IDやheap上のin-flight集合へ永続的な意味を持たせない。init、post-upgrade、job更新後はSQLiteの最短起床時刻からtimerを再登録する。

Canister全体でactive leaseは1件に限定する。claimは120秒のleaseと単調増加するgenerationをSQLite transactionで取得し、RPC、署名、broadcast、Ledger、receipt確認の直前にleaseを更新する。await後のcheckpointは同じgenerationだけを受理する。期限切れleaseを回収した後に古いcallbackが戻っても、外部結果でstable stateを上書きできない。EVMは保存済みraw transaction、Ledgerは保存済みtransfer identityを再利用し、外部の冪等性またはDuplicate応答から回収する。

新規Deposit/Withdrawalはrecordと`phase = settlement`のjobを同じtransactionで作成する。
EVM Submittedはoperationと`phase = confirmation`の確認待ちjobを同じtransactionで保存する。
ADR 0020に従い、このjobは起床時刻を持たない。
確認完了後はjobを`phase = settlement`へ戻す。
停止、完了はfenced outcomeとして保存し、停止理由とstopped jobを同じtransactionへ含める。

timer、新規受付後の自動進行、手動Continue、専用confirmation APIは同じrunnerを使用する。
通常のscheduled/leased jobを手動Continueで迂回できない。
確認待ちjobは専用confirmation APIだけがclaimする。
その他の手動claimはstopped、jobなしの非終端record、5分以上overdueのschedule、expired leaseだけに許可し、quota消費とclaimを同じtransactionで行う。

public scheduler healthはjob件数とdispatcher diagnosticsから導出する。degraded/faulted表示は運用判断の情報であり、個別jobの実行可否を決めるglobal gateにはしない。
