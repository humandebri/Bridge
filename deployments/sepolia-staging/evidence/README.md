# Sepolia staging evidence

現行の外部staging証跡はschema v8だけを受理する。v8 manifestは`scripts/plan007/staging-e2e-driver.sh`で新規初期化し、固定10 stageを順に記録して、全受入条件を満たした場合だけ`SHORT_DELAY_LIVE`となる。現時点のrepositoryには完成済みv8 live-acceptance manifestを保存しておらず、過去証跡を遡及的に合格扱いしない。

checked-in `local-e2e.json`と`archive/<source-prefix>/`以下のschema v7 manifest/artifactは読取専用の監査履歴である。v7はresume、追記、migration、dual-read、現行staging判定に使用しない。新しいschema v8 local evidenceはclean commitから`scripts/plan007-local-gate.sh /secure/work/local-e2e.json`でrepository外へ生成し、driverの`BRIDGE_STAGING_LOCAL_EVIDENCE`へ明示指定する。

`reinstall-decision-2026-08-27.json`と`fresh-stack-2026-08-28.json`は、既存Canister principalを一度だけdestructive reinstallし、現在のdeployment instanceとBase contractsを作成した履歴を固定する。v8の`bootstrap_attestation`はこの2 artifactのhashと現行bindingを照合するだけで、reinstall、contract redeploy、reactivationを実行または許可しない。将来の更新は同じCanister ID、deployment instance、schema v35／wire v30を保つcurrent-schema upgradeだけを受理する。

v8のupgrade、binding、frontend、smoke、wallet、refund各stageはsummaryと一致するhash-bound stage receiptを必須とする。`rpc_rehearsal`は専用verifierを通過した`rpc-rehearsal-manifest`、`live_acceptance`はreactivation schedule/execute receiptと監視receiptを必須とし、自己申告summaryだけでは`SHORT_DELAY_LIVE`へ遷移しない。

`archive/dbedb941/`、`archive/f24c09d2/`、`archive/dd0cbdb-failed-rpc-order/`、`archive/dd0cbdb-failed-activation-salt/`は失効した過去系列である。Deposit ID衝突、activation salt衝突、失敗RPC順序を含む各履歴は現行bindingや受入条件の根拠にしない。

`short-delay-test-only`証跡をproduction promotionまたは259200秒Timelock rehearsalへ使用しない。秘密情報やcredential付きRPC URLをartifactへ保存しない。
