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

`IC_IDENTITY_PEM`はCanister APIの認証専用である。Service FeeとactivationにはGovernance identityを使い、pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause identityを使える。`SigningUnavailable`では安全な失敗分類を表示し、自動再試行しない。`InsufficientCycles`はtop-upとreserve確認後、`CallRejected`、`CallFailed`、`CostUnavailable`はchain-key serviceの回復確認後にだけ、保存済み`Prepared` transactionを明示的に再試行する。それ以外の分類は再試行せず、canister stateとcontroller-only logsを調査する。`BASE_RPC_URL`にAPI keyを含める場合も、値を出力・共有しない。
