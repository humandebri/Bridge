# Phase 0〜1Aの検証境界

Bridgeのビジネスロジックを導入する前に、両検証器が実際に動作することを確認する。

- `smt/pass`はSolidity SMTCheckerのCHC engineで証明する。
- `smt/fail`はassertionのcounterexampleにより拒否されなければならない。
- `verus/pass.rs`は実行可能なRust関数1件を証明する。
- `verus/fail.rs`は事後条件違反により拒否されなければならない。

Phase 1AではBase interfaceのselectorと型だけを固定し、Bridge固有の不変条件をまだ証明しない。IC runtime、Base実行、ledger semantics、modelと本番実装の対応は未検証である。後続Phaseでsmoke用の性質をADRの証明義務へ置換し、外部仮定を明記する。
