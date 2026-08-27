# Bridge verification boundary

Lean projectはcross-chain protocolの正式な抽象仕様である。
状態遷移、不変条件、frontendの判断、pending queueの更新を`verification/lean/BridgeSpec`へ集約し、Lakeで定理を検査する。
Lean executableが生成する`verification/generated/protocol-vectors.json`をRust、Solidity、TypeScriptのconsumerで読み、実装の代表的な境界値を同じ期待値と照合する。
release対象claimは`Claims.lean`、有限幅モデルは`FiniteWidthModel.lean`、抽象モデルとの対応は`ModelRefinement.lean`、統合状態traceとcertificateは`Protocol.lean`へ分離する。`ModelRefinement`はproduction refinementではなく、生成vectorを介したbounded conformanceのモデル側根拠である。
`verification/claims.tsv` schema 6はprotocolとMint Authorizationのclaim、`assurance_target`（`release-safety` / `model-support`）、`required_strength`、Lean定理、Verus義務、SMT/Halmos obligation ID、明示的なimplementation basis、production symbol、transaction test、vector section、外部仮定を一つの型付きmanifestで管理する。現在の37 claimについてtargetと最低強度の19/18区分はchecker側の固定policyとも完全一致させ、manifestだけによる監査対象外への降格や最低強度の交換を拒否する。SMTとHalmos義務はすべて`supporting`であり、単独ではclaim全体を`implementation-proved`へ昇格させない。implementation strengthは、Lean contractが証明済みであり、production-bound Verus evidenceの必須集合が`implementation_basis`と完全一致し、全Verus義務がproduction-bound、または同じclaimのproduction-boundな`executable`／`shared-expression`だけに依存する`derived`として被覆される場合だけ`implementation-proved`となる。production symbolとtransaction testだけなら`production-linked`、抽象証明だけなら`abstract-proved`である。`model`、未結合義務、未解決derivedが一つでもあれば昇格させない。`scripts/check_claim_manifest.py`はschema 7の`verification/output/claim-report.json`を算出し、`release-safety`がLean contract、必要強度、production symbol、transaction testを欠く場合は`release-blocked`としてgateを失敗させる。外部仮定はstatusと混同せず独立表示する。
`verification/proof-impact.tsv`は安全関連production sourceをclaimと必須proof stageへ対応付ける。watched root内のRustまたはSolidity sourceが未登録、claimのproduction sourceにownerがない、またはproof receiptのsource fingerprintが現在のsource・proof資材と異なる場合、`scripts/check_proof_impact.py`はfail closedにする。fingerprintはproduction source、proof consumerとtest（PicJS integrationを含む）、driver、toolchain・build・test設定、lockfileを保守的に含み、生成output、build cache、`.venv`だけを除外する。schema v7 receiptはproof開始時のfingerprintを固定し、必須stageの開始前・終了後、claim evidence生成前後、最終検証で同じ値を要求する。各stageが同じfingerprintで順序どおり一度ずつ`pass`し、38件すべてがrelease-ready、release-blockedとmodel-supportが0件、証拠強度がimplementation-proved 20件とproduction-linked 18件、conditional-livenessが5件の場合だけcompleteとして受理する。`halmos-and-negative`は固定lock環境でpositiveを全件証明し、各mutantが正確に一つの反例を返すことを要求する。`claim-transaction-tests` stageは`verification/claim-test-manifest.tsv`へ登録された各testが正確に1件成功したことを検査する。定理がそのまま適用できる変更に無意味なproof file差分は要求せず、現在のsourceに対する全stageの再実行を要求する。

receipt summaryはclaim総数に加え、`release-ready`、`release-blocked`、`model-support`、`conditional-liveness`、および`implementation-proved`、`production-linked`、`abstract-proved`の件数を保持する。旧receipt schemaは受理しない。

