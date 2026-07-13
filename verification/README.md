# Bridge verification boundary

## 証明済み

`bridge-core/src/kernel.rs`のproduction共有式をVerusとCargoでdual-compileする。従来の履歴照合に加え、Settlement Reserve、scheduler優先度、nonce、fee payout、administrator権限、audit sequence、EVM rankを証明する。`verus/manifest.tsv`は各資産安全kernelをpass proofと独立negative fixtureへ対応付ける。

## production adapterでモデル化済み

Deposit、Withdrawal、EVM operation、Reconciliation Holdのrecord APIは共有kernelの判定を呼ぶ。rich recordのfield保存、terminal排他、fee coordinator、stable counter更新はRustの遷移表・有限総当たり・再オープンテストでrefinementを確認する。

## テストのみ

StableBTreeMap、wire CBOR、schema v4再オープン、Candid、timer task、ledger/EVM adapterはRust、PocketIC、smoke testの対象でありVerusの証明対象ではない。
未デプロイのためlegacy schema migrationは設けない。

## 外部仮定

IC message rollback、stable structures、Serde/CBOR、ICRC ledger履歴完全性、Base finality、EVM RPC canisterの集約結果、threshold ECDSA、外部サービスの可用性を信頼境界とする。

reserveに入力するprovider合意済みETH残高、canister cycles残高、gas上限の真正性と妥当性は外部仮定である。
