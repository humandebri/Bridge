# Governance relayer CLI

Bridge Canisterがthreshold署名したBase governance transactionを、運用者が明示的に送信・確定するCLIである。EVM秘密鍵は使用しない。

```bash
export BRIDGE_CANISTER_ID='...'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
export IC_IDENTITY_PEM='/secure/path/governance.pem'
npm run governance-relayer -- prepare --action pause-deposits
unset IC_IDENTITY_PEM
export IC_IDENTITY_PEM='/secure/path/confirmation-relayer.pem'
npm run governance-relayer -- run
```

コマンドは`prepare`、`status`、`relay`、`confirm`、`run`、`replace`、`refresh-attestation`、`drain-emergency`を提供する。Gate Bの直前に専用confirmation relayer identityで`refresh-attestation`を実行する。`status`と`relay`は匿名actorを使える。`confirm`とconfirmationを含む`run`は専用confirmation relayer identityを使い、保存済みpending成果物の同じraw transactionだけを送信する。replacementはCanisterへ新feeを明示して再署名を依頼し、CLI側では作成しない。

`IC_IDENTITY_PEM`は`confirm`、`run`、`prepare`、`replace`、activation、`refresh-attestation`、緊急操作に必要である。`confirm`、`run`、attestation refreshにはrelease profileへ固定した専用confirmation relayer identityを使う。Service FeeとactivationにはGovernance identityを使い、pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause identityを使える。`status`とraw transactionの`relay`は匿名のままである。confirmation callerの報告は信用せず、保存済みoperation IDと署名generation hashの一致をRPC前に検査し、receiptや状態値はCanisterが独立観測する。追加rate limitとcooldownは未実装で、既存singleflightだけを維持する。`SigningUnavailable`では安全な失敗分類を表示し、自動再試行しない。`BASE_RPC_URL`にAPI keyを含める場合も、値を出力・共有しない。

`run`、activation、`drain-emergency`はreverted receiptを検出するとFinalized pollingを直ちに停止する。receipt確定後、専用confirmation relayer identityで`confirm --operation-id <id> --transaction-hash <hash>`を実行し、Canister側の独立したFinalized観測でoperationを終端化する。
