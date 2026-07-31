# KINIC–Base Bridge

KINICトークンをICPとBaseの間で移動し、両チェーンにまたがる裏付け債務を管理する文脈。

本BridgeはKINIC専用である。以下の`SNSトークン`や`bSNS`は設計上の総称として使い、実際の対象はKINIC、Base上のERC-20 metadataは`name = "kinic"`、`symbol = "KINIC"`とする。

## Language

**Deposit**:
ICPでSNSトークンをロックし、Service Feeを引いた量のbSNSをBaseでmintする1件の要求。
_Avoid_: Bridge transaction, transfer

**Escrowed Unquoted**:
DepositのLedger pullは確定したが、Finalized Base状態に対するquoteとmint予約をまだ確定していない状態。RPC一時障害や観測不一致ではこの状態を維持し、0値quoteを保存しない。
_Avoid_: Zero quote, failed deposit, reserved mint

**Deposit Refund**:
Ledger pull後のfreshなBase検証でpause、fee、limit、reserveの拒否が確定したDepositについて、元のIC accountへgross amountから固定Ledger feeを引いた額を返す補償transfer。Service Feeは確定せず、返金額とLedger feeの合計をgross amountに固定する。
_Avoid_: Service Fee refund, Base refund, arbitrary payout

**Withdrawal**:
Baseでbridged tokenをburnし、ICPでSNSトークンをreleaseする1件の要求。Base refundは提供しない。
_Avoid_: Return transfer, redeem transaction

**Bridge Exposure**:
Baseの発行済みbridged tokenと、burn済みだがICP releaseが未確定のWithdrawalの合計。Bridgeが裏付ける債務総額を表す。
_Avoid_: Outstanding supply, moved amount

**Per-Deposit Limit**:
1件のDepositとして受理できる最大量。累積移動量やBridge Exposureの上限ではない。
_Avoid_: Supply cap, bridge cap

**Mint Throughput Limit**:
一定時間にdeposit mintできる量を制御する流量上限。全量移動を禁止せず、短時間の被害拡大を抑える。
_Avoid_: Daily supply cap, outstanding cap

**Bridgeable SNS Token**:
ユーザーが移動可能なledger accountに保有する、neuronへstakeされていないSNSトークン。
_Avoid_: Total supply, staked token

**bSNS**:
Bridgeable SNS Tokenを1:1で裏付けるBase上のERC-20。SNS Governanceの投票権やneuron権限を持たない。
これは内部の総称であり、ERC-20 metadataへ`b` prefixを付けることを意味しない。KINIC用deployのtoken nameは`kinic`、symbolは`KINIC`とする。
_Avoid_: Cross-chain governance token, voting token

**Withdrawal Settlement**:
WithdrawalのICP ReleaseがLedger成功または履歴照合で確定し、`Paid`になった状態。Base refundは存在せず、`Committed`後のBase stateは終端である。
_Avoid_: Withdrawal completion, payout status

**Service Fee**:
成功したDepositまたはWithdrawalごとにSNSトークンで徴収する固定手数料。送金額に比例せず、デプロイ時に固定した上限を超えない。
_Avoid_: Gas fee, percentage fee, spread

**Fee Recipient**:
確定済みService Feeを受け取る現在のSNS ledger account。管理者が変更すると、未送金fee reserve全体の送付先も新accountへ変わる。
_Avoid_: Treasury owner, escrow owner

**Settlement Reserve**:
単一のBridge資源残高のうち、既存Withdrawal Settlementの完了を優先するため論理的に予約した部分。
_Avoid_: Settlement wallet, separate treasury

**Asset Safety**:
Bridgeが曖昧な外部結果を成功扱いせず、二重mint・二重release・裏付けを超えるfee支出を防ぐ性質。形式証明とテストは明示されたモデルおよび外部仮定の範囲だけを保証する。
_Avoid_: Guaranteed recovery, guaranteed availability

**Settlement Liveness**:
受理済みのDepositまたはCommitted Withdrawalが最終状態へ進める性質。RPC、Ledger、threshold signing、cycles、wallet同意、運用補充に依存し、本Bridgeはeventual completionを保証しない。
_Avoid_: Asset Safety, automatic recovery

**Reconciliation Hold**:
外部transferの成否を確定できず、二重処理を避けるため補償操作を禁止した状態。時間経過だけでは解除しない。
_Avoid_: Timed out, failed, retryable

**Refund Reconciliation Hold**:
Deposit RefundのLedger結果を確定できず、二重返金を避けるため同一refund identityと証拠探索を保持する状態。成功証拠でRefundedへ進み、完全な不存在証明後だけ経済payloadを維持した新attemptを作る。
_Avoid_: Refund retry, timed-out refund, manual refund

**Deposit Cancellation**:
DepositのICP pullが完全な履歴scanにより存在しないと証明された後、そのDeposit IDを再利用不能にする終端結果。token移動やService Fee確定を伴わない。
_Avoid_: Retry, timeout, reopen

**Withdrawal Transfer Attempt**:
1件のWithdrawalに対する特定のICRC release identity。不存在が完全に証明された場合だけ番号を増やし、経済的payloadを維持した新identityへ更新できる。
_Avoid_: Withdrawal retry, replacement withdrawal

**Ledger History Watermark**:
Reconciliationで完全性を確認したinclusiveなledger block index。最初の未走査blockがledger tipを超え、archiveとindexを含むexact transfer探索が完了した場合だけ不存在証拠になる。
_Avoid_: Timestamp, timeout, last attempted block

**Upgrade Authority**:
Bridge canisterのコード更新を承認するSNS Governance。SNS Rootが唯一のcontrollerとして採択済みupgradeを実行する。
_Avoid_: Runtime administrator, developer controller

**Governance Principal**:
Upgrade Authorityと同じSNS Governance principal。Canister上の管理操作と、closed Base管理操作を送信するGovernance Operator laneを制御する。
_Avoid_: developer controller, human EVM wallet

**Pause Principal**:
IC/Base双方のpause、記録済みpending Timelock operationのcancel、許可されたSettlementの進行だけを行う単一IC principal。再開、role rotation、fee管理、upgradeを行わない。
_Avoid_: Governance Principal, canister controller

**Governance Operator**:
Bridge CanisterがMint Signerとは別pathから導出し、Base pause、Service Fee、Timelock propose/cancel/executeだけを送信するthreshold address。
_Avoid_: human wallet, Mint Signer, canister controller

**Bridge Signer**:
Bridge canisterのthreshold ECDSAで管理され、BaseのDeposit mintだけを実行する単一address。Withdrawalのburnは利用者が実行する。
_Avoid_: Base Admin, Runtime Administrator, owner

**Base Admin Timelock**:
Governance Operatorだけをproposer/executor/cancellerに持ち、自己adminと24時間minimum delayを維持するOpenZeppelin Timelock。人間walletへroleを付与しない。
_Avoid_: human wallet, external DEFAULT_ADMIN_ROLE holder
