# Sepolia staging evidence

現在の`local-e2e.json`はschema v7の`short-delay-test-only` local promotion証跡であり、既定パスの`sepolia-e2e.json`はそのSHA-256とsource commitをstaging bindingに固定する。既定パスのmanifestと`artifacts/`だけが、driverで再開できる現行staging証跡である。

`archive/<source-prefix>/`は過去系列をmanifestとartifactの組で保持する監査履歴であり、再開、追記、現行staging判定には使用しない。旧`dbedb941`系列は`archive/dbedb941/`、version 31最終系列は`archive/f24c09d2/`へ固定している。`archive/dd0cbdb-failed-rpc-order/`は、version 32 reinstall直後の検査でCustom RPCの順序digest不一致を検出し、activation前に再reinstallした未使用installの証跡である。2026-07-31に確認したDeposit ID衝突と未救済5 TICRC1は失効した旧証跡として`incident-2026-07-31-deposit-id-collision.json`へ、2026-08-12に検出したreinstall後のactivation salt衝突は`incident-2026-08-12-activation-salt-collision.json`へ固定する。

evidence schema v7は、5分Timelock staging、既存test Canister v35 install（明示的なreinstall）、real frontend E2E、same-Wasm reopen検証がすべて成功した後に`node scripts/plan007/generate-local-e2e.mjs`で再生成する。evidenceの`deployment_instance_id`はlocal環境のversion 35 RuntimeBindingだけを固定し、staging bindingの正本には使わない。staging固有IDはfrontend profileで管理する。BridgeとbSNSの配置先固有immutable領域はSolidity artifactの`immutableReferences`でゼロ化し、正規化runtime template SHA-256を外部配置と照合する。配置先固有の生runtime hashは完成frontend profileへ別途固定する。`short-delay-test-only`証跡をproduction promotionまたは24時間Timelock rehearsalへ使用しない。
