# Sepolia staging evidence

旧Canister発Mint transaction方式の`local-e2e.json`は、EIP-712 Mint Authorization移行で無効になったため削除した。

schema v6のevidenceは、5分Timelock staging Bridge再deploy、Canister v28 reinstall、real frontend E2E、same-Wasm reopen検証がすべて成功した後に`node scripts/plan007/generate-local-e2e.mjs`で再生成する。BridgeとbSNSの配置先固有immutable領域はSolidity artifactの`immutableReferences`でゼロ化し、正規化runtime template SHA-256を外部配置と照合する。配置先固有の生runtime hashは完成frontend profileへ別途固定する。`short-delay-test-only`証跡をproduction promotionまたは24時間Timelock rehearsalへ使用しない。
