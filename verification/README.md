# Phase 0〜1Eの検証境界

productionと共有する純粋coreをSMTCheckerで検証し、Verusは検証器自体のpass/fail fixtureを維持する。

- `smt/pass`はproductionの`MintAccounting`を直接使い、net mint量、window消費量の単調性、成功時の上限保存をSolidity SMTCheckerのCHC engineで証明する。
- `smt/pass`は`WithdrawalAccounting`も直接使い、safeなsettlement分解、fee上限、minimum、ReleaseのApply・Idempotent・Reject判定、terminal状態の排他性を証明する。
- `smt/pass`は`BridgeAdministration`も直接使い、Runtime Administratorのlimit変更方向、role分離、Service Fee上限を証明する。
- `smt/fail`は各fixtureを個別に実行し、window境界を欠くfixture、`Released → Refunded`、Runtime Administratorによるlimit引上げがそれぞれ確定counterexampleとして拒否されることを確認する。
- `verus/pass.rs`は実行可能なRust関数1件を証明する。
- `verus/fail.rs`は事後条件違反により拒否されなければならない。

Phase 1Eでは上記libraryのSMT証明に加え、concrete ABI snapshot、stateful Bridge invariant、EIP-3009 authorization nonce fuzzをFoundry gateへ加える。
EVM transaction rollback、caller modifier、Deposit IDとledger block indexのmapping一意性、event非重複、OpenZeppelin ERC-20、EIP-712、ECDSA、TimelockController、block timestampは証明せず、Foundry testまたは外部仮定とする。
Bridgeは指定addressが正しい72時間Timelockであることを証明しない。
Release acknowledgementはBridge Signerを信頼した記録であり、ICP Releaseの暗号学的proofではない。
IC runtimeとledger semanticsは後続Phaseの検証対象とする。

個別の主張、検証手段、外部仮定の対応は[obligations.md](obligations.md)を参照する。
