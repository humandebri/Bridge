# Bridge canister状態機械

## 境界

`bridge-core`は時刻、caller、IC API、Candid、stable structures、外部通信へ依存しない。
adapterがCandid `Nat`とBase `uint256`を`Amount(u128)`へchecked変換し、finalizedなBase stateから得たfee・limit・mint windowをevent入力として渡す。
Phase 2の公開Candidは`get_bridge_status` queryだけであり、資産を動かすupdate methodは存在しない。

## 遷移

- Deposit: `PullPending → Escrowed → MintPending → Minted`
- Depositの不明ledger結果: `PullPending → ReconciliationHold`
- Withdrawal release: `Observed → ReleasePending → ReleaseTransferred → AcknowledgePending → Released`
- Withdrawal refund: `Observed → RefundPending → Refunded`
- Withdrawalの不明ledger結果: `ReleasePending → ReconciliationHold`
- EVM操作: `Prepared → Submitted → Finalized`

同一eventのretryは現在のstateとpayloadが一致するときだけ冪等になる。
同一IDの受付payload hashが異なるretryはconflictとして拒否する。
`Released`と`Refunded`はterminalで相互遷移せず、Release開始後はRefund経路へ入れない。
Service FeeはDeposit mint確定時またはICP Release成功時だけ会計へ加算する。
Reconciliation Holdはrequest種別・request ID・transfer identity・hold IDが一致する証拠付きresolutionだけを受理する。
成功証拠はledger block index、不存在証拠は完全履歴確認済みwatermarkを必須とし、request recordとHold recordを一体で遷移・保存する。
Holdから直接transfer、refund、補償へ遷移しない。
Settlement ReserveはETH weiとcyclesの`u128`予算をcomponent-wiseに検査する。

## Stable schema v1

| Memory ID | 内容 |
|---:|---|
| 0 | schema version cell |
| 1 | accounting cell |
| 2 | Deposit map |
| 3 | Withdrawal map |
| 4 | EVM operation map |
| 5 | Reconciliation Hold map |
| 6 | counters cell |
| 7–15 | 予約済み、再利用禁止 |

各valueは先頭1 byteのwire versionとCBOR payloadからなる最大16 KiBのbounded blobである。
未知version、decode失敗、超過サイズ、未知schema versionはerrorまたはtrapとしてfail closedに扱い、default stateへ置換しない。
stateはstable structuresへrecord単位で直接保存し、`pre_upgrade`で一括serializeしない。
pending EVM操作とopen Holdの件数はmemory ID 6へchecked差分更新し、`get_bridge_status`は履歴mapを走査しない。
