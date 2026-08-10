# Sepolia staging evidence

現在の`local-e2e.json`はschema v7の`short-delay-test-only` local promotion証跡であり、既定パスの`sepolia-e2e.json`はそのSHA-256とsource commitをstaging bindingに固定する。既定パスのmanifestと`artifacts/`だけが、driverで再開できる現行staging証跡である。

`archive/<source-prefix>/`は過去系列をmanifestとartifactの組で保持する監査履歴であり、再開、追記、現行staging判定には使用しない。旧`dbedb941`系列は`archive/dbedb941/`へ固定している。2026-07-31に確認したDeposit ID衝突と未救済5 TICRC1は失効した旧証跡として`incident-2026-07-31-deposit-id-collision.json`へ固定する。

schema v7のevidenceは、5分Timelock staging、Canister v32 upgrade、real frontend E2E、same-Wasm reopen検証がすべて成功した後に`node scripts/plan007/generate-local-e2e.mjs`で再生成する。evidenceの`deployment_instance_id`はlocal環境のv32 PublicConfigだけを固定し、staging bindingの正本には使わない。staging固有IDはfrontend profileで管理する。BridgeとbSNSの配置先固有immutable領域はSolidity artifactの`immutableReferences`でゼロ化し、正規化runtime template SHA-256を外部配置と照合する。配置先固有の生runtime hashは完成frontend profileへ別途固定する。`short-delay-test-only`証跡をproduction promotionまたは24時間Timelock rehearsalへ使用しない。
