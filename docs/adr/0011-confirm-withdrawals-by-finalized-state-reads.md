---
status: accepted
---

# Withdrawalをブラウザ通知と同期一回検証で取り込む

> Withdrawalのsafe確定、hash永続化、自動通知はADR 0018でsupersedeされた。本書は旧設計の歴史記録である。

> Confirmation確認の起動方法に関する判断はADR 0017でsupersedeされた。複数RPCによるfinalized state検証は維持する。

ブラウザはHistoryの明示走査でfinalizedな`WithdrawalCreated` eventを発見した後、別のreceipt確認を行わず、認証付き`notify_withdrawal`へtransaction hashを送る。
Bridge canisterは同じupdate call内で、通知されたreceipt、Bridge contract address、`WithdrawalCreated` event、confirmation、event内のIC ownerとcallerの一致を複数provider合意で検証する。
検証に成功した場合はWithdrawal recordを保存して取込結果を返す。
定期的な`eth_getLogs` discovery fallbackは持たない。

## Considered Options

- 全block範囲を定期走査する案は、未処理Withdrawalがなくても継続的にHTTPS outcallとcyclesを消費するため不採用とする。
- `latest`または`safe`を基準にする案は、mintを誘発する読み取りでreorgを踏むことが二重発行と同義であるため不採用とする。
- 通知されたtransactionのfinalized receiptとeventを一回だけ検証し、そのupdate call内で取り込む案を採用する。

## Consequences

- Withdrawal確定までの遅延としてL1 finalize（実用上10〜20分程度）を受け入れる。
- EVM RPCとLedger fee取得は自動再試行せず、失敗理由を`notify_withdrawal`のcallerへ返す。
- caller別と全体の10分窓attempt quotaは、高価なEVM RPC callの前に適用する。
- ブラウザは未取込hashを永続化しない。Historyの明示`Refresh`または`Scan older`がfinalized Bridge eventからhashを再構築する。
- 通知に失敗した利用者はHistoryを再度明示Refreshし、event行の`Check and notify`を操作する。
- 取込後のrelease、acknowledgement、refundはADR 0017に従い、EVM transaction送信後のconfirmation確認と次の正常段階をone-shot timerで進める。Walletやブラウザを閉じても継続し、停止理由が保存された場合だけHistoryまたは許可された管理者のRetryで再開する。
- 通知されないWithdrawalはCanisterから自動発見されず、Historyで明示的にeventを取得して通知するまでBase上でpendingのまま残る。
- 2/3のRPC providerが同時に誤る可能性への残余信頼は、ADR 0005の外部仮定監査リストへ載せる。
