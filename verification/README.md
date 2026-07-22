# Bridge verification boundary

Withdrawalの検証対象は、Base上の不可逆な`Committed` burnとCanister上の未決済債務である。Base refund、release acknowledgement、Withdrawal用EVM operationはモデルに存在しない。

- Foundryはfee driftのburn前revert、固定quote、atomic burn、削除selector不在、Committed後の再mint不能を検査する。
- `bridge-core/src/kernel.rs`はCargoとVerusで共有し、固定quote、phase遷移、fee一回計上を検査する。
- `bridge-core/src/kernel.rs`はさらにsnapshot refresh owner、reserve observation token、settlement lease generationをproductionと共有し、stale worker、drift、generation wrapを拒否する。
- LeanはBase supply減少とCanister債務発生、固定宛先への支払、1:1 backingに加え、frontendのFinalized成功・revert・retry判断とserialized queue更新をモデル化する。
- Rust/integrationはcanonical Finalized照合、Ledger成功・Duplicate・BadFee・曖昧結果、純額Fee reserve、追加EVM transaction不在を検査する。

Solidity SMTはproduction共有predicateの性質であり、完全なdeployed contract proofではない。frontend LeanモデルはTypeScript実装そのものの証明ではなく、純粋な判断関数との対応を網羅テストで検査する。EVM rollbackとEIP-1153 transient storage lifetime、Web Locks、browser storage、providerの`finalized`意味論、EVM RPC quorum、wallet、ICRC履歴の真正性、SQLite atomicityとSQL row selectionは外部仮定である。Ledger Fee超過はruntime guardでrelease前に停止し、Base withdrawal pauseとfee同期後に同じrecordを再検証する。

本番未デプロイのためschema v17再オープンとwire v15だけを検証し、旧schema migration、compatibility shim、dual-read、fallbackは提供しない。
