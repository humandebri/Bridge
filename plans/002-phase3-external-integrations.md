# Plan 002: Phase 3 外部連携とローカル E2E

> **履歴資料**：この本文はPlan 002完了時点のschema v2と実装境界を記録している。
> 現行仕様はリポジトリ直下の`README.md`と`docs/`を参照する。

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001
- **State**: DONE

## Implemented boundary

- 公開`request_deposit`はcallerとclient request IDからDeposit IDを決定し、finalized Base fee/limit確認後、同一identityのICRC-2 pullを実行する。
- ICRCの成功と`Duplicate`は同じ成功証拠として扱い、call rejection・decode不能はReconciliation Holdへ移す。dedup期間後はLedgerと動的archiveの全rangeを照合し、完全被覆できない限りabsentにしない。
- Base監視は`WithdrawalCreated`を発見にだけ使用し、`finalized`の`getWithdrawal`を受付根拠にする。3 provider中2の一致を必須とする。
- EVM操作は単一stable nonce queue、固定contract、固定selectorのEIP-1559 envelopeとして保存し、threshold ECDSA署名後のraw transactionを再送用に保持する。
- timerはWithdrawal発見、Hold照合、ICP Release、mint/acknowledgement/refund送信、finalized receiptとcontract state確認をstable recordから再開する。
- 本番未デプロイのためlegacy migrationは持たず、schema v2以外をfail closedで拒否する。現行schemaの未完了record、nonce、cursor、会計はupgrade後も保持する。

## Deferred to Plan 003

- Settlement Reserveの実コスト、task優先queue、fee bump、Runtime Administrator、手動Governance resolution、運用監査ログ。
- mainnetのBase address、ECDSA key、gas上限と監視値の確定。

## Verification

- Rust format、clippy、workspace test、Wasm build、Candid drift、ICP buildをCI gateで検査する。
- Base ABIは変更せず、selector/topic snapshotと既存Foundry/SMT/Verus gateを維持する。
- PicJSでmock Ledger、mock EVM RPC、management threshold ECDSAを同一PocketIC topologyへ導入し、Deposit、Withdrawal release/acknowledgement、Base Refund、Reconciliation Holdのupgrade保持、stuck receiptを検証した。
- E2Eは空きportを自動割当し、同一operationの再broadcastが同一raw transactionになることを検証する。`scripts/ci-local.sh checks`でmock/bridge Wasm buildとE2Eを必須gate化した。
