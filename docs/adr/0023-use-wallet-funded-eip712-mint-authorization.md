---
status: accepted
---

# Base walletが送信するEIP-712 Mint Authorizationを使う

CanisterはDeposit Mint transactionを作成・送信せず、Finalized Base snapshotへ束縛したEIP-712 `MintAuthorization`へthreshold ECDSA署名する。任意のBase walletが`mintDepositWithAuthorization`を送り、そのwalletがgasを支払う。recipientは署名に固定する。

Authorizationは作成元Finalized Base timestampから固定2時間（7,200秒）の期限を持ち、同じDeposit IDへdigestやdeadlineを変えた再発行をしない。期限後、Canisterはcanonical Finalized blockで`isDepositProcessed`を検証する。未処理なら失効証拠を保存してLedger refundへ進み、処理済みならexact eventとcanonical receiptの証拠を保存して`Minted`へ進む。不一致時は返金せずfail closedする。

この決定により、Mint用ETH reserve、gas見積り、nonce、raw transaction、rebroadcast、replacement、wallet confirmation APIを削除する。Governance Operatorのtransaction laneだけは別に維持する。
