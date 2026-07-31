# Sepolia staging evidence

旧Canister発Mint transaction方式の`local-e2e.json`は、EIP-712 Mint Authorization移行で無効になったため削除した。

schema v4のevidenceは、Bridge再deploy、Canister v25 reinstall、real frontend E2E、same-Wasm reopen検証がすべて成功した後に`node scripts/plan007/generate-local-e2e.mjs`で再生成する。旧evidenceをpromotionやpreflightへ使用しない。
