# Bridge verification boundary

Lean projectはcross-chain protocolの正式な抽象仕様である。
状態遷移、不変条件、frontendの判断、pending queueの更新を`verification/lean/BridgeSpec`へ集約し、Lakeで定理を検査する。
Lean executableが生成する`verification/generated/protocol-vectors.json`をRust、Solidity、TypeScriptのconsumerで読み、実装の代表的な境界値を同じ期待値と照合する。
仕様、定理、consumerの対応は`verification/refinement-manifest.tsv`で固定し、未登録または欠落した対応をCIで拒否する。
manifestはsection、Lean definition、Lean theorem、runner、consumer source、test selectorの6列で構成し、同じsectionに複数consumerを登録できる。
CIは許可済みのRust、Foundry、Vitest runnerだけを使用し、各selectorが正確に1件成功したことを機械可読な結果から確認する。
このvector照合は列挙されたcaseに対するbounded conformanceであり、Rust、Solidity、TypeScript実装全体の完全なsemantic refinementではない。

Withdrawalの検証対象は、Base上の不可逆な`Committed` burnとCanister上の未決済債務である。Base refund、release acknowledgement、Withdrawal用EVM operationはモデルに存在しない。

- Foundryはfee driftのburn前revert、固定quote、atomic burn、処理済みDeposit IDのreplay拒否を検査し、ABI snapshotはWithdrawal専用のrefund/remint selectorが存在しないことを検査する。
- `bridge-core/src/kernel.rs`はCargoとVerusで共有し、固定quote、phase遷移、fee一回計上を検査する。
- `bridge-core/src/kernel.rs`はさらにsnapshot refresh owner、reserve observation token、settlement lease generationをproductionと共有し、stale worker、drift、generation wrapを拒否する。
- LeanはBase supply減少とCanister債務発生、固定宛先への支払、1:1 backingに加え、frontendのFinalized成功・revert・retry判断とserialized queue更新を正式な抽象モデルとして定義する。
- Rust/integrationはcanonical Finalized照合、Ledger成功・Duplicate・BadFee・曖昧結果、純額Fee reserve、追加EVM transaction不在を検査する。

Solidity SMTはproduction共有predicateの性質であり、完全なdeployed contract proofではない。
frontend LeanモデルはTypeScript実装そのものの証明ではなく、生成vectorと純粋な判断関数との対応をテストで検査する。
Bridge Signerは通常のDeposit mint権限を持つため、Withdrawal専用remint経路の不在は、侵害されたSignerが別の未処理Deposit IDをmintできないことを意味しない。
EVM rollbackとEIP-1153 transient storage lifetime、Web Locks、browser storage、providerの`finalized`意味論、EVM RPC quorum、wallet、ICRC履歴の真正性、SQLite atomicityとSQL row selectionは外部仮定である。
Ledger Fee超過はruntime guardでrelease前に停止し、Base withdrawal pauseとfee同期後に同じrecordを再検証する。

本番未デプロイのためschema v18再オープンとwire v15を検証する。実配置済みstaging v17から空のDeposit状態だけを受理する限定migration以外に、compatibility shim、dual-read、fallbackは提供しない。
