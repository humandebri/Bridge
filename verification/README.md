# Phase 0〜1Bの検証境界

Bridgeのビジネスロジックを導入する前に、両検証器が実際に動作することを確認する。

- `smt/pass`はproductionの`MintAccounting`を直接使い、net mint量、window消費量の単調性、成功時の上限保存をSolidity SMTCheckerのCHC engineで証明する。
- `smt/fail`はwindow requested量の境界検査を意図的に除去し、assertionの確定counterexampleにより拒否されなければならない。
- `verus/pass.rs`は実行可能なRust関数1件を証明する。
- `verus/fail.rs`は事後条件違反により拒否されなければならない。

Phase 1BのSMT証明は算術libraryの非revert経路だけを対象とする。EVM transaction rollback、Deposit ID mappingの一意性、OpenZeppelin ERC-20・EIP-712・ECDSAの正当性、block timestampの単調性は証明せず、Foundry testまたは外部仮定とする。Bridge全体の状態遷移、IC runtime、ledger semanticsは未検証であり、後続PhaseでADRの証明義務を追加する。
