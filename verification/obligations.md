# Bridge検証義務表

優先順位は資産安全性、凍結済みBase ABI、ADR、production Rust、テストの順とする。

| 主張 | 根拠 | 検証 | 境界 |
|---|---|---|---|
| DepositはMinted、Cancelled、MintRevertedへ排他的に終端し、Cancelled IDを再利用しない。MintRevertedの予約は復旧upgradeまで保持する | 資産安全性、ADR 0006 | record遷移表、有限探索、stable retry test | stable mapのID一意性 |
| WithdrawalはReleased、Refunded、AcknowledgeReverted、RefundRevertedへ排他的に終端する | Base ABI、ADR 0003 | Rust遷移表、Foundry invariant、PocketIC revert test | Base transaction rollback |
| Release attemptは不存在証明後だけ増加し、経済payloadを保存する | ADR 0006 | production kernel Verus、Rust negative test | ledger履歴の真正性 |
| 不完全scan・別request・別transfer・証拠欠落でHoldを解除しない | 資産安全性 | production kernel Verus、negative fixture、coordinator test | archive/index応答の完全性 |
| 同一retryは冪等、異なるpayloadはconflict | 資産安全性 | production kernel Verus、全state table | hash衝突耐性 |
| Deposit Service Feeは受付時に固定し、Mint確定時に一度だけ会計へ加算する | ADR 0004 | Foundry fee変更test、Rust accounting test、PocketIC | Bridge signerのcalldata真正性、IC message rollback |
| BaseのPer-Deposit Limit、Mint Throughput Limit、window長はdeploy後に変更できない | ADR 0009 | Solidity immutable、ABI snapshot、Foundry authorization test | constructorへ渡すprofile値の妥当性 |
| EVM operationはQueued(0)→Prepared(1)→Submitted(2)→Finalized/Reverted(3)で単調 | Phase 3仕様 | production kernel Verus、遷移表、Rust exhaustive test | Base finality |
| Base safe観測はprimary EVM rankを進めず、取り消し可能なsidecarに限定する | safe確認レイヤー | Rust storage/coordinator test、PicJS safe regression test | provider合意、Base safe head |
| pending/open counterは状態分類差分と一致する | query契約 | production kernel Verus、stable counter test | stable write rollback |
| Mint受付はfinalized window消費量、未確定予約量、新規net量をchecked加算しlimit以下に限定する | 資産安全性 | production kernel Verus、Rust境界test、PocketIC | finalized Base snapshotの真正性 |
| reserve必要量は非終端Withdrawal数に対して単調でoverflow時は拒否する | Plan 003 | production kernel Verus、Rust境界test | 残高・費用入力の真正性 |
| ETHとcyclesの両reserveを独立に満たすまでDepositを受理しない | Plan 003 | production kernel Verus、PicJS | provider合意、cycles API |
| Settlement intentをMintより先に選び、同rankは最小operation IDとする | Plan 003 | production kernel Verus、storage test、PicJS | stable map read |
| Preparedがある間はnonceを新規割当せず、incrementはoverflow拒否する | Plan 003 | production kernel Verus、coordinator reopen test | stable write rollback |
| fee payoutはledger feeとpending debitを含め、成功・Duplicateの初回だけ減算する | Plan 003 | production kernel Verus、ledger adapter test、PicJS | ledger応答・履歴の真正性 |
| pause、finance、Governanceの許可集合は交差roleを含めaction単位で限定する | Plan 003 | production kernel Verus、全組合せRust test | Principal比較 |
| audit sequenceはappend時だけchecked incrementする | Plan 003 | production kernel Verus、storage test | stable insert/write rollback |

IC/EVM/Ledger応答、stable memoryとasync callの原子性はproof対象外であり、adapter、reopen、PocketICテストでrefinementを検証する。
