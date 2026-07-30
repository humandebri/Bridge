# Bridge verification boundary

Lean projectはcross-chain protocolの正式な抽象仕様である。
状態遷移、不変条件、frontendの判断、pending queueの更新を`verification/lean/BridgeSpec`へ集約し、Lakeで定理を検査する。
Lean executableが生成する`verification/generated/protocol-vectors.json`をRust、Solidity、TypeScriptのconsumerで読み、実装の代表的な境界値を同じ期待値と照合する。
release対象claimは`Claims.lean`、有限幅semanticsは`Implementation.lean`、抽象モデルとの対応は`Refinement.lean`、統合状態traceとcertificateは`Protocol.lean`へ分離する。
`verification/claims.tsv`はprotocolとMint Authorizationのclaim、Lean定理、Verus義務、SMT scalar義務、production symbol、transaction test、vector section、外部仮定を一つの型付きmanifestで管理する。証拠statusはmanifestへ手入力せず、`scripts/check_claim_manifest.py`が最弱要素から算出して`verification/output/claim-report.json`へ出力する。外部仮定を含むclaimは必ず`partial`となる。`claim-test-manifest.tsv`のselectorはclaimのsymbolと同一、またはVitest/Jestでそのsymbolをnamed callbackとして直接登録したtest名でなければならない。
`verification/proof-impact.tsv`は安全関連production sourceをclaimと必須proof stageへ対応付ける。watched root内のRustまたはSolidity sourceが未登録、claimのproduction sourceにownerがない、またはproof receiptのsource fingerprintが現在のsource・proof資材と異なる場合、`scripts/check_proof_impact.py`はfail closedにする。fingerprintはproduction source、proof consumerとtest（PicJS integrationを含む）、driver、toolchain・build・test設定、lockfileを保守的に含み、生成outputとbuild cacheだけを除外する。schema v4 receiptは必須stageが順序どおり一度ずつ`pass`し、claimが空でない場合だけcompleteとして受理する。`claim-transaction-tests` stageは`verification/claim-test-manifest.tsv`へ登録された各testが正確に1件成功したことを検査する。定理がそのまま適用できる変更に無意味なproof file差分は要求せず、現在のsourceに対する全stageの再実行を要求する。

変更されたMint Authorization経路の安全claimは同manifest内でAuthorization binding、expiry refund、exact mint finalization、epoch invalidation、reservation lifecycle、fee一回性、deposit backing、manual claim exclusionへ分割する。暗号known-answerは`verification/known-answer-manifest.tsv`で別管理し、Lean生成policy証拠へ昇格させない。
CIはこれらの全Lean theorem、Verus manifest、claim台帳、全vector section、全consumerを完全一致検査し、production linkはcompilerで型検査する。
refinement manifestはsection、抽象定義、有限幅implementation定義、対応定理、runner、consumer source、test selector、production symbolの8列で構成し、同じsectionに複数consumerを登録できる。
CIは許可済みのRust、Foundry、Vitest runnerだけを使用し、各consumerが登録sectionを読みproduction symbolを呼ぶことと、各selectorが正確に1件成功したことを確認する。
このvector照合は列挙されたcaseに対するbounded conformanceであり、Rust、Solidity、TypeScript実装全体の完全なsemantic refinementではない。

Withdrawalの検証対象は、Base上の不可逆な`Committed` burnとCanister上の未決済債務である。Base refund、release acknowledgement、Withdrawal用EVM operationはモデルに存在しない。

- Foundryはfee driftのburn前revert、固定quote、atomic burn、処理済みDeposit IDのreplay拒否を検査し、ABI snapshotはWithdrawal専用のrefund/remint selectorが存在しないことを検査する。
- settlement、deposit admission、reservation、fee recipient rotation、fee payout、hold resolution、lease outcome、manual claimのtyped decisionはCargoとVerusが同じ実行関数本体を使用し、Verusが結果variant、全delta fieldと境界拒否を直接検査する。manifest上の`shared`義務はCargo式とVerus specで式macroを共有するpredicate proofであり、実行関数全体のproofとは呼ばない。
- `bridge-core/src/kernel.rs`はさらにsnapshot refresh owner、reserve observation token、settlement lease generation、canonical probe block一致、Withdrawal・reconciliation holdの派生index分類をproductionと共有し、Verusで各predicateを検査する。
- LeanはBase supply減少とCanister債務発生、固定宛先への支払、1:1 backingに加え、frontendのFinalized成功・revert・retry判断とserialized queue更新を正式な抽象モデルとして定義する。
- Rust/integrationはcanonical Finalized照合、Ledger成功・Duplicate・BadFee・曖昧結果、純額Fee reserve、追加EVM transaction不在を検査する。

Solidity SMTはproduction共有predicateの性質であり、完全なdeployed contract proofではない。
frontend LeanモデルはTypeScript実装そのものの証明ではなく、生成vectorと純粋な判断関数との対応をテストで検査する。
Bridge SignerはEIP-712 Mint Authorizationへ署名する。侵害されたSignerは未処理Deposit IDへの有効なAuthorizationを作成できるため、epoch invalidation、pause、固定limit、mint windowが被害速度の境界となる。
EIP-1898 `requireCanonical`の正しさ、EVM rollbackとEIP-1153 transient storage lifetime、ABI decoder、Web Locks、browser storage、providerの`finalized`意味論、EVM RPC quorum、wallet、ICRC履歴の真正性、SQLite atomicityとSQL row selectionは外部仮定である。形式証明の対象は、decode後のblock一致、enumから派生indexへの分類、成功したbrowser storage更新後のqueue状態までである。
Verus/Rust/LLVM、Lean kernel、Solidity SMTChecker、Wasm compilerはtrusted computing baseであり、source-level proofをWasm binary verificationとは呼ばない。
Ledger Fee超過はruntime guardでrelease前に停止し、Base withdrawal pauseとfee同期後に同じrecordを再検証する。

Leanの`step`は`Safe next`による事後フィルタを持たない。`raw_step_preserves_safe`が受理された各生遷移について安全性を直接証明し、有限trace定理はそのlemmaから帰納する。canonical・Ledger certificateは対象identityを含むが、その履歴やRPC情報の真正性は外部仮定である。

本番未デプロイのためschema v29再オープンとwire v25を検証する。migration、compatibility shim、dual-read、fallbackは提供せず、旧schemaと未知schemaはfail closedにする。

## Release proof gate

自己申告のproof attestationは使用しない。
固定production driverは不可逆操作の直前に、manifestへ束縛されたclean checkout内の`scripts/ci-local.sh proofs`を直接実行する。
実行前後のHEAD、archive tree hash、worktree、submodule revisionが一致しない場合、またはproofが失敗した場合はreleaseを中止する。
`proof-attestation.json`がGate bundleに残っている場合も、obsolete artifactとして拒否する。
proof成功後は、同じclean revisionからBridge Canister WasmとBridge contract runtimeをofflineで二回buildし、二つのbuildとrelease manifestのSHA-256が完全一致しなければ不可逆操作へ進まない。

このgateはローカルの固定sourceとtoolchainを信頼境界とする再現性検査であり、第三者CI provenance、compiler correctness、sourceとbinaryのsemantic equivalenceは主張しない。
