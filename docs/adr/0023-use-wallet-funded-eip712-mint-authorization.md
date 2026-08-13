---
status: accepted
---

# Base walletが送信するEIP-712 Mint Authorizationを使う

CanisterはDeposit Mint transactionを作成・送信せず、Finalized Base snapshotへ束縛したEIP-712 `MintAuthorization`へthreshold ECDSA署名する。任意のBase walletが`mintDepositWithAuthorization`を送り、そのwalletがgasを支払う。recipientは署名に固定する。

Authorizationは作成元Finalized Base timestampから固定2時間（7,200秒）の期限を持ち、同じDeposit IDへdigestやdeadlineを変えた再発行をしない。署名保存時にservice feeを一度だけ確定する。既存のFinalized snapshotが期限を超えたときは個別Base照合なしでmint予約を解放し、`RefundAvailable`にする。

この決定により、Mint用ETH reserve、gas見積り、nonce、raw transaction、rebroadcast、replacement、成功後のIC wallet確認署名を削除する。UIがBase receipt/eventをCanister Depositと統合して成功を表示する。Refundは`request_deposit_refund`でだけ起動し、任意の非anonymous Principalが進行できるが、宛先・金額・transfer identityはDeposit recordに固定する。認可発行済みならcanonical Finalized blockで`isDepositProcessed`を検証する。未処理だけを返金し、処理済みはexact event/receiptを保存して`Minted`へ進め、不一致時は資金を動かさずfail closedする。自動Base照合と自動Ledger refundは持たない。
