---
status: accepted
---

# 個別 transaction の確認と Canister の記録から履歴を表示する

UI の通常送信と履歴表示では Base イベントの範囲検索を行わない。送信後は保存した transaction hash の receipt を確認し、対象コントラクト、イベント、認可または呼出し引数との一致を検証する。mint の実行成功は `Success` と表示し、finality と Canister への記録は別の状態として扱う。withdrawal は Base 側の成功と IC への支払完了を分ける。通信失敗では確認済みの成功を消さず、canonicality の不一致や receipt の消失が確認された場合に再確認へ戻す。

`notify_deposit_mint` は deposit ID と transaction hash を受け取り、Canister が固定 RPC の quorum、runtime、確定済みチェックポイントに束縛した正準 receipt、認可の完全一致を独立検証する。既存の `MintReconciled` production kernel に証拠を渡して記録する。フロントの成功申告を証拠にはせず、通知は返金を開始しない。既存の返金要求に必要な期限、未処理状態、確定済み証拠の条件は変えない。

deposit 一覧は既存 API を使い、保存済み mint receipt の transaction hash、block number、log index を公開 view に含める。withdrawal 一覧は Base requester ごとの索引を使用し、通知者の Principal を利用者の識別には使わない。ページは20件を基本、100件を上限とし、通知受理時刻の降順と withdrawal ID の組で安定したカーソルを構成する。現行 withdrawal 記録は削除しないため history_truncated は false。将来の保持期間変更では本体・通知索引・利用者索引を同じ transaction で削除し、打切り情報も更新する。

v35 から未公開の v36 への移行で索引を追加する。SQLite MemoryId 120 と wire version 30 は維持する。既存通知の逆引き、既存 withdrawal の利用者別索引をそれぞれ100件以下の同期 transaction で構築し、各バッチのカーソルを永続化する。再構築中は IndexNotReady を返す。Canister 更新と索引の検証が済むまで新 UI は公開せず、稼働中 v35 の証拠制約を緩めない。

未通知取引は端末内の再開情報で追跡する。別端末では transaction hash を入力し、接続中の送受信者と receipt の内容を照合して復旧する。hash が不明な未通知取引の別端末からの自動発見は提供しない。共有インデクサーや外部 Explorer API は追加しない。

UI の RPC は検証済み runtime profile に指定する。Alchemy UI キーの許可 Origin は https://bridge.kinic.xyz とし、Canister の RPC キー・設定は変更しない。Origin 制限は公開ブラウザキーの利用制限であり、キーを秘密にする機能ではない。テスト用環境では本番キーを使用しない。

## 証拠と検証範囲

状態遷移の安全性は既存の authorization_binding、exact_mint_finalization、expiry_refund、refund_evidence_enforcement と withdrawal_finalization の production kernel に従う。通知の資源制限は既存の notification_quota_isolation を再利用する。UI の再開情報は pending_queue に結び付ける。新しい RPC receipt adapter と履歴索引の接続は回帰テストで検証するが、索引の全 SQL 実装を抽象定理だけで証明済みとは扱わない。RPC の canonicality、固定 provider の chain binding、暗号学的真正性、IC message atomicity、ブラウザ永続化の可用性は外部仮定として残る。

公開用UIは、immutableなv35 Gate Bとactivation後のhash-linked upgrade chainからv36終端を検証する。runtime profileに終端module/schema、upgrade chain digest、明示的なUI RPC設定digestを束縛し、配置直前にlive RuntimeBindingと完全一致させる。Alchemy URLはこの公開設定からのみ決定的に描画する。索引再構築中は`list_withdrawals`の失敗で公開ゲートを拒否する。既存v35 UIはCanister更新まで配信を継続し、v35に新UIを公開する互換経路は設けない。
