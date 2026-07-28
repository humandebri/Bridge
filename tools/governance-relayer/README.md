# Governance relayer CLI

Bridge Canisterがthreshold署名したBase governance transactionを、運用者が明示的に送信・確定するCLIである。EVM秘密鍵は使用しない。

```bash
export BRIDGE_CANISTER_ID='...'
export IC_IDENTITY_PEM='/secure/path/governance.pem'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
npm run governance-relayer -- prepare --action pause-deposits
npm run governance-relayer -- run
```

コマンドは`prepare`、`status`、`relay`、`confirm`、`run`、`replace`、`drain-emergency`を提供する。`run`はpending成果物があれば同じraw transactionから再開する。replacementはCanisterへ新feeを明示して再署名を依頼し、CLI側では作成しない。

`IC_IDENTITY_PEM`はCanister APIの認証専用である。Service FeeとactivationにはGovernance identityを使い、pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause identityを使える。`status`が`SigningUnavailable`を返した場合、通常操作は同じ`prepare`、緊急操作は`drain-emergency`を再実行して保存済み`Prepared` transactionの署名を再試行する。`BASE_RPC_URL`にAPI keyを含める場合も、値を出力・共有しない。
