# Governance relayer CLI

Bridge Canisterがthreshold署名したBase governance transactionを、運用者が明示的に送信・確定するCLIである。EVM秘密鍵は使用しない。

```bash
export BRIDGE_CANISTER_ID='...'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
export IC_IDENTITY_PEM='/secure/path/governance.pem'
npm run governance-relayer -- prepare --action pause-deposits
unset IC_IDENTITY_PEM
npm run governance-relayer -- run
```

コマンドは`prepare`、`status`、`relay`、`confirm`、`run`、`replace`、`drain-emergency`を提供する。`status`、`relay`、`confirm`、`run`は匿名actorを使い、`run`は保存済みpending成果物の同じraw transactionだけを送信する。replacementはCanisterへ新feeを明示して再署名を依頼し、CLI側では作成しない。

`IC_IDENTITY_PEM`は`prepare`、`replace`、activation、緊急操作だけに必要である。Service FeeとactivationにはGovernance identityを使い、pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause identityを使える。匿名confirmationは保存済みoperation IDと署名generation hashの一致をRPC前に検査し、receiptや状態値をrelayerから受け取らない。追加rate limitとcooldownは未実装で、既存singleflightだけを維持する。`SigningUnavailable`では安全な失敗分類を表示し、自動再試行しない。`BASE_RPC_URL`にAPI keyを含める場合も、値を出力・共有しない。
