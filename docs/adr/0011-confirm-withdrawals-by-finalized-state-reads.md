---
status: accepted
---

# Withdrawalをブラウザ通知とfinalized receiptで確定する

ブラウザは`createWithdrawal`のreceiptがfinalized head以下になった後、認証付き`notify_withdrawal`へtransaction hashを送る。Bridge canisterは通知されたreceipt、Bridge contract address、`WithdrawalCreated` event、finality、event内のIC ownerとcallerの一致を複数provider合意で独立検証する。定期的な`eth_getLogs` discovery fallbackは持たない。

## Considered Options

- 全block範囲を定期走査する案は、未処理Withdrawalがなくても継続的にHTTPS outcallとcyclesを消費するため不採用とする。
- `latest`または`safe`を基準にする案は、mintを誘発する読み取りでreorgを踏むことが二重発行と同義であるため不採用とする。
- 通知されたtransactionのfinalized receiptとeventを採用する。

## Consequences

- Withdrawal確定までの遅延としてL1 finalize（実用上10〜20分程度）を受け入れる。
- 通知キューは64件、caller別pending 4件、10分窓でcaller 4件・全体32件に制限し、timerごとに最大1件だけ検証する。
- ブラウザは未通知hashをlocal storageへ保持し、同じIC identityで再訪したときに再通知する。通知も再訪もないWithdrawalは自動発見されない。
- 2/3のRPC providerが同時に誤る可能性への残余信頼は、ADR 0005の外部仮定監査リストへ載せる。
