---
status: accepted
---

# Withdrawalをブラウザ通知と同期一回検証で取り込む

> WithdrawalのSafe確認、hash永続化、自動通知はADR 0018でsupersedeされた。現行実装と運用判断にはADR 0018だけを使用する。

> Confirmation確認の起動方法に関する判断はADR 0017でsupersedeされた。旧確認レベルはADR 0018によりSafeへ置換された。

ブラウザはHistoryの明示走査でSafeな`WithdrawalCreated` eventを発見した後、別のreceipt確認を行わず、認証付き`notify_withdrawal`へtransaction hashを送る。
Bridge canisterは同じupdate call内で、通知されたreceipt、Bridge contract address、`WithdrawalCreated` event、confirmation、event内のIC ownerとcallerの一致を複数provider合意で検証する。
検証に成功した場合はWithdrawal recordを保存して取込結果を返す。
定期的な`eth_getLogs` discovery fallbackは持たない。

## Considered Options

- 全block範囲を定期走査する案は、未処理Withdrawalがなくても継続的にHTTPS outcallとcyclesを消費するため不採用とする。
- 旧設計ではL1 settlement完了まで待つ案を選択していたが、ADR 0018でcanonical Safe blockへ変更した。
- 通知されたtransactionのreceipt、event、state、Bridge snapshotを同一Safe block hashへ束縛し、そのupdate call内で取り込む。

## Consequences

- SafeはL1 settlement完了よりreorg耐性が弱く、finality前reorgを受容リスクとして扱う。
- EVM RPCとLedger fee取得は自動再試行せず、失敗理由を`notify_withdrawal`のcallerへ返す。
- caller別と全体の10分窓attempt quotaは、高価なEVM RPC callの前に適用する。
- ブラウザは未取込hashを永続化しない。Historyの明示`Refresh`または`Scan older`がSafe Bridge eventからhashを再構築する。
- 通知に失敗した利用者はHistoryを再度明示Refreshし、event行の`Check and notify`を操作する。
- 取込後のrelease、acknowledgement、refundはADR 0017に従い、EVM transaction送信後のconfirmation確認と次の正常段階をone-shot timerで進める。Walletやブラウザを閉じても継続し、停止理由が保存された場合だけHistoryまたは許可された管理者のRetryで再開する。
- 通知されないWithdrawalはCanisterから自動発見されず、Historyで明示的にeventを取得して通知するまでBase上でpendingのまま残る。
- 2/3のRPC providerが同時に誤る可能性への残余信頼は、ADR 0005の外部仮定監査リストへ載せる。
