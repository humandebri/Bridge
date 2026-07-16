# Bridge verification boundary

Withdrawalの検証対象は、Base上の不可逆な`Committed` burnとCanister上の未決済債務である。Base refund、release acknowledgement、Withdrawal用EVM operationはモデルに存在しない。

- Foundryはfee driftのburn前revert、固定quote、atomic burn、削除selector不在、Committed後の再mint不能を検査する。
- `bridge-core/src/kernel.rs`はCargoとVerusで共有し、固定quote、Ledger Fee上限、phase遷移、fee一回計上を検査する。
- LeanはBase supply減少とCanister債務発生、固定宛先への支払、1:1 backingをモデル化する。
- Rust/integrationはcanonical Finalized照合、Ledger成功・Duplicate・BadFee・曖昧結果、純額Fee reserve、追加EVM transaction不在を検査する。

Solidity SMTはharnessの性質であり、完全なdeployed contract proofではない。providerの`finalized`意味論、EVM RPC quorum、ICRC履歴の真正性、SQLite atomicityは外部仮定である。

本番未デプロイのためschema v10再オープンとwire v9だけを検証し、旧schema migration、compatibility shim、dual-read、fallbackは提供しない。
