---
status: accepted
---

# staging OnFinality providerをdRPCへ置換する

Base Sepolia stagingのCustom RPC集合は、PublicNode、`sepolia.base.org`、OnFinalityの固定順序から、PublicNode、`sepolia.base.org`、dRPCの固定順序へ一方向に置換する。旧digestは`3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48`、新digestは`df7e867aaf6abeaf00b0f61e8662fa87c6f8675eb0aebdf7b09f8c99a499d064`である。

2026-08-19のstaging診断では、activation transaction receiptは外部3 providerで一致した一方、公式EVM RPC Canister経由のFinalized観測は不一致になった。OnFinalityはIC HTTPS outcallへHTTP 401を返し、残るPublicNodeと`sepolia.base.org`は異なるFinalized headを返したため、2-of-3 thresholdを成立させられなかった。旧Tenderly endpointもIC HTTPS outcallのreplica間で同一応答を形成できなかった。

dRPCは外部からBase Sepolia chain ID `84532`を返し、公式EVM RPC Canister経由の単独Finalized応答でもreplica consensusを形成した。PublicNode、`sepolia.base.org`、dRPCの3-provider呼出しではPublicNodeとdRPCが同一block `45678951`へ収束し、`Consistent(Ok(...))`を返した。このIC経由結果を、外部`eth_chainId`検査だけでは検出できないprovider適合性の置換証拠とする。

置換は`test-deployment` Wasmだけが受理し、Base Sepolia chain ID、公式EVM RPC Canister ID、旧集合、新集合、順序、両digest、live state countsを固定する。未知のcurrent digest、未知provider、順序変更、state driftはupgrade全体をrollbackする。保存済みFinalized水位は保持し、runtime attestation cacheだけを失効させる。

この決定はstaging provider集合だけを変更し、[ADR 0024](0024-validate-rpc-chain-binding-before-runtime.md)のconfigured chain binding、install-domain binding、runtime quorumの責務を変更しない。activation前に新集合の全endpointへ`eth_chainId`を実行し、公式EVM RPC Canister経由のFinalized quorumが成立する証拠を保存する。
