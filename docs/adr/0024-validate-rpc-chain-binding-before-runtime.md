---
status: accepted
---

# RPC chain bindingを稼働前に検証する

Bridgeのconfigured chain IDはinstall domainを構成する設定値であり、Finalized block responseから観測した値ではない。Custom RPCを使う環境では、operatorがRPC URL、期待chain ID、各URLの接続先chainをdeployment lifetimeにわたって固定する。deploy・activation前preflightはreview済みの3 endpointすべてへ`eth_chainId`を実行し、到達不能、不正応答、期待chain IDとの不一致が1件でもあればfail closedにする。

本番Base MainnetのCanister outcallは公式EVM RPC Canisterの組み込み`BaseMainnet` provider群を使用し、`custom_evm_rpc_urls`は空配列に固定する。本番profileの3件のcredential-free RPC URLはCanisterへ注入せず、Gate A、activation前live preflight、UI監視の独立観測に使用する。Base Sepolia stagingと`test-deployment` buildでは、review済みのCustom RPC 3件を使用する。staging Wasmには、review済みTenderly集合からreview済みOnFinality集合への一方向置換だけを行うupgrade例外がある。旧・新URL配列、順序、両digest、Base Sepolia chain ID、公式EVM RPC Canister IDをすべて固定し、新集合への同値再実行以外の変更はtrapする。production Wasmはこの型・decoder・置換経路をコンパイルしない。これらのprovider集合を同一のものとして扱わない。

runtimeの2-of-3 quorumは、Finalized観測、canonical block hash、receipt、contract stateなどの応答不一致とprovider障害を扱う。`notify_withdrawal`のFinalized観測だけは最新headの完全一致ではなく、2 provider以上が証言する最大checkpointを選び、その高さのcanonical block hashをexact 2-of-3で再取得する。他のruntime snapshot、deposit、refundのFinalized head取得は変更しない。provider URLまたはその接続先chainが稼働中に切り替わる脅威への検知手段ではない。stagingの固定置換時は保存済みFinalized水位を保持しつつruntime attestation cacheを失効させ、次のEVM観測で新provider集合からruntime codeを再確認する。この設計では稼働中の`eth_chainId`反復検証も、期限付きchain attestationも行わない。

## 記録の意味

- `FinalizedObservation`は、RPCから実測したFinalized block番号、block hash、観測時刻だけを保持する。
- RPC auditのrequest digestはconfigured chain IDへ束縛する。quorum response digestはchain IDをRPC観測結果として含めない。
- stable `FinalizedObservationRecord.chain_id`は、保存したblock観測をinstall domainへ束縛するconfigured chain IDである。RPC responseから取得したchain IDではない。
- Mint evidence、EIP-712 domain、Governance nonceなどのchain bindingにはconfigured chain IDを使用する。
- stable recordと現在のconfigのbinding不一致、および異なるinstall-domain record間の競合は引き続き拒否する。

## 不採用案

- 各runtime operationで`eth_chainId`を呼ぶ案は、設定時に検証済みで稼働中不変とする接続先を反復検証するだけであり、本設計の脅威モデルには追加の安全性を与えないため採用しない。
- 期限付きchain attestationを更新する案は、provider接続先の稼働中切替を別の脅威として導入するため採用しない。
- configured chain IDを`FinalizedObservation`へ代入して同じ設定値と比較する案は、RPC観測を証明しないtautologyになるため採用しない。
- configured chain IDをquorum response auditへ含める案は、設定値をproviderの応答値として誤読させるため採用しない。

## 再検討条件

今回固定したstagingの一方向置換を超えてRPC URL、configured chain ID、または各URLの接続先chainを稼働中に変更可能にする場合は、この決定を再検討する。その変更では、attestationの失効条件、stable install-domain binding、audit意味論、既存recordの扱い、runtime quorumの責務を新しい脅威モデルに基づいて設計し直す。

claimが依存する外部仮定とfail-closed動作の機械可読な正本は`verification/assumptions.tsv`の`rpc_provider_chain_configuration`とする。operator手順は`docs/runbooks/operations.md`、rehearsal条件は`docs/runbooks/evm-rpc-canister-rehearsal.md`、証跡要件は`deployments/evidence-v1/README.md`に従う。
