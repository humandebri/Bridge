# 30分で確認するBridge証明の骨格

## 命題と保証対象（5分）

**Claim contract**は、release claimごとのLean命題である。
**本番証拠**は、その命題を支える共有kernelのVerus義務、有限個のvector照合、transaction testである。
両者の接続と外部仮定は[claim台帳](claims.tsv)を参照する。
[仕様対応表](claim-semantics.tsv)は前提と結論を説明し、[主要定義表](definition-semantics.tsv)は共通概念の意味を一か所に置く。
新しい表から証拠強度を算出することはない。

[statement一覧](generated/claim-statements.md)では、witnessの型から命題定義へ進み、前提が強すぎないか、結論が仕様の一部を落としていないかを確認する。
たとえば署名のfee一回性と最低残存時間は別の条件であり、両方を命題で確認する。

## 会計差分から履歴へ（10分）

GlobalHistoryのbackingは、escrowがBase supply、fee reserve、未mint債務、未release債務の合計に等しいという条件である。
署名は未mint債務からfee reserveへ移し、mintは未mint債務からBase supplyへ移す。
refundはescrowと未mint債務を同額減らす。
payoutではescrow減少とfee reserve増加の合計が未release債務の減少と一致する。

この差分をrecordと全体集計へ同時に適用する。
一歩の保存定理は、IDの一意性、record合計と集計の一致、backing、予約を持つrecordがDepositであることを保存する。
別IDのrecordが変わらないことは独立した補題で示す。
受理履歴の長さに関する帰納法が、一歩の保存を履歴全体へ拡張する。

ここでの`AccountingInvariant`は実Ledger操作の認証条件を含まない。
`ModelBoundaries.accounting_invariant_does_not_certify_payment`は、会計条件を満たすrecordが、送金をせずcallbackでpaidへ移る例である。
これは会計モデルの表現範囲を固定する例であり、本番でそのcallbackが受理されるという主張ではない。

## 個別の保証と抽象化（8分）

DepositHistoryは認可の発行から終端までの履歴を扱う。
署名イベントはIC観測時刻を受け取り、期限のu64範囲、加算のoverflow拒否、残り300秒以上を検査する。
300秒ちょうどを受理し、299秒、期限超過、最大時刻での加算を拒否する。
本番の最低残存時間predicateとの一致は有限幅モデルの定理とRustのvector consumerで結び付ける。
暗号署名そのものとIC時刻の真正性は外部仮定である。

統合Protocolでは、保存したquoteの宛先と純額が各遷移で変わらないことを示す。
その保存則を履歴へ拡張し、最終withdrawalの値が初期状態の保存quoteに一致することを導く。
`pending_payout_is_bounded_by_reserve_across_trace`はpayout予約とreserveの関係を示す補題であり、service feeの上下限は局所predicateの定理で確認する。

`filterSafeStoredState`はSafeを前提に状態を受理する抽象フィルタである。
その出力がSafeである証明から、実際のSQLite decodeやschema migrationの安全性は導けない。
本番の復元は既存のRustとPocketICのtransaction test、SQLiteの原子性、固定schemaの外部条件に依存する。

条件付きlivenessは、対象終端操作が選択されるまで常時受理可能であり、必要な外部操作が可能で、dispatcherがweakly fairであることから到達性を導く。
資金受領からこの受理可能性を導く証明はない。
`DepositTerminalProgressLemmas`はmintの含意とrefundの含意の論理積であり、同じ実行についての二者択一を表す命題ではない。

## 変更をレビューする手順（7分）

まず仕様対応表の前提、結論、未証明境界を確認する。
次にstatement一覧と主要定義の差分を読み、初期値との関係、有限幅、認可条件、受理条件が失われていないか確認する。
実装との対応はclaim台帳に戻り、共有kernelを検査する義務と、モデルだけの補題を区別する。

命題や主要定義を変更したら対応表の説明または`review_note`も更新する。
生成物は次のコマンドで更新する。

```sh
python3 scripts/check_claim_semantics.py --write
python3 scripts/check_claim_semantics.py --base-sha <trusted-baseの40桁SHA>
```

比較元は指定したGit commitから読む。
同じcandidateのJSONを書き換えるだけでレビューを不要にはできない。
trusted CIでは既存の`BRIDGE_TRUSTED_BASE_SHA`を使い、`verification/`への変更に必要な対象HEADのレビューを維持する。
比較元にsnapshotがない初回導入はbootstrapとして表示する。
既存trusted-baseが新しい検査コードを含まない間は、ローカル検証と既存のbootstrap手順が必要である。

ソースdigestはコメントや証明の書き換えも通知する。
この通知は意味が変わったことの証明ではなく、人が差分を読むための保守的な検査である。
主要定義に列挙していない依存定義や構造の変更もソースdigestに現れるため、該当ソースの差分を確認する。

独立カーネルでの検査は未導入である。
現行の公理依存検査と通常のLean buildが通ることを、独立カーネルの結果として報告しない。
