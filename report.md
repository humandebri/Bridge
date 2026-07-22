# KINIC–Base Bridge セキュリティ修正・検証レポート

更新日：2026-07-22
対象：staged、unstaged、untrackedを含む現在の作業ツリー
前提：本番未デプロイのため、旧ABI、旧Candid、旧stable schemaとの互換処理は持たない

## 結論

採択済みの対応計画について、Contract、Canister、UI、release/evidence、local E2Eの修正を実装した。資産移動、管理権限、外部観測、stable storage、ブラウザ再送、デプロイ証跡の各境界は、曖昧または不整合な状態を成功として扱わない。

本番deployとactivationそのものは実行していない。Gate AにはIC certificate・emergency pause receipt/auditとBase receipt/logのrepository-owned検証を実装し、Gate BにはSNS upgrade proposalの認証済みquery、Root-only controller、live module hash照合を実装した。activationは固定SNS generic-function proposalの提出と成功検証を分離し、提案の`Executed`表示だけでは成功扱いにしない。Canisterのactivation状態とBase Timelockの2-of-3 Finalized postconditionを束縛したreceiptが発行されるまで、schedule/executeの完了を報告しない。x402はBridgeの配置・activation条件から除外した。

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
| Gate A | offline構造検査を非認可判定として分離し、IC certificateとBase 2-of-3 Finalized receipt/logをlive検証する |
| Gate B / activation | SNS upgradeとfunction registryを認証済みqueryで照合し、execute提出前にschedule receiptをlive再認証し、schedule/executeをCanister状態・canonical Finalized Base Timelock postcondition付きreceiptへ束縛する |
| Local E2E | Anvil/PocketICのport・PID・network stateを所有権付きで管理し、upgrade証跡をschema v2へ固定する |

## 検証結果

| 対象 | 結果 | コマンド・件数 |
|---|---|---|
| Rust workspace | PASS | `cargo test --workspace`、176 tests |
| Stable storage破損回帰 | PASS | 従来未検査だった8 tableのmalformed rowをすべて拒否 |
| Solidity | PASS | `forge test`、75 tests（fuzz/invariantを含む） |
| Formal proofs | PASS | Lean、Solidity SMT、Verus。Withdrawal成功時の`escrow debit = amountOut + ledgerFee`、`fee reserve credit = serviceFee - ledgerFee`、`liability debit = amountOut + serviceFee`を共有kernelで証明 |
| UI unit | PASS | Vitest 135 tests |
| UI static/build | PASS | typecheck、ESLint、Vite build |
| UI browser | PASS | Playwright desktop/mobile 8 tests |
| ABI/Candid生成 | PASS | `codegen:abi:check`、`codegen:candid:check` |
| Release/local safety | PASS | production driver/release、CI state isolation、Plan 007 evidence tests |
| PocketIC integration | PASS | 54 tests |

## 形式証明の責務範囲

形式証明が保証するのは、repository内の共有kernelと、そのkernelを呼ぶ状態遷移に対する算術・状態不変条件である。具体的にはfeeの一回計上、overflowとfee inversionの拒否、固定quoteと送金identityの一致、成功前のfee確定禁止、終端状態からの再送禁止などを対象とする。

形式証明だけでは、IC consensus、query signature、Base Finalizedの正しさ、RPC providerの独立性、SNS proposalが実際に対象methodへ到達した事実、ICRC Ledger履歴、threshold署名、運用者の手順遵守を保証しない。これらは認証済みquery/read-state、3 provider中2件の一致、receipt/log、統合テスト、運用evidenceで補完する。「全シナリオを数学的に防いだ」という主張はしない。

## 残存リスクと本番blocker

- 実Base RPC 3 provider、実ICRC Ledger、実SNS/controller handover、監視通知の証跡は外部rehearsalが必要である。
- EVM RPC quorum、Finalized意味論、ICRC履歴、SQLite VFS、threshold key、運用鍵管理は信頼境界として残る。
- immutable Base contractの欠陥が本番後に判明した場合、既存bSNSの救済経路はなく、別Bridge pairの再配置でも旧pair資産がstrandedになる可能性を受容している。
- fixed window境界では、各window上限の最大2倍が短時間に通過しうる。production値はこの係数を織り込むが、滑動窓の厳密上限ではない。
- Canister、threshold signing、cycles、EVM RPCの相関障害では、緊急時にBase側の再pauseまで到達できない可能性を受容している。独立した人間EVM管理鍵やPause Guardianは置かない。
- 本番deploy、controller handover、unpause、資産受付開始は、この作業ツリーのテスト成功だけでは承認されない。repository-ownedな真正性検証済みGate A/Gate B evidenceと別の明示承認が必要である。
