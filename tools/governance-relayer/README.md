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

コマンドは`seal-operational-config`、`prepare`、`prepare-schedule-activation`、`prepare-execute-activation`、`status`、`relay`、`confirm`、`run`、`replace`、`refresh-attestation`、`drain-emergency`を提供する。初回activationの2つのprepareだけはproduction controller identityを使い、`--artifact-file`へ排他的に生成したJSONを変更不能なrelease証跡directoryへ保存する。`relay`と`confirm`はその同じfileがlive pending transactionと完全一致しなければ拒否する。`relay`は匿名actorで実行でき、`confirm`は固定confirmation relayer identityだけを使い、結果を`--receipt-file`へ排他的に保存する。activationをprepare・relay・confirmまで同一identityで連続実行するコマンドは提供しない。replacementは旧artifactとの一致を検証し、新しいgenerationを別の`--output-artifact-file`へ保存する。

`confirm`のtransaction hash指定は`--transaction-hash`を正規形とする。`--hash`は短縮aliasとして受理するが、両方の同時指定、同一optionの重複、コマンドに属さない未知optionはfail closedで拒否する。

`IC_IDENTITY_PEM`は`confirm`、`run`、`prepare`、`replace`、activation prepare、`refresh-attestation`、緊急操作に必要である。seal時に固定したproduction controllerがcontrollerである間のactivation prepareには同controller identityを使い、そのprincipalをcontrollerから外した後はGovernance identityを使う。`confirm`、`run`、attestation refreshにはrelease profileへ固定した専用confirmation relayer identityを使う。Service FeeにはGovernance identityを使い、pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause identityを使える。`status`とraw transactionの`relay`は匿名のままである。confirmation callerの報告は信用せず、保存済みoperation IDと署名generation hashの一致をRPC前に検査し、receiptや状態値はCanisterが独立観測する。

`run`と`drain-emergency`はreverted receiptを検出するとFinalized pollingを直ちに停止する。activationはprepareのJSONとGate B authorization/bindingを保存後、別processの`relay --artifact-file <artifact.json> --authorization-file <authorization.json> --binding-file <prepare-receipt.json>`、Finalized確認、専用confirmation relayerによる`confirm --artifact-file <artifact.json> --authorization-file <authorization.json> --binding-file <prepare-receipt.json> --receipt-file <confirmation.json>`の順で進め、Canister側の独立観測でoperationを終端化する。activation kindはbindingなしの`relay`/`confirm`とgeneric `run`/`replace`を拒否し、fee replacementはdriverから`replace-activation`を使って同じauthorizationへ新generationを結合する。

単独の`relay`は送信RPCの応答しか観測しないため、成功hashや`already known`を最終確認済みとは扱わない。CLI表示も常に`Unverified`とし、終端化にはCanister側の独立したFinalized観測を必要とする。
