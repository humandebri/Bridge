# KINIC–Base Bridge セキュリティ修正・検証レポート

更新日：2026-07-22
対象：staged、unstaged、untrackedを含む現在の作業ツリー
前提：本番未デプロイのため、旧ABI、旧Candid、旧stable schemaとの互換処理は持たない

## 結論

採択済みの対応計画について、Contract、Canister、UI、release/evidence、local E2Eの修正を実装した。資産移動、管理権限、外部観測、stable storage、ブラウザ再送、デプロイ証跡の各境界は、曖昧または不整合な状態を成功として扱わない。

本番deployとactivationは未完了である。Gate AはBase receipt/logとIC certificate/auditの真正性検証がなく、Gate BはSNS proposalの実行完了照合とx402真正性検証がないため、どちらも必ず非ゼロ終了する。したがって、現状のスクリプトから誤ってproduction deploy、unpause、activation成功を報告することはない。

なお、当初要求の「P2以上100件」は件数を満たすための水増しを行わない。重大度は具体的な攻撃経路と影響で判定すべきであり、このレポートは再現・修正・回帰検証できた境界だけを完了扱いとする。

## 実装した主要なセキュリティ境界

| 領域 | 修正後の境界 |
|---|---|
| Withdrawal Contract | 同一transaction内の複数withdrawalを拒否し、amountをu128範囲へ制限し、sink recipientを拒否する |
| Timelock | delayを72時間以上30日以下へ制限し、各roleを単一memberへ固定し、open roleと外部adminを拒否し、pending operationを追跡する |
| Canister設定 | zero/重複contract、権限主体の重複、不正RPC URL、上限不整合をinstall時に拒否する |
| 外部観測 | Finalized block hashへEVM call/code/receiptを束縛し、RPC不一致・欠落・nonce競合・曖昧broadcastをfail closedにする |
| Ledger settlement | 固定tipを越えるpageを切り詰め、live feeが確定quoteと異なる場合は送金前に停止する |
| Scheduler | leaseを5分へ制限し、別recordの期限超過jobをhead-of-line blockingせず、retry/backoffとprior hash追跡を行う |
| Stable storage | stable schemaはv17だけを受理し、段階検証対象の全tableでdecode、主キー、参照、index整合性を検査する |
| UI | wallet account/chainをwrite直前に再検証し、Web Locksで重複操作を防ぎ、pending intentをversion付きで永続化する |
| Release | source、submodule、Wasm、runtime bytecode、deployment receipt、live snapshot、controller handoverをhashで連結する |
| Local E2E | Anvil/PocketICのport・PID・network stateを所有権付きで管理し、upgrade証跡をschema v2へ固定する |

## 検証結果

| 対象 | 結果 | コマンド・件数 |
|---|---|---|
| Rust workspace | PASS | `cargo test --workspace`、172 tests |
| Stable storage破損回帰 | PASS | 従来未検査だった8 tableのmalformed rowをすべて拒否 |
| Solidity | PASS | `forge test`、75 tests（fuzz/invariantを含む） |
| Formal proofs | PASS | Lean、Solidity SMT、Verus 55 obligationsと全negative fixture |
| UI unit | PASS | Vitest 135 tests |
| UI static/build | PASS | typecheck、ESLint、Vite build |
| UI browser | PASS | Playwright desktop/mobile 8 tests |
| ABI/Candid生成 | PASS | `codegen:abi:check`、`codegen:candid:check` |
| Release/local safety | PASS | production driver/release、CI state isolation、Plan 007 evidence tests |
| PocketIC integration | PASS | 54 tests |

## 残存リスクと本番blocker

- SNS proposalのrepository-ownedな提出・certificate/executed状態照合と、x402 calldata/receiptの真正性検証が未実装である。自己申告のraw bytesとdigestだけでGate Bを通過させないため、これらが実装・検証されるまで`verify-live`とactivation driverは必ず非ゼロ終了する。
- 監視演習のBase receipt/logとIC certificate/auditをrepository-ownedに検証する経路が未実装である。自己申告のraw bytesとdigestだけでGate Aを通過させないため、真正性検証が実装されるまで`validate-bundle --offline`とdeploy driverは必ず非ゼロ終了する。
- 実Base RPC 3 provider、実ICRC Ledger、実SNS/controller handover、監視通知、x402の証跡は外部rehearsalが必要である。
- EVM RPC quorum、Finalized意味論、ICRC履歴、SQLite VFS、threshold key、運用鍵管理は信頼境界として残る。
- 本番deploy、controller handover、unpause、資産受付開始は、この作業ツリーのテスト成功だけでは承認されない。repository-ownedな真正性検証済みGate A/Gate B evidenceと別の明示承認が必要である。