変更されたMint Authorization経路の安全claimは同manifest内でAuthorization binding、expiry refund、exact mint finalization、epoch invalidation、reservation lifecycle、fee一回性、deposit backing、manual claim exclusionへ分割する。暗号known-answerは`verification/known-answer-manifest.tsv`で別管理し、Lean生成policy証拠へ昇格させない。
CIはこれらの全Lean theorem、Verus manifest、claim台帳、全vector section、全consumerを完全一致検査する。Verus proof名は一意であり、非executable proofは登録specを当該`ensures`から参照し、executable proofはnamed returnを`ensures`で拘束して登録kernel callを最終戻り式として返す。production call siteはRust production root内へ限定する。Solidity proof linkはcompiler ASTのcontract・完全なoverload signatureへ一意に解決し、SMT passからproduction kernelへの直接callとBridge wrapperからpost-auth commit境界への直接callを`referencedDeclaration`で検査する。Bridge wrapperは`digest`を`_mintAuthorizationDigest(authorization)`から生成し、同じ`digest`と`signature`で署名回復し、`effects`を宣言IDまで固定した全18フィールドの`MintTransitionInput`を受け取る唯一の`evaluateMint` callから生成し、両変数を再代入せず、commitへ`authorization`、`digest`、`effects`の順序と宣言IDを保って渡す。
refinement manifestはsection、抽象定義、有限幅モデル定義、model refinement定理、runnerの5列で構成し、同じsectionに複数consumerを登録できる。consumer source、test selector、production呼出しはrunnerごとの明示的rendererが決定的に生成し、生成物はGit管理する。
CIは許可済みのRust、Foundry、Vitest runnerだけを使用し、manifestとrendererの完全被覆、生成物のdrift、非生成testによるselector重複を拒否したうえで、各selectorが正確に1件成功したことを確認する。production linkはownership情報であり、文字列の存在だけでは証明強度を上げない。
このvector照合は列挙されたcaseに対するbounded conformanceであり、Rust、Solidity、TypeScript実装全体の完全なsemantic refinementではない。

Withdrawalの検証対象は、Base上の不可逆な`Committed` burnとCanister上の未決済債務である。Base refund、release acknowledgement、Withdrawal用EVM operationはモデルに存在しない。

条件付きliveness 5件はrelease claim catalogに含めない。Lean定理は補助定理として保持し、完全修飾定理名、命題型、強いfairness・外部解決・action admissibility仮定をchecker側の固定policyと完全一致させる。proof gateは各定理を期待する命題型として型検査し、axiom dependencyをclaim witnessと同じallowlistで検査する。production未結合境界は`conditional-liveness.tsv`および`conditional-liveness.md`へ記録する。

- Foundryはfee driftのburn前revert、固定quote、atomic burn、処理済みDeposit IDのreplay拒否を検査し、ABI snapshotはWithdrawal専用のrefund/remint selectorが存在しないことを検査する。
- settlement、deposit admission、deposit identity preflight、reservation、fee recipient rotation、fee payout、hold resolution、lease outcome、manual claimのtyped decisionはCargoとVerusが同じ実行関数本体を使用し、Verusが結果variant、全delta fieldと境界拒否を直接検査する。manifest上の`shared-expression`義務は、単一armの登録macroをCargo式とVerus specが正確に一度ずつ呼び、直接parameterの位置、引数数、整数定数aliasが一致するpredicate proofである。productionの派生式はspecの任意入力へ対応するpredicate入力境界であり、実行関数全体のproofとは呼ばない。`derived`は別記述のspecまたは複数kernelの合成proofであり、単独ではproduction実装証明へ昇格しない。`model`はproduction結合を持たない。
- `bridge-core/src/kernel.rs`はさらにsnapshot refresh owner、reserve observation token、settlement lease generation、canonical probe block一致、Withdrawal・reconciliation holdの派生index分類をproductionと共有し、Verusで各predicateを検査する。
- LeanはBase supply減少とCanister債務発生、固定宛先への支払、1:1 backingに加え、frontendのFinalized成功・revert・retry判断とserialized queue更新を正式な抽象モデルとして定義する。
- Rust/integrationはcanonical Finalized照合、Ledger成功・Duplicate・BadFee・曖昧結果、純額Fee reserve、追加EVM transaction不在を検査する。

