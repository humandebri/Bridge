---
status: accepted
---

# Replace the staging OnFinality provider with dRPC

Replace the Base Sepolia staging Custom RPC set in one direction, from the fixed order PublicNode, `sepolia.base.org`, OnFinality to PublicNode, `sepolia.base.org`, dRPC. The old digest is `3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48`; the new digest is `df7e867aaf6abeaf00b0f61e8662fa87c6f8675eb0aebdf7b09f8c99a499d064`.

Staging diagnostics on 2026-08-19 found that all three external providers agreed on the activation transaction receipt, while Finalized observations through the official EVM RPC Canister disagreed. OnFinality returned HTTP 401 to IC HTTPS outcalls; PublicNode and `sepolia.base.org` returned different Finalized heads, preventing a 2-of-3 threshold. The earlier Tenderly endpoint also failed to produce identical responses across IC HTTPS outcall replicas.

dRPC returned Base Sepolia chain ID `84532` externally and achieved replica consensus for a single-provider Finalized response through the official EVM RPC Canister. In the three-provider call using PublicNode, `sepolia.base.org`, and dRPC, PublicNode and dRPC converged on block `45678951`, returning `Consistent(Ok(...))`. Use this result through IC as replacement evidence of provider suitability that external `eth_chainId` checks alone cannot establish.

Only `test-deployment` Wasm accepts this replacement, fixing the Base Sepolia chain ID, official EVM RPC Canister ID, old/new sets, ordering, both digests, and live state counts. An unknown current digest, unknown provider, changed ordering, or state drift rolls back the entire upgrade. Preserve the saved Finalized watermark and invalidate only the runtime attestation cache.

This decision changes only the staging provider set, preserving [ADR 0024](0024-validate-rpc-chain-binding-before-runtime.md)'s configured chain binding, install-domain binding, and runtime quorum responsibilities. Before activation, call `eth_chainId` on every new endpoint and retain evidence of Finalized quorum through the official EVM RPC Canister.