Solidity SMTはproduction共有predicateの性質、Halmosは署名認証後のcommit境界におけるstate・token・rollbackの性質であり、いずれもpublic wrapperまたは完全なdeployed contract proofではない。
frontend LeanモデルはTypeScript実装そのものの証明ではなく、生成vectorと純粋な判断関数との対応をテストで検査する。
Bridge SignerはEIP-712 Mint Authorizationへ署名する。侵害されたSignerは未処理Deposit IDへの有効なAuthorizationを作成できるため、epoch invalidation、pause、固定limit、mint windowが被害速度の境界となる。
EIP-1898 `requireCanonical`の正しさ、EVM rollbackとEIP-1153 transient storage lifetime、ABI decoder、Web Locks、browser storage、providerの`finalized`意味論、provider応答の真正性、wallet、ICRC履歴の真正性、SQLite atomicityとSQL row selectionは外部仮定である。decode済み`(block number, block hash)`からexact 2-of-3 identityを選ぶpredicateはproduction-shared kernelとVerusが同じ式を使用する。形式証明の対象はこの選択、decode後のblock一致、enumから派生indexへの分類、成功したbrowser storage更新後のqueue状態までである。
Bridge runtimeの不変性は外部仮定である。保存済み観測をwarm attestationとして再利用できる条件はproduction-shared predicateとVerusで検査し、cold成功後の永続化、経路間再利用、upgrade/reinstall境界はRustとPocketIC transaction testで検査する。
Verus/Rust/LLVM、Lean kernel、Solidity SMTChecker、Halmos/Z3、Wasm compilerはtrusted computing baseであり、source-level proofをWasm binary verificationとは呼ばない。
固定100,000 rawのLedger Feeがcharged Service Feeを超える場合は固定fee guardでrelease前に停止し、Base withdrawal pauseと設定確認後に同じrecordを再検証する。Ledger Feeの不変性は外部仮定であり、runtime settlementは`icrc1_fee()`を照会しない。

Leanの`step`は`Safe next`による事後フィルタを持たない。`raw_step_preserves_safe`が受理された各生遷移について安全性を直接証明し、有限trace定理はそのlemmaから帰納する。canonical・Ledger certificateは対象identityを含むが、その履歴やRPC情報の真正性は外部仮定である。

schema v34再オープンとwire v29をRust transaction testとsame-Wasm PocketIC testで検証する。Productionは旧schemaをfail closedにし、test-deploymentはreview済みstaging v33／wire28 migrationだけを例外として検証する。

## Production-equivalence definition

本番相当は「本番実装全体が形式検証済み」を意味しない。claimごとに`abstract-proved`、`production-linked`、`implementation-proved`を区別し、外部仮定とTCBを別欄で開示する。TCBの正しさは証明の外側の仮定であり、証明で除去できない。

本番相当とは以下を満たすことと定義する:

1. release proof gateがclean checkoutから直接passする（後述）。
2. 各`release-safety` claimがmanifestで要求した証拠強度を満たし、`release-blocked`が0件である。
3. 外部仮定は`verification/assumptions.tsv`へ登録され、依存claim・fault test・運用監視・fail-closed動作が明記されている。

## Release proof gate

自己申告のproof attestationは使用しない。
固定production driverは不可逆操作の直前に、manifestへ束縛されたclean checkout内の`scripts/ci-local.sh proofs`を直接実行する。
実行前後のHEAD、archive tree hash、worktree、submodule revisionが一致しない場合、またはproofが失敗した場合はreleaseを中止する。
`proof-attestation.json`がGate bundleに残っている場合も、obsolete artifactとして拒否する。
proof成功後は、同じclean revisionからBridge Canister WasmとBridge contract runtimeをofflineで二回buildし、二つのbuildとrelease manifestのSHA-256が完全一致しなければ不可逆操作へ進まない。

このgateはローカルの固定sourceとtoolchainを信頼境界とする再現性検査であり、第三者CI provenance、compiler correctness、sourceとbinaryのsemantic equivalenceは主張しない。

`verification/output/proof-receipt.json`はproof gateの生成成果物であり、git追跡しない。receiptのsource fingerprintは実行開始時のworking treeを固定するため、追跡されたソースに一致させる方式ではなく、release gate内で同一fingerprintのまま全stageが`pass`し`complete: true`になることを`ci-local.sh proofs`自身が強制する。receiptはgitignoreされているため、コミット内容とreceiptのfingerprintの一致は期待しない。gate bundleへは`proof-attestation.json`を含めず、receipt自体もGate bundleへ同梱しない。fingerprint不一致や未完了stage、非pass stageを含むreceiptはfail closedで再実行を要求する。
