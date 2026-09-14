# Claim statements and specification correspondence

Leanが解釈した命題型・主要定義。証明本体は出力しない。仕様との意味的一致や独立カーネル検査の証拠ではない。

Toolchain: `leanprover/lean4:v4.30.0`

## claim: activation_preflight

仕様: `docs/canister-state-machine.md`

前提: ControlPlane到達状態、非pause、activationCount>0

結論: 最後のactivationはvalidated

未証明境界: 初期activationCount=0は除外。外部観測の正しさは前提

証拠・外部仮定: `claims.tsv:activation_preflight` / `claims.tsv:activation_preflight`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.activation_preflight_witness : BridgeSpec.ClaimContracts.ActivationPreflight
```

主要定義: `BridgeSpec.ClaimContracts.ActivationPreflight`

## claim: authorization_binding

仕様: `docs/adr/0023-use-wallet-funded-eip712-mint-authorization.md`

前提: authorization commitまたは署名installが受理

結論: domain・epoch・IC起点900秒期限を束縛し、署名時にu64範囲と残り300秒以上

未証明境界: 署名の暗号学的真正性・IC時刻とNatの対応は外部仮定

証拠・外部仮定: `claims.tsv:authorization_binding` / `claims.tsv:authorization_binding`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.authorization_binding_witness : BridgeSpec.ClaimContracts.AuthorizationBinding
```

主要定義: `BridgeSpec.ClaimContracts.AuthorizationBinding`, `BridgeSpec.MintAuthorization.installSignature`, `BridgeSpec.signatureTimeAllowed`, `BridgeSpec.MintAuthorization.Authorization.valid`

## claim: automatic_retry_limit

仕様: `docs/canister-state-machine.md`

前提: laneと失敗回数・上限を入力

結論: automatic laneかつfailures<limitのときだけ許可

未証明境界: 失敗回数の永続化と停止後のuser再開は別証拠

証拠・外部仮定: `claims.tsv:automatic_retry_limit` / `claims.tsv:automatic_retry_limit`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.automatic_retry_limit_witness : BridgeSpec.ClaimContracts.AutomaticRetryLimit
```

主要定義: `BridgeSpec.ClaimContracts.AutomaticRetryLimit`

## claim: canonical_probe

仕様: `docs/bridge-flow.md`

前提: receiptとsnapshotのblock番号

結論: predicateは番号の一致と同値

未証明境界: 番号一致だけではcanonical hashの真正性は保証しない

証拠・外部仮定: `claims.tsv:canonical_probe` / `claims.tsv:canonical_probe`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.canonical_probe_witness : BridgeSpec.ClaimContracts.CanonicalProbe
```

主要定義: `BridgeSpec.ClaimContracts.CanonicalProbe`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: committed_quote

仕様: `docs/canister-state-machine.md`

前提: commit受理、初期Safeと受理されたtrace

結論: 純額は正、gross=純額+fee、終端宛先・純額は初期保存quoteと一致

未証明境界: 本番からLean traceへの完全な写像は未証明

証拠・外部仮定: `claims.tsv:committed_quote` / `claims.tsv:committed_quote`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.committed_quote_witness : BridgeSpec.ClaimContracts.CommittedQuote
```

主要定義: `BridgeSpec.ClaimContracts.CommittedQuote`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: confirmed_activation_evidence_binding

仕様: `docs/canister-state-machine.md`

前提: activationのmatch個数・generation・signedAt、upgrade完了hookのcallerと時刻を入力

結論: activationは単一matchとmetadata厳密一致。SNS更新はRootの完了hookが採択・移譲後かつ未来でない

未証明境界: IC callbackとcaller・時刻の真正性、同時upgradeがないこと、SNSとIC実行完了は外部仮定

証拠・外部仮定: `claims.tsv:confirmed_activation_evidence_binding` / `claims.tsv:confirmed_activation_evidence_binding`

レビュー理由: 同一Wasmのproposal受理とpost_upgrade完了を区別する判定を追加。実SNSの失敗と完了を別途検証する。

```lean
BridgeSpec.ClaimContracts.confirmed_activation_evidence_binding_witness : BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding
```

主要定義: `BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding`

## claim: cycles_top_up_request_policy

仕様: `docs/canister-state-machine.md`

前提: balance・threshold・inProgress・authorizedを入力

結論: 認可済み・非実行中・balance≤thresholdのときだけ要求可能

未証明境界: launcherの補充実行や将来残高回復は未証明

証拠・外部仮定: `claims.tsv:cycles_top_up_request_policy` / `claims.tsv:cycles_top_up_request_policy`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.cycles_top_up_request_policy_witness : BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy
```

主要定義: `BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy`

## claim: deposit_admission

仕様: `docs/canister-state-machine.md`

前提: admitDepositが純額を返す

結論: fee・正の純額・一件上限・window上限を満たす

未証明境界: 局所モデルは予約量を独立入力に持たない。本番の予約込み受付は別Verus義務

証拠・外部仮定: `claims.tsv:deposit_admission` / `claims.tsv:deposit_admission`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.deposit_admission_witness : BridgeSpec.ClaimContracts.DepositAdmission
```

主要定義: `BridgeSpec.ClaimContracts.DepositAdmission`

## claim: deposit_backing

仕様: `docs/canister-state-machine.md`

前提: 初期会計不変条件と受理されたsignature・mint・refund履歴

結論: backingを保存し、各操作で規定会計差分を適用

未証明境界: 会計モデルのphaseは実外部操作を証明しない

証拠・外部仮定: `claims.tsv:deposit_backing` / `claims.tsv:deposit_backing`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.deposit_backing_witness : BridgeSpec.ClaimContracts.DepositBacking
```

主要定義: `BridgeSpec.ClaimContracts.DepositBacking`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: deposit_identity_preflight

仕様: `docs/canister-state-machine.md`

前提: IdentityHistory到達状態で候補preflightが受理

結論: 候補IDは処理済みではない

未証明境界: RPCの真正性とinstall instanceの一意性は外部境界

証拠・外部仮定: `claims.tsv:deposit_identity_preflight` / `claims.tsv:deposit_identity_preflight`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.deposit_identity_preflight_witness : BridgeSpec.ClaimContracts.DepositIdentityPreflight
```

主要定義: `BridgeSpec.ClaimContracts.DepositIdentityPreflight`

## claim: epoch_invalidation

仕様: `docs/canister-state-machine.md`

前提: 既存authorizationまたはretiredSigner≠replacementSigner

結論: 認可の再発行を拒否し、旧signerは未来epochでも拒否

未証明境界: 署名回復・epoch更新のEVM実行は別証拠

証拠・外部仮定: `claims.tsv:epoch_invalidation` / `claims.tsv:epoch_invalidation`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.epoch_invalidation_witness : BridgeSpec.ClaimContracts.EpochInvalidation
```

主要定義: `BridgeSpec.ClaimContracts.EpochInvalidation`

## claim: exact_mint_finalization

仕様: `docs/canister-state-machine.md`

前提: completeMintが受理

結論: 成功receiptはfinalized以下で、deposit・recipient・digestがauthorization一致

未証明境界: hashやRPC digestの非0は真正性証明ではない。暗号とcanonicalityは外部仮定

証拠・外部仮定: `claims.tsv:exact_mint_finalization` / `claims.tsv:exact_mint_finalization`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.exact_mint_finalization_witness : BridgeSpec.ClaimContracts.ExactMintFinalization
```

主要定義: `BridgeSpec.ClaimContracts.ExactMintFinalization`, `BridgeSpec.MintAuthorization.MintEvidence.valid`

## claim: expiry_refund

仕様: `docs/canister-state-machine.md`

前提: startExpiredRefundが受理

結論: 未処理・deposit/digest一致・期限を厳密に超過、会計backingを保存

未証明境界: Finalized証拠の真正性と実Ledger転送は外部境界

証拠・外部仮定: `claims.tsv:expiry_refund` / `claims.tsv:expiry_refund`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.expiry_refund_witness : BridgeSpec.ClaimContracts.ExpiryRefund
```

主要定義: `BridgeSpec.ClaimContracts.ExpiryRefund`, `BridgeSpec.MintAuthorization.ExpiryEvidence.valid`

## claim: fee_accounting_once

仕様: `docs/canister-state-machine.md`

前提: 初期Depositからの受理traceと署名install

結論: fee credit回数は高々1で、署名時のfee差分はauthorizationの値

未証明境界: GlobalHistoryとDepositHistoryを本番履歴へ接続する完全証明はない

証拠・外部仮定: `claims.tsv:fee_accounting_once` / `claims.tsv:fee_accounting_once`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.fee_accounting_once_witness : BridgeSpec.ClaimContracts.FeeAccountingOnce
```

主要定義: `BridgeSpec.ClaimContracts.FeeAccountingOnce`, `BridgeSpec.MintAuthorization.installSignature`, `BridgeSpec.signatureTimeAllowed`, `BridgeSpec.MintAuthorization.Authorization.valid`

## claim: fee_payout

仕様: `docs/canister-state-machine.md`

前提: feePayoutAllowedが受理

結論: pendingと新規debitはreserve内、成功時だけdebitを計上

未証明境界: 外部転送結果の正しさと永続化は外部境界

証拠・外部仮定: `claims.tsv:fee_payout` / `claims.tsv:fee_payout`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.fee_payout_witness : BridgeSpec.ClaimContracts.FeePayout
```

主要定義: `BridgeSpec.ClaimContracts.FeePayout`

## claim: fee_recipient_rotation

仕様: `docs/canister-state-machine.md`

前提: rotateFeeRecipientが受理

結論: pending payoutは0、残高・既計上feeは保存しrecipientを更新

未証明境界: 本番認可とSQL commitは別証拠

証拠・外部仮定: `claims.tsv:fee_recipient_rotation` / `claims.tsv:fee_recipient_rotation`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.fee_recipient_rotation_witness : BridgeSpec.ClaimContracts.FeeRecipientRotation
```

主要定義: `BridgeSpec.ClaimContracts.FeeRecipientRotation`

## claim: funding_attempt_lifecycle

仕様: `docs/canister-state-machine.md`

前提: funding outcome enumを入力

結論: 成功・duplicate・曖昧・retryable・確定失敗を規定decisionへ分類

未証明境界: enumへの外部結果のdecodeとSQL transactionは別証拠

証拠・外部仮定: `claims.tsv:funding_attempt_lifecycle` / `claims.tsv:funding_attempt_lifecycle`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.funding_attempt_lifecycle_witness : BridgeSpec.ClaimContracts.FundingAttemptLifecycle
```

主要定義: `BridgeSpec.ClaimContracts.FundingAttemptLifecycle`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: funding_reconciliation_freshness

仕様: `docs/canister-state-machine.md`

前提: absence・finalScan・dedupExpiredのboolを入力

結論: fresh scanが必要な条件とrelease可能条件を区別

未証明境界: 完全履歴・経過時刻・scan結果の真正性は外部境界

証拠・外部仮定: `claims.tsv:funding_reconciliation_freshness` / `claims.tsv:funding_reconciliation_freshness`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.funding_reconciliation_freshness_witness : BridgeSpec.ClaimContracts.FundingReconciliationFreshness
```

主要定義: `BridgeSpec.ClaimContracts.FundingReconciliationFreshness`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: governance_confirmation_authorization

仕様: `docs/canister-state-machine.md`

前提: callerと現在のrelayer・governance・pauseをNatで表現

結論: 非0かつ許可された三者のいずれかだけ受理

未証明境界: Natの0とanonymous Principalの対応、controller取得は本番側境界

証拠・外部仮定: `claims.tsv:governance_confirmation_authorization` / `claims.tsv:governance_confirmation_authorization`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.governance_confirmation_authorization_witness : BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization
```

主要定義: `BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization`

## claim: governance_nonce_chain_binding

仕様: `docs/canister-state-machine.md`

前提: ControlPlane到達状態に最後のgovernance chainがある

結論: governance chainはconfigured chainと一致

未証明境界: RPC観測chainIdやprovider切替耐性は主張しない

証拠・外部仮定: `claims.tsv:governance_nonce_chain_binding` / `claims.tsv:governance_nonce_chain_binding`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.governance_nonce_chain_binding_witness : BridgeSpec.ClaimContracts.GovernanceNonceChainBinding
```

主要定義: `BridgeSpec.ClaimContracts.GovernanceNonceChainBinding`

## claim: governance_transaction_affordability

仕様: `docs/canister-state-machine.md`

前提: observedWei<requiredWei

結論: requiredWei以下の残高では支払可能条件を満たさない

未証明境界: 必要額見積りや外部残高照会は証明しない

証拠・外部仮定: `claims.tsv:governance_transaction_affordability` / `claims.tsv:governance_transaction_affordability`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.governance_transaction_affordability_witness : BridgeSpec.ClaimContracts.GovernanceTransactionAffordability
```

主要定義: `BridgeSpec.ClaimContracts.GovernanceTransactionAffordability`

## claim: hold_resolution

仕様: `docs/canister-state-machine.md`

前提: holdRetryAllowedが受理

結論: exact successまたはcomplete absenceが存在

未証明境界: 入力boolから証拠の真正性は導けない

証拠・外部仮定: `claims.tsv:hold_resolution` / `claims.tsv:hold_resolution`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.hold_resolution_witness : BridgeSpec.ClaimContracts.HoldResolution
```

主要定義: `BridgeSpec.ClaimContracts.HoldResolution`

## claim: initial_activation_authorization

仕様: `docs/canister-state-machine.md`

前提: bootstrap・governance・sealed・phase・migration分類入力

結論: bootstrap期間の認可と消費、seal caller条件、migration分類を規定

未証明境界: migration分類の定義展開だけでは実復元を証明しない

証拠・外部仮定: `claims.tsv:initial_activation_authorization` / `claims.tsv:initial_activation_authorization`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.initial_activation_authorization_witness : BridgeSpec.ClaimContracts.InitialActivationAuthorization
```

主要定義: `BridgeSpec.ClaimContracts.InitialActivationAuthorization`

## claim: lease_lane_isolation

仕様: `docs/canister-state-machine.md`

前提: lane claim decisionがallow

結論: 対象は非activeでlane capacity未満

未証明境界: lane入力の分類とruntime dispatcherは別証拠

証拠・外部仮定: `claims.tsv:lease_lane_isolation` / `claims.tsv:lease_lane_isolation`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.lease_lane_isolation_witness : BridgeSpec.ClaimContracts.LeaseLaneIsolation
```

主要定義: `BridgeSpec.ClaimContracts.LeaseLaneIsolation`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: lease_outcome

仕様: `docs/canister-state-machine.md`

前提: lease outcome predicateとGlobalHistory更新が受理

結論: leaseはactiveでgeneration一致、会計不変条件と他recordを保存

未証明境界: async実行・SQL行選択・callbackの外部効果は別証拠

証拠・外部仮定: `claims.tsv:lease_outcome` / `claims.tsv:lease_outcome`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.lease_outcome_witness : BridgeSpec.ClaimContracts.LeaseOutcome
```

主要定義: `BridgeSpec.ClaimContracts.LeaseOutcome`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: ledger_block_provenance

仕様: `docs/canister-state-machine.md`

前提: Ledger indexをinstallする受理trace

結論: 既存indexを保持し、競合を拒否し、refundにはfunding indexが必要

未証明境界: index値の履歴真正性とSQL行選択は外部仮定

証拠・外部仮定: `claims.tsv:ledger_block_provenance` / `claims.tsv:ledger_block_provenance`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.ledger_block_provenance_witness : BridgeSpec.ClaimContracts.LedgerBlockProvenance
```

主要定義: `BridgeSpec.ClaimContracts.LedgerBlockProvenance`

## claim: nonterminal_deposit_index_consistency

仕様: `docs/canister-state-machine.md`

前提: DepositPhaseを入力

結論: refunded・cancelled・minted以外だけindex対象

未証明境界: SQL index維持と実record走査は別証拠

証拠・外部仮定: `claims.tsv:nonterminal_deposit_index_consistency` / `claims.tsv:nonterminal_deposit_index_consistency`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.nonterminal_deposit_index_consistency_witness : BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency
```

主要定義: `BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency`

## claim: notification_quota_isolation

仕様: `docs/canister-state-machine.md`

前提: 通知受付・ingestion predicateが受理

結論: global・caller・ingestion上限未満、cooldownはhash一致かつ期限未満

未証明境界: 永続カウンタ・期間更新・caller真正性は別証拠

証拠・外部仮定: `claims.tsv:notification_quota_isolation` / `claims.tsv:notification_quota_isolation`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.notification_quota_isolation_witness : BridgeSpec.ClaimContracts.NotificationQuotaIsolation
```

主要定義: `BridgeSpec.ClaimContracts.NotificationQuotaIsolation`

## claim: operational_config_seal

仕様: `docs/canister-state-machine.md`

前提: sealedとcandidateValidの入力

結論: 未sealかつ候補有効の場合だけsealし、asset操作はsealedの場合だけ許可

未証明境界: 呼出し元が真偽入力を正しく作ることと永続化は別証拠

証拠・外部仮定: `claims.tsv:operational_config_seal` / `claims.tsv:operational_config_seal`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.operational_config_seal_witness : BridgeSpec.ClaimContracts.OperationalConfigSeal
```

主要定義: `BridgeSpec.ClaimContracts.OperationalConfigSeal`

## claim: paid_call_cycle_reserve

仕様: `docs/canister-state-machine.md`

前提: reserve+attachedCycles+margin≤liquid、実請求額≤予算

結論: 課金後もreserveを残す

未証明境界: ICの実課金・返却とruntime会計は外部境界

証拠・外部仮定: `claims.tsv:paid_call_cycle_reserve` / `claims.tsv:paid_call_cycle_reserve`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.paid_call_cycle_reserve_witness : BridgeSpec.ClaimContracts.PaidCallCycleReserve
```

主要定義: `BridgeSpec.ClaimContracts.PaidCallCycleReserve`

## claim: payment_identity

仕様: `docs/canister-state-machine.md`

前提: GlobalHistoryのpayoutまたはrecord更新が受理

結論: payoutの純額・宛先はrecordに一致し、別IDのrecordは不変

未証明境界: callbackのpaid phase自体は実送金の証拠ではない

証拠・外部仮定: `claims.tsv:payment_identity` / `claims.tsv:payment_identity`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.payment_identity_witness : BridgeSpec.ClaimContracts.PaymentIdentity
```

主要定義: `BridgeSpec.ClaimContracts.PaymentIdentity`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: pending_queue

仕様: `docs/bridge-flow.md`

前提: blockedな既存entryまたはstorage書込失敗

結論: blocked retryを保持し、書込失敗時はsessionを保持してdurable結果なし

未証明境界: Web Locks・browser storage原子性は外部仮定

証拠・外部仮定: `claims.tsv:pending_queue` / `claims.tsv:pending_queue`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.pending_queue_witness : BridgeSpec.ClaimContracts.PendingQueue
```

主要定義: `BridgeSpec.ClaimContracts.PendingQueue`

## claim: refund_evidence_enforcement

仕様: `docs/canister-state-machine.md`

前提: 初期Depositからの受理trace中に期限切れrefund開始がある

結論: 未処理・deposit/digest一致・strict expiryが必要

未証明境界: RPC canonicalityと履歴真正性は外部仮定

証拠・外部仮定: `claims.tsv:refund_evidence_enforcement` / `claims.tsv:refund_evidence_enforcement`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.refund_evidence_enforcement_witness : BridgeSpec.ClaimContracts.RefundEvidenceEnforcement
```

主要定義: `BridgeSpec.ClaimContracts.RefundEvidenceEnforcement`, `BridgeSpec.MintAuthorization.ExpiryEvidence.valid`

## claim: refund_request_authorization

仕様: `docs/canister-state-machine.md`

前提: 受理prefix後のrequestExpiredRefundが受理

結論: authenticated=trueかつdepositProcessed=false

未証明境界: callerは既存recordの宛先・金額を変更できないことは本番側証拠

証拠・外部仮定: `claims.tsv:refund_request_authorization` / `claims.tsv:refund_request_authorization`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.refund_request_authorization_witness : BridgeSpec.ClaimContracts.RefundRequestAuthorization
```

主要定義: `BridgeSpec.ClaimContracts.RefundRequestAuthorization`

## claim: reservation_commit

仕様: `docs/canister-state-machine.md`

前提: 有限幅境界内の予約計算と受理された予約解放

結論: 予約+候補の合計を保存し、解放は正確に0、二重解放を拒否

未証明境界: GlobalHistoryの解放イベント自体には時刻証拠がない

証拠・外部仮定: `claims.tsv:reservation_commit` / `claims.tsv:reservation_commit`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.reservation_commit_witness : BridgeSpec.ClaimContracts.ReservationCommit
```

主要定義: `BridgeSpec.ClaimContracts.ReservationCommit`

## claim: reservation_lifecycle

仕様: `docs/canister-state-machine.md`

前提: GlobalHistoryの予約解放イベントが受理

結論: 予約を正確に0へし、二重解放を拒否

未証明境界: 期限判断はDepositHistoryと本番側の別証拠

証拠・外部仮定: `claims.tsv:reservation_lifecycle` / `claims.tsv:reservation_lifecycle`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.reservation_lifecycle_witness : BridgeSpec.ClaimContracts.ReservationLifecycle
```

主要定義: `BridgeSpec.ClaimContracts.ReservationLifecycle`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: runtime_attestation_reuse

仕様: `docs/canister-state-machine.md`

前提: 到達ControlPlane状態に再利用domainがある

結論: 再利用domainは現在のinstall domainと一致

未証明境界: runtime不変性、warm観測の正しさ、保存と再利用経路は別証拠

証拠・外部仮定: `claims.tsv:runtime_attestation_reuse` / `claims.tsv:runtime_attestation_reuse`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.runtime_attestation_reuse_witness : BridgeSpec.ClaimContracts.RuntimeAttestationReuse
```

主要定義: `BridgeSpec.ClaimContracts.RuntimeAttestationReuse`

## claim: service_fee_maximum

仕様: `docs/canister-state-machine.md`

前提: fee上下限の局所predicateとDeposit署名履歴

結論: fee変更は固定範囲内、署名fee計上は一度だけ

未証明境界: pending payoutのtrace上限は別のreserve性質。全実行のfee設定写像は未証明

証拠・外部仮定: `claims.tsv:service_fee_maximum` / `claims.tsv:service_fee_maximum`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.service_fee_maximum_witness : BridgeSpec.ClaimContracts.ServiceFeeMaximum
```

主要定義: `BridgeSpec.ClaimContracts.ServiceFeeMaximum`, `BridgeSpec.Protocol.Safe`, `BridgeSpec.Protocol.filterSafeStoredState`

## claim: settlement_backing

仕様: `docs/canister-state-machine.md`

前提: 初期会計不変条件と受理された履歴・payout

結論: backingを保存し、escrow・feeReserve・未決済債務へ規定差分を適用

未証明境界: 実Ledger送金とSQLの原子性は外部境界

証拠・外部仮定: `claims.tsv:settlement_backing` / `claims.tsv:settlement_backing`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.settlement_backing_witness : BridgeSpec.ClaimContracts.SettlementBacking
```

主要定義: `BridgeSpec.ClaimContracts.SettlementBacking`, `BridgeSpec.GlobalHistory.AccountingInvariant`, `BridgeSpec.GlobalHistory.Backed`, `BridgeSpec.GlobalHistory.eventDelta`

## claim: signing_cycle_reserve

仕様: `docs/canister-state-machine.md`

前提: reserve+signingCost+margin≤liquid、実請求額≤見積り

結論: 請求後もreserveを残す

未証明境界: IC課金額と見積りの対応は外部仮定

証拠・外部仮定: `claims.tsv:signing_cycle_reserve` / `claims.tsv:signing_cycle_reserve`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.signing_cycle_reserve_witness : BridgeSpec.ClaimContracts.SigningCycleReserve
```

主要定義: `BridgeSpec.ClaimContracts.SigningCycleReserve`

## claim: withdrawal_admission_boundary

仕様: `docs/canister-state-machine.md`

前提: withdrawalIdAdmissibleが受理

結論: minimumは非0でobservedはminimum以上

未証明境界: Natと32byte big-endian IDの対応は共有predicateとテスト

証拠・外部仮定: `claims.tsv:withdrawal_admission_boundary` / `claims.tsv:withdrawal_admission_boundary`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.withdrawal_admission_boundary_witness : BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary
```

主要定義: `BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary`

## claim: withdrawal_finality_quorum

仕様: `docs/bridge-flow.md`

前提: 三providerのheadまたはidentityからcheckpointを選択

結論: 二者が高さをattestし、identity版は高さとhashの二者一致

未証明境界: providerの正しいchain設定と応答真正性は外部仮定

証拠・外部仮定: `claims.tsv:withdrawal_finality_quorum` / `claims.tsv:withdrawal_finality_quorum`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.withdrawal_finality_quorum_witness : BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum
```

主要定義: `BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum`

## claim: withdrawal_finalization

仕様: `docs/bridge-flow.md`

前提: receipt成功・canonical・blockを入力

結論: notifyには成功かつfinalized以下かつcanonicalが必要、finalized不在ならretry

未証明境界: provider応答とブラウザ実装全体は証明しない

証拠・外部仮定: `claims.tsv:withdrawal_finalization` / `claims.tsv:withdrawal_finalization`

レビュー理由: 初回対応表。命題・依存定義・外部境界を分離して登録。

```lean
BridgeSpec.ClaimContracts.withdrawal_finalization_witness : BridgeSpec.ClaimContracts.WithdrawalFinalization
```

主要定義: `BridgeSpec.ClaimContracts.WithdrawalFinalization`

## liveness: deposit_terminal_progress_lemmas

仕様: `verification/conditional-liveness.md`

前提: 対象終端操作の継続的admissibility、weak fairness、readyAt以降の外部可用性、登録されたuser/keeper操作

結論: mintの条件付き含意とrefundの条件付き含意の論理積。共通実行の二者択一ではない

未証明境界: 本番schedulerや資金受領からのadmissibility導出は未証明。release claimに含めない

証拠・外部仮定: `conditional-liveness.tsv:deposit_terminal_progress_lemmas` / `conditional-liveness.tsv:deposit_terminal_progress_lemmas`

レビュー理由: 初回対応表。条件付き補題と本番の到達性を区別。

```lean
BridgeSpec.Liveness.deposit_terminal_progress_lemmas : BridgeSpec.Liveness.DepositTerminalProgressLemmas
```

主要定義: `BridgeSpec.Liveness.DepositTerminalProgressLemmas`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: expired_deposit_eventually_refunded

仕様: `verification/conditional-liveness.md`

前提: 対象終端操作の継続的admissibility、weak fairness、readyAt以降の外部可用性、登録されたuser/keeper操作

結論: 対象depositがrefundedへ到達

未証明境界: 本番schedulerや資金受領からのadmissibility導出は未証明。release claimに含めない

証拠・外部仮定: `conditional-liveness.tsv:expired_deposit_eventually_refunded` / `conditional-liveness.tsv:expired_deposit_eventually_refunded`

レビュー理由: 初回対応表。条件付き補題と本番の到達性を区別。

```lean
BridgeSpec.Liveness.expired_deposit_eventually_refunded : BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded
```

主要定義: `BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: funded_deposit_eventually_minted

仕様: `verification/conditional-liveness.md`

前提: 対象終端操作の継続的admissibility、weak fairness、readyAt以降の外部可用性、登録されたuser/keeper操作

結論: 対象depositがmintedへ到達

未証明境界: 本番schedulerや資金受領からのadmissibility導出は未証明。release claimに含めない

証拠・外部仮定: `conditional-liveness.tsv:funded_deposit_eventually_minted` / `conditional-liveness.tsv:funded_deposit_eventually_minted`

レビュー理由: 初回対応表。条件付き補題と本番の到達性を区別。

```lean
BridgeSpec.Liveness.funded_deposit_eventually_minted : BridgeSpec.Liveness.FundedDepositEventuallyMinted
```

主要定義: `BridgeSpec.Liveness.FundedDepositEventuallyMinted`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: funding_failure_eventually_cancelled

仕様: `verification/conditional-liveness.md`

前提: 対象終端操作の継続的admissibility、weak fairness、readyAt以降の外部可用性、登録されたuser/keeper操作

結論: 対象depositがcancelledへ到達

未証明境界: 本番schedulerや資金受領からのadmissibility導出は未証明。release claimに含めない

証拠・外部仮定: `conditional-liveness.tsv:funding_failure_eventually_cancelled` / `conditional-liveness.tsv:funding_failure_eventually_cancelled`

レビュー理由: 初回対応表。条件付き補題と本番の到達性を区別。

```lean
BridgeSpec.Liveness.funding_failure_eventually_cancelled : BridgeSpec.Liveness.FundingFailureEventuallyCancelled
```

主要定義: `BridgeSpec.Liveness.FundingFailureEventuallyCancelled`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## liveness: withdrawal_eventually_paid

仕様: `verification/conditional-liveness.md`

前提: 対象終端操作の継続的admissibility、weak fairness、readyAt以降の外部可用性、登録されたuser/keeper操作

結論: 対象withdrawalがpaidへ到達

未証明境界: 本番schedulerや資金受領からのadmissibility導出は未証明。release claimに含めない

証拠・外部仮定: `conditional-liveness.tsv:withdrawal_eventually_paid` / `conditional-liveness.tsv:withdrawal_eventually_paid`

レビュー理由: 初回対応表。条件付き補題と本番の到達性を区別。

```lean
BridgeSpec.Liveness.committed_withdrawal_eventually_paid : BridgeSpec.Liveness.WithdrawalEventuallyPaid
```

主要定義: `BridgeSpec.Liveness.WithdrawalEventuallyPaid`, `BridgeSpec.Liveness.AdmissibleUntilOccurs`, `BridgeSpec.Liveness.CommonOperationalAssumptions`, `BridgeSpec.Liveness.WeakFair`

## Shared definitions

### BridgeSpec.ClaimContracts.ActivationPreflight

specification: 最後のactivationはvalidated

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ActivationPreflight : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    Eq.{1} (BridgeSpec.ControlPlane.State.paused state) Bool.false →
      GT.gt.{0} (BridgeSpec.ControlPlane.State.activationCount state) 0 →
        Eq.{1} (BridgeSpec.ControlPlane.State.lastActivationValidated state) Bool.true
```

### BridgeSpec.ClaimContracts.AuthorizationBinding

specification: domain・epoch・IC起点900秒期限を束縛し、署名時にu64範囲と残り300秒以上

仕様: `docs/adr/0023-use-wallet-funded-eip712-mint-authorization.md`

```lean
BridgeSpec.ClaimContracts.AuthorizationBinding : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState}
    {authorization : BridgeSpec.MintAuthorization.Authorization}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin},
    Eq.{1} (BridgeSpec.MintAuthorization.commitAuthorization state authorization origin) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization next) (Option.some.{0} authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin)
              BridgeSpec.MintAuthorization.authorizationTtl))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.chainId authorization)
              (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedChainId origin))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedVerifyingContract origin))
              (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.epoch authorization)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedEpoch origin))))))
  (And
    (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {observedTimestamp : Nat},
      Eq.{1} (BridgeSpec.MintAuthorization.installSignature state observedTimestamp) (Option.some.{0} next) →
        Exists.{1} fun authorization =>
          And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
            (And
              (LE.le.{0} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
                BridgeSpec.MintAuthorization.maxU64)
              (And (LE.le.{0} observedTimestamp (HSub.hSub.{0, 0, 0} BridgeSpec.MintAuthorization.maxU64 300))
                (LE.le.{0} (HAdd.hAdd.{0, 0, 0} observedTimestamp 300)
                  (BridgeSpec.MintAuthorization.Authorization.deadline authorization)))))
    BridgeSpec.ClaimContracts.DepositTransitionSafety)
```

### BridgeSpec.ClaimContracts.AutomaticRetryLimit

specification: automatic laneかつfailures<limitのときだけ許可

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.AutomaticRetryLimit : Prop
∀ (automaticLane : Bool) (failures limit : Nat),
  Iff (Eq.{1} (BridgeSpec.ClaimContracts.automaticRetryAllowed automaticLane failures limit) Bool.true)
    (And (Eq.{1} automaticLane Bool.true) (LT.lt.{0} failures limit))
```

### BridgeSpec.ClaimContracts.CanonicalProbe

specification: predicateは番号の一致と同値

仕様: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.CanonicalProbe : Prop
And
  (∀ (receiptBlock snapshotBlock : Nat),
    Iff (Eq.{1} (BridgeSpec.canonicalProbeMatches receiptBlock snapshotBlock) Bool.true)
      (Eq.{1} receiptBlock snapshotBlock))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.CommittedQuote

specification: 純額は正、gross=純額+fee、終端宛先・純額は初期保存quoteと一致

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.CommittedQuote : Prop
And
  (∀ {amount serviceFee : Nat} {destination : BridgeSpec.Account} {withdrawal : BridgeSpec.Withdrawal},
    Eq.{1} (BridgeSpec.commit amount serviceFee destination) (Option.some.{0} withdrawal) →
      BridgeSpec.QuoteValid withdrawal)
  (And
    (∀ {state final : BridgeSpec.Protocol.ProtocolState} {events : List.{0} BridgeSpec.Protocol.ProtocolEvent},
      BridgeSpec.Protocol.Safe state →
        BridgeSpec.Protocol.Runs state events final →
          And
            (Eq.{1} (BridgeSpec.Withdrawal.destination (BridgeSpec.Protocol.ProtocolState.withdrawal final))
              (BridgeSpec.Protocol.ProtocolState.committedDestination state))
            (Eq.{1} (BridgeSpec.Withdrawal.amountOut (BridgeSpec.Protocol.ProtocolState.withdrawal final))
              (BridgeSpec.Protocol.ProtocolState.committedAmountOut state)))
    BridgeSpec.ClaimContracts.IntegratedProtocolReachability)
```

### BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding

specification: activationの単一match・metadata厳密一致と、Rootによる採択・移譲後のupgrade完了hook

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ConfirmedActivationEvidenceBinding : Prop
And
  (∀ (foundMatch foundAdditionalMatch : Bool),
    Iff
      (Eq.{1} (BridgeSpec.ClaimContracts.confirmedActivationAttemptIsUnique foundMatch foundAdditionalMatch) Bool.true)
      (And (Eq.{1} foundMatch Bool.true) (Eq.{1} foundAdditionalMatch Bool.false)))
  (And
    (∀ (confirmedGeneration confirmedSignedAt artifactGeneration artifactSignedAt : Nat),
      Iff
        (Eq.{1}
          (BridgeSpec.ClaimContracts.confirmedActivationMetadataMatches confirmedGeneration confirmedSignedAt
            artifactGeneration artifactSignedAt)
          Bool.true)
        (And (Eq.{1} confirmedGeneration artifactGeneration) (Eq.{1} confirmedSignedAt artifactSignedAt)))
    (∀ (root : Bool) (completed decided handover now : Nat),
      Iff (Eq.{1} (BridgeSpec.ClaimContracts.snsUpgradeCompletionAllowed root completed decided handover now) Bool.true)
        (And (Eq.{1} root Bool.true)
          (And (GT.gt.{0} decided 0)
            (And (GE.ge.{0} completed decided) (And (GE.ge.{0} completed handover) (LE.le.{0} completed now)))))))
```

### BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy

specification: 認可済み・非実行中・balance≤thresholdのときだけ要求可能

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.CyclesTopUpRequestPolicy : Prop
∀ (balance threshold : Nat) (inProgress authorized : Bool),
  Iff
    (Eq.{1} (Bool.and (Bool.and authorized (Bool.not inProgress)) (Decidable.decide (LE.le.{0} balance threshold)))
      Bool.true)
    (And (Eq.{1} authorized Bool.true) (And (Eq.{1} inProgress Bool.false) (LE.le.{0} balance threshold)))
```

### BridgeSpec.ClaimContracts.DepositAdmission

specification: fee・正の純額・一件上限・window上限を満たす

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositAdmission : Prop
And
  (∀ {admission : BridgeSpec.DepositAdmission} {net : Nat},
    Eq.{1} (BridgeSpec.admitDeposit admission) (Option.some.{0} net) →
      And
        (LE.le.{0} (BridgeSpec.DepositAdmission.serviceFee admission)
          (BridgeSpec.DepositAdmission.maximumServiceFee admission))
        (And
          (LT.lt.{0} (BridgeSpec.DepositAdmission.serviceFee admission)
            (BridgeSpec.DepositAdmission.grossAmount admission))
          (And
            (Eq.{1} net
              (HSub.hSub.{0, 0, 0} (BridgeSpec.DepositAdmission.grossAmount admission)
                (BridgeSpec.DepositAdmission.serviceFee admission)))
            (And (GT.gt.{0} net 0)
              (And (LE.le.{0} net (BridgeSpec.DepositAdmission.perDepositLimit admission))
                (LE.le.{0} (HAdd.hAdd.{0, 0, 0} (BridgeSpec.DepositAdmission.mintedInWindow admission) net)
                  (BridgeSpec.DepositAdmission.mintWindowLimit admission)))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.DepositBacking

specification: backingを保存し、各操作で規定会計差分を適用

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositBacking : Prop
And
  (∀ {state final : BridgeSpec.GlobalHistory.GlobalState} {events : List.{0} BridgeSpec.GlobalHistory.Event},
    BridgeSpec.GlobalHistory.AccountingInvariant state →
      BridgeSpec.GlobalHistory.Runs state events final →
        BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting final))
  (And
    (∀ {record next : BridgeSpec.GlobalHistory.Record},
      Eq.{1}
          (BridgeSpec.GlobalHistory.applyRecord record
            (BridgeSpec.GlobalHistory.Event.installSignature (BridgeSpec.GlobalHistory.Record.id record)))
          (Option.some.{0} next) →
        And
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic next))
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic record))
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record)))
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
            (HSub.hSub.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
    (And
      (∀ {record next : BridgeSpec.GlobalHistory.Record},
        Eq.{1}
            (BridgeSpec.GlobalHistory.applyRecord record
              (BridgeSpec.GlobalHistory.Event.mint (BridgeSpec.GlobalHistory.Record.id record)))
            (Option.some.{0} next) →
          And
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.baseSupply (BridgeSpec.GlobalHistory.Record.economic next))
              (HAdd.hAdd.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.baseSupply (BridgeSpec.GlobalHistory.Record.economic record))
                (BridgeSpec.GlobalHistory.Record.netAmount record)))
            (Eq.{1}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
                (BridgeSpec.GlobalHistory.Record.netAmount record))))
      (∀ {record next : BridgeSpec.GlobalHistory.Record} {amount : Nat},
        Eq.{1}
            (BridgeSpec.GlobalHistory.applyRecord record
              (BridgeSpec.GlobalHistory.Event.refund (BridgeSpec.GlobalHistory.Record.id record) amount))
            (Option.some.{0} next) →
          And
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic record)) amount))
            (Eq.{1}
              (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic next))
              (HSub.hSub.{0, 0, 0}
                (BridgeSpec.GlobalHistory.Economic.unmintedLiability (BridgeSpec.GlobalHistory.Record.economic record))
                amount)))))
```

### BridgeSpec.ClaimContracts.DepositIdentityPreflight

specification: 候補IDは処理済みではない

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.DepositIdentityPreflight : Prop
∀ {state next : BridgeSpec.IdentityHistory.State} {candidate : Nat},
  BridgeSpec.IdentityHistory.Reachable state →
    Eq.{1} (BridgeSpec.IdentityHistory.preflight state candidate) (Option.some.{0} next) →
      Eq.{1} (state candidate) Bool.false
```

### BridgeSpec.ClaimContracts.EpochInvalidation

specification: 認可の再発行を拒否し、旧signerは未来epochでも拒否

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.EpochInvalidation : Prop
And
  (∀ {state : BridgeSpec.MintAuthorization.DepositState}
    {current replacement : BridgeSpec.MintAuthorization.Authorization}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin},
    Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} current) →
      Eq.{1} (BridgeSpec.MintAuthorization.commitAuthorization state replacement origin) Option.none.{0})
  (And
    (∀ {authorizationEpoch currentEpoch retiredSigner replacementSigner : Nat},
      Ne.{1} retiredSigner replacementSigner →
        Eq.{1}
          (BridgeSpec.MintAuthorization.evmMintAuthorizationAccepted authorizationEpoch
            (HAdd.hAdd.{0, 0, 0} currentEpoch 1) retiredSigner replacementSigner)
          Bool.false)
    BridgeSpec.ClaimContracts.AuthorizationBinding)
```

### BridgeSpec.ClaimContracts.ExactMintFinalization

specification: 成功receiptはfinalized以下で、deposit・recipient・digestがauthorization一致

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ExactMintFinalization : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {evidence : BridgeSpec.MintAuthorization.MintEvidence},
    Eq.{1} (BridgeSpec.MintAuthorization.completeMint state evidence) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptSucceeded evidence) Bool.true)
        (And
          (LE.le.{0} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlock evidence)
            (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlock evidence))
          (Exists.{1} fun authorization =>
            And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.depositId evidence)
                  (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.recipient evidence)
                    (BridgeSpec.MintAuthorization.Authorization.recipient authorization))
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.authorizationDigest evidence)
                    (BridgeSpec.MintAuthorization.Authorization.digest authorization)))))))
  BridgeSpec.ClaimContracts.DepositBacking
```

### BridgeSpec.ClaimContracts.ExpiryRefund

specification: 未処理・deposit/digest一致・期限を厳密に超過、会計backingを保存

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ExpiryRefund : Prop
And
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState}
    {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin}
    {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
    Eq.{1} (BridgeSpec.MintAuthorization.startExpiredRefund state origin evidence) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
        (Exists.{1} fun authorization =>
          And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
                (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
                  (BridgeSpec.MintAuthorization.Authorization.digest authorization))
                (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
                  (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))))
  BridgeSpec.ClaimContracts.DepositBacking
```

### BridgeSpec.ClaimContracts.FeeAccountingOnce

specification: fee credit回数は高々1で、署名時のfee差分はauthorizationの値

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeeAccountingOnce : Prop
And
  (∀ {next : BridgeSpec.Protocol.Deposit.State} {events : List.{0} BridgeSpec.Protocol.Deposit.Event},
    Eq.{1} (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial events) (Option.some.{0} next) →
      LE.le.{0} (BridgeSpec.Protocol.Deposit.traceFeeCreditCount events) 1)
  (∀ {state next : BridgeSpec.MintAuthorization.DepositState} {observedTimestamp : Nat},
    Eq.{1} (BridgeSpec.MintAuthorization.installSignature state observedTimestamp) (Option.some.{0} next) →
      Exists.{1} fun authorization =>
        And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.authorization state) (Option.some.{0} authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeReserve next)
              (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.feeReserve state)
                (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization)))
            (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeCounted next) Bool.true)))
```

### BridgeSpec.ClaimContracts.FeePayout

specification: pendingと新規debitはreserve内、成功時だけdebitを計上

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeePayout : Prop
And
  (∀ {reserve pending amount fee : Nat},
    Eq.{1} (BridgeSpec.feePayoutAllowed reserve pending amount fee) Bool.true →
      And (LE.le.{0} pending reserve)
        (And (LE.le.{0} (HAdd.hAdd.{0, 0, 0} amount fee) (HSub.hSub.{0, 0, 0} reserve pending))
          (And (Eq.{1} (BridgeSpec.payoutDebit Bool.false amount fee) 0)
            (Eq.{1} (BridgeSpec.payoutDebit Bool.true amount fee) (HAdd.hAdd.{0, 0, 0} amount fee)))))
  BridgeSpec.ClaimContracts.SettlementBacking
```

### BridgeSpec.ClaimContracts.FeeRecipientRotation

specification: pending payoutは0、残高・既計上feeは保存しrecipientを更新

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FeeRecipientRotation : Prop
And
  (∀ {state next : BridgeSpec.FeeState} {recipient : Nat},
    Eq.{1} (BridgeSpec.rotateFeeRecipient state recipient) (Option.some.{0} next) →
      And (Eq.{1} (BridgeSpec.FeeState.pendingPayout state) 0)
        (And (Eq.{1} (BridgeSpec.FeeState.reserve next) (BridgeSpec.FeeState.reserve state))
          (And (Eq.{1} (BridgeSpec.FeeState.confirmedDepositFees next) (BridgeSpec.FeeState.confirmedDepositFees state))
            (And
              (Eq.{1} (BridgeSpec.FeeState.confirmedWithdrawalFees next)
                (BridgeSpec.FeeState.confirmedWithdrawalFees state))
              (And (Eq.{1} (BridgeSpec.FeeState.pendingPayout next) 0)
                (Eq.{1} (BridgeSpec.FeeState.recipient next) recipient))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.FundingAttemptLifecycle

specification: 成功・duplicate・曖昧・retryable・確定失敗を規定decisionへ分類

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FundingAttemptLifecycle : Prop
And
  (And
    (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.definitiveFailure)
      BridgeSpec.FundingAttemptDecision.release)
    (And
      (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.success)
        BridgeSpec.FundingAttemptDecision.promoteSuccess)
      (And
        (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.duplicate)
          BridgeSpec.FundingAttemptDecision.promoteSuccess)
        (And
          (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.ambiguous)
            BridgeSpec.FundingAttemptDecision.promoteAmbiguous)
          (Eq.{1} (BridgeSpec.decideFundingAttempt BridgeSpec.FundingOutcomeKind.retryableFailure)
            BridgeSpec.FundingAttemptDecision.retain)))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.FundingReconciliationFreshness

specification: fresh scanが必要な条件とrelease可能条件を区別

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.FundingReconciliationFreshness : Prop
And
  (And
    (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.false Bool.false)
      BridgeSpec.FundingReconciliationDecision.wait)
    (And
      (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.false Bool.true)
        BridgeSpec.FundingReconciliationDecision.wait)
      (And
        (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.true Bool.false)
          BridgeSpec.FundingReconciliationDecision.wait)
        (And
          (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.false Bool.true Bool.true)
            BridgeSpec.FundingReconciliationDecision.wait)
          (And
            (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.false Bool.false)
              BridgeSpec.FundingReconciliationDecision.restartFresh)
            (And
              (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.false Bool.true)
                BridgeSpec.FundingReconciliationDecision.restartFresh)
              (And
                (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.true Bool.false)
                  BridgeSpec.FundingReconciliationDecision.wait)
                (Eq.{1} (BridgeSpec.decideFundingReconciliation Bool.true Bool.true Bool.true)
                  BridgeSpec.FundingReconciliationDecision.release))))))))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization

specification: 非0かつ許可された三者のいずれかだけ受理

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceConfirmationAuthorization : Prop
∀ (caller currentConfirmationRelayer currentGovernance currentPause : Nat),
  Iff
    (Eq.{1}
      (BridgeSpec.ClaimContracts.confirmationCallerAuthorized caller currentConfirmationRelayer currentGovernance
        currentPause)
      Bool.true)
    (And (Ne.{1} caller 0)
      (Or (Eq.{1} caller currentConfirmationRelayer)
        (Or (Eq.{1} caller currentGovernance) (Eq.{1} caller currentPause))))
```

### BridgeSpec.ClaimContracts.GovernanceNonceChainBinding

specification: governance chainはconfigured chainと一致

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceNonceChainBinding : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    ∀ (chainId : Nat),
      Eq.{1} (BridgeSpec.ControlPlane.State.lastGovernanceChain state) (Option.some.{0} chainId) →
        Eq.{1} chainId (BridgeSpec.ControlPlane.State.configuredChainId state)
```

### BridgeSpec.ClaimContracts.GovernanceTransactionAffordability

specification: requiredWei以下の残高では支払可能条件を満たさない

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.GovernanceTransactionAffordability : Prop
∀ (observedWei requiredWei : Nat), LT.lt.{0} observedWei requiredWei → Not (LE.le.{0} requiredWei observedWei)
```

### BridgeSpec.ClaimContracts.HoldResolution

specification: exact successまたはcomplete absenceが存在

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.HoldResolution : Prop
And
  (∀ {success absence : Bool},
    Eq.{1} (BridgeSpec.holdRetryAllowed success absence) Bool.true →
      Or (Eq.{1} success Bool.true) (Eq.{1} absence Bool.true))
  BridgeSpec.ClaimContracts.IntegratedProtocolReachability
```

### BridgeSpec.ClaimContracts.InitialActivationAuthorization

specification: bootstrap期間の認可と消費、seal caller条件、migration分類を規定

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.InitialActivationAuthorization : Prop
And
  (∀ (bootstrapController governance sealed bootstrapActive validPhase : Bool),
    Iff
      (Eq.{1}
        (BridgeSpec.ClaimContracts.initialActivationAuthorized bootstrapController governance sealed bootstrapActive
          validPhase)
        Bool.true)
      (And (Eq.{1} validPhase Bool.true)
        (And (Eq.{1} sealed Bool.true)
          (ite.{1} (Eq.{1} bootstrapActive Bool.true) (Eq.{1} bootstrapController Bool.true)
            (Eq.{1} governance Bool.true)))))
  (And
    (∀ (authorityPresent confirmedExecute : Bool),
      Iff
        (Eq.{1}
          (BridgeSpec.ClaimContracts.bootstrapActivationAuthorityAfterTransition authorityPresent confirmedExecute)
          Bool.true)
        (And (Eq.{1} authorityPresent Bool.true) (Eq.{1} confirmedExecute Bool.false)))
    (And
      (∀ (controller bootstrap : Bool),
        Iff (Eq.{1} (BridgeSpec.ClaimContracts.operationalConfigSealCallerAuthorized controller bootstrap) Bool.true)
          (And (Eq.{1} controller Bool.true) (Eq.{1} bootstrap Bool.true)))
      (∀ (sealed paused pauseIsOld pauseIsNew markerUnbound markerIsNew rolesDistinct : Bool),
        Eq.{1}
          (BridgeSpec.ClaimContracts.bootstrapPausePrincipalMigrationCode sealed paused pauseIsOld pauseIsNew
            markerUnbound markerIsNew rolesDistinct)
          (ite.{1} (Eq.{1} sealed Bool.true) 2
            (ite.{1} (Eq.{1} (Bool.and pauseIsNew markerIsNew) Bool.true) 1
              (ite.{1} (Eq.{1} (Bool.and (Bool.and (Bool.and paused pauseIsNew) markerUnbound) rolesDistinct) Bool.true)
                4
                (ite.{1}
                  (Eq.{1} (Bool.and (Bool.and (Bool.and paused pauseIsOld) markerUnbound) rolesDistinct) Bool.true) 0
                  3)))))))
```

### BridgeSpec.ClaimContracts.LeaseLaneIsolation

specification: 対象は非activeでlane capacity未満

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LeaseLaneIsolation : Prop
And
  (∀ {targetActive targetAutomatic : Bool} {activeInLane capacity : Nat},
    Eq.{1} (BridgeSpec.decideLeaseLaneClaim targetActive targetAutomatic activeInLane capacity)
        BridgeSpec.LeaseLaneClaimDecision.allow →
      And (Eq.{1} targetActive Bool.false) (LT.lt.{0} activeInLane capacity))
  BridgeSpec.ClaimContracts.GlobalInterleavingSafety
```

### BridgeSpec.ClaimContracts.LeaseOutcome

specification: leaseはactiveでgeneration一致、会計不変条件と他recordを保存

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LeaseOutcome : Prop
And
  (∀ {active : Bool} {currentGeneration outcomeGeneration : Nat},
    Eq.{1} (BridgeSpec.leaseOutcomeCurrent active currentGeneration outcomeGeneration) Bool.true →
      And (Eq.{1} active Bool.true) (Eq.{1} currentGeneration outcomeGeneration))
  BridgeSpec.ClaimContracts.GlobalInterleavingSafety
```

### BridgeSpec.ClaimContracts.LedgerBlockProvenance

specification: 既存indexを保持し、競合を拒否し、refundにはfunding indexが必要

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.LedgerBlockProvenance : Prop
BridgeSpec.LedgerBlockProvenance.ClaimContract
```

### BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency

specification: refunded・cancelled・minted以外だけindex対象

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.NonterminalDepositIndexConsistency : Prop
∀ (phase : BridgeSpec.MintAuthorization.DepositPhase),
  Iff (Eq.{1} (BridgeSpec.MintAuthorization.nonterminalDepositIndexed phase) Bool.true)
    (And (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.refunded)
      (And (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.cancelled)
        (Ne.{1} phase BridgeSpec.MintAuthorization.DepositPhase.minted)))
```

### BridgeSpec.ClaimContracts.NotificationQuotaIsolation

specification: global・caller・ingestion上限未満、cooldownはhash一致かつ期限未満

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.NotificationQuotaIsolation : Prop
And
  (∀ {globalCount callerCount globalLimit callerLimit ingestionCount ingestionLimit : Nat},
    Eq.{1} (BridgeSpec.notificationAdmissionAllowed globalCount callerCount globalLimit callerLimit) Bool.true →
      Eq.{1} (BridgeSpec.notificationIngestionAllowed ingestionCount ingestionLimit) Bool.true →
        And (LT.lt.{0} globalCount globalLimit)
          (And (LT.lt.{0} callerCount callerLimit) (LT.lt.{0} ingestionCount ingestionLimit)))
  (And
    (∀ {hashMatches : Bool} {nowNs retryAfterNs : Nat},
      Eq.{1} (BridgeSpec.notificationFailureCooldownActive hashMatches nowNs retryAfterNs) Bool.true →
        And (Eq.{1} hashMatches Bool.true) (LT.lt.{0} nowNs retryAfterNs))
    BridgeSpec.ClaimContracts.IntegratedProtocolReachability)
```

### BridgeSpec.ClaimContracts.OperationalConfigSeal

specification: 未sealかつ候補有効の場合だけsealし、asset操作はsealedの場合だけ許可

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.OperationalConfigSeal : Prop
And
  (∀ (sealed candidateValid : Bool),
    Iff (Eq.{1} (BridgeSpec.ClaimContracts.operationalConfigSealAllowed sealed candidateValid) Bool.true)
      (And (Eq.{1} sealed Bool.false) (Eq.{1} candidateValid Bool.true)))
  (∀ (sealed : Bool),
    Iff (Eq.{1} (BridgeSpec.ClaimContracts.assetOperationsAllowed sealed) Bool.true) (Eq.{1} sealed Bool.true))
```

### BridgeSpec.ClaimContracts.PaidCallCycleReserve

specification: 課金後もreserveを残す

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.PaidCallCycleReserve : Prop
∀ (liquid reserve attachedCycles callMargin charged : Nat),
  LE.le.{0} (HAdd.hAdd.{0, 0, 0} (HAdd.hAdd.{0, 0, 0} reserve attachedCycles) callMargin) liquid →
    LE.le.{0} charged (HAdd.hAdd.{0, 0, 0} attachedCycles callMargin) →
      LE.le.{0} reserve (HSub.hSub.{0, 0, 0} liquid charged)
```

### BridgeSpec.ClaimContracts.PaymentIdentity

specification: payoutの純額・宛先はrecordに一致し、別IDのrecordは不変

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.PaymentIdentity : Prop
And
  (∀ {record next : BridgeSpec.GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.payout (BridgeSpec.GlobalHistory.Record.id record) ledgerFee transferAmount
            destination))
        (Option.some.{0} next) →
      And (Eq.{1} transferAmount (BridgeSpec.GlobalHistory.Record.netAmount record))
        (And (Eq.{1} destination (BridgeSpec.GlobalHistory.Record.paymentDestination record))
          (LE.le.{0} ledgerFee (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
  (∀ {state next : BridgeSpec.GlobalHistory.GlobalState} {event : BridgeSpec.GlobalHistory.Event} {other : Nat},
    Ne.{1} other (BridgeSpec.GlobalHistory.Event.id event) →
      Eq.{1} (BridgeSpec.GlobalHistory.step state event) (Option.some.{0} next) →
        Eq.{1} (BridgeSpec.GlobalHistory.findRecord? (BridgeSpec.GlobalHistory.GlobalState.records next) other)
          (BridgeSpec.GlobalHistory.findRecord? (BridgeSpec.GlobalHistory.GlobalState.records state) other))
```

### BridgeSpec.ClaimContracts.PendingQueue

specification: blocked retryを保持し、書込失敗時はsessionを保持してdurable結果なし

仕様: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.PendingQueue : Prop
And
  (∀ {queue : BridgeSpec.PendingQueue} {existing incoming : BridgeSpec.PendingQueueEntry},
    Eq.{1} (BridgeSpec.PendingQueueEntry.blocked existing) Bool.true →
      Eq.{1} (queue (BridgeSpec.PendingQueueEntry.key incoming)) (Option.some.{0} existing) →
        Eq.{1}
          (Option.map.{0, 0} (fun entry => BridgeSpec.PendingQueueEntry.blocked entry)
            (BridgeSpec.restorePendingQueue queue incoming (BridgeSpec.PendingQueueEntry.key incoming)))
          (Option.some.{0} Bool.true))
  (∀ (queue : BridgeSpec.PendingQueue),
    And (Eq.{1} (BridgeSpec.PendingQueueWrite.session (BridgeSpec.recordPendingQueueWrite queue Bool.false)) queue)
      (Eq.{1} (BridgeSpec.PendingQueueWrite.durable (BridgeSpec.recordPendingQueueWrite queue Bool.false))
        Option.none.{0}))
```

### BridgeSpec.ClaimContracts.RefundEvidenceEnforcement

specification: 未処理・deposit/digest一致・strict expiryが必要

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RefundEvidenceEnforcement : Prop
∀ {final : BridgeSpec.Protocol.Deposit.State} {historyPrefix suffix : List.{0} BridgeSpec.Protocol.Deposit.Event}
  {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin} {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
  Eq.{1}
      (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial
        (HAppend.hAppend.{0, 0, 0} historyPrefix
          (List.cons.{0} (BridgeSpec.Protocol.Deposit.Event.startExpiredRefund origin evidence) suffix)))
      (Option.some.{0} final) →
    And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
      (Exists.{1} fun authorization =>
        And
          (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
            (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
              (BridgeSpec.MintAuthorization.Authorization.digest authorization))
            (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
              (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))
```

### BridgeSpec.ClaimContracts.RefundRequestAuthorization

specification: authenticated=trueかつdepositProcessed=false

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RefundRequestAuthorization : Prop
∀ {state next : BridgeSpec.Protocol.Deposit.State} {historyPrefix : List.{0} BridgeSpec.Protocol.Deposit.Event}
  {authenticated : Bool} {origin : BridgeSpec.MintAuthorization.AuthorizationOrigin}
  {evidence : BridgeSpec.MintAuthorization.ExpiryEvidence},
  Eq.{1} (BridgeSpec.Protocol.Deposit.depositRun BridgeSpec.Protocol.Deposit.initial historyPrefix)
      (Option.some.{0} state) →
    Eq.{1} (BridgeSpec.MintAuthorization.requestExpiredRefund authenticated state origin evidence)
        (Option.some.{0} next) →
      And (Eq.{1} authenticated Bool.true)
        (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
```

### BridgeSpec.ClaimContracts.ReservationCommit

specification: 予約+候補の合計を保存し、解放は正確に0、二重解放を拒否

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ReservationCommit : Prop
And
  (∀ (reserved candidate : Nat),
    have next := BridgeSpec.commitMintReservation reserved candidate;
    Eq.{1} (HAdd.hAdd.{0, 0, 0} (Prod.fst.{0, 0} next) (Prod.snd.{0, 0} next)) (HAdd.hAdd.{0, 0, 0} reserved candidate))
  BridgeSpec.ClaimContracts.ReservationLifecycle
```

### BridgeSpec.ClaimContracts.ReservationLifecycle

specification: 予約を正確に0へし、二重解放を拒否

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ReservationLifecycle : Prop
And
  (∀ {record next : BridgeSpec.GlobalHistory.Record},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.releaseReservation (BridgeSpec.GlobalHistory.Record.id record)))
        (Option.some.{0} next) →
      Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic next)) 0)
  (∀ {record : BridgeSpec.GlobalHistory.Record},
    Eq.{1} (BridgeSpec.GlobalHistory.Record.reservationReleased record) Bool.true →
      Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal (BridgeSpec.GlobalHistory.Record.phase record)) Bool.false →
        Eq.{1}
          (BridgeSpec.GlobalHistory.applyRecord record
            (BridgeSpec.GlobalHistory.Event.releaseReservation (BridgeSpec.GlobalHistory.Record.id record)))
          Option.none.{0})
```

### BridgeSpec.ClaimContracts.RuntimeAttestationReuse

specification: 再利用domainは現在のinstall domainと一致

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.RuntimeAttestationReuse : Prop
∀ {initialDomain : BridgeSpec.ControlPlane.InstallDomain} {state : BridgeSpec.ControlPlane.State},
  BridgeSpec.ControlPlane.Reachable initialDomain state →
    ∀ (reusedDomain : BridgeSpec.ControlPlane.InstallDomain),
      Eq.{1} (BridgeSpec.ControlPlane.State.lastReusedDomain state) (Option.some.{0} reusedDomain) →
        Eq.{1} reusedDomain (BridgeSpec.ControlPlane.State.domain state)
```

### BridgeSpec.ClaimContracts.ServiceFeeMaximum

specification: fee変更は固定範囲内、署名fee計上は一度だけ

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.ServiceFeeMaximum : Prop
And
  (∀ (serviceFee minimumServiceFee maximumServiceFee : Nat),
    Iff (Eq.{1} (BridgeSpec.serviceFeeChangeAllowed serviceFee minimumServiceFee maximumServiceFee) Bool.true)
      (And (LE.le.{0} minimumServiceFee serviceFee) (LE.le.{0} serviceFee maximumServiceFee)))
  BridgeSpec.ClaimContracts.FeeAccountingOnce
```

### BridgeSpec.ClaimContracts.SettlementBacking

specification: backingを保存し、escrow・feeReserve・未決済債務へ規定差分を適用

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.SettlementBacking : Prop
And
  (∀ {state final : BridgeSpec.GlobalHistory.GlobalState} {events : List.{0} BridgeSpec.GlobalHistory.Event},
    BridgeSpec.GlobalHistory.AccountingInvariant state →
      BridgeSpec.GlobalHistory.Runs state events final →
        BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting final))
  (∀ {record next : BridgeSpec.GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
    Eq.{1}
        (BridgeSpec.GlobalHistory.applyRecord record
          (BridgeSpec.GlobalHistory.Event.payout (BridgeSpec.GlobalHistory.Record.id record) ledgerFee transferAmount
            destination))
        (Option.some.{0} next) →
      And
        (Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic next))
          (HSub.hSub.{0, 0, 0}
            (BridgeSpec.GlobalHistory.Economic.escrow (BridgeSpec.GlobalHistory.Record.economic record))
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record) ledgerFee)))
        (And
          (Eq.{1} (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic next))
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.feeReserve (BridgeSpec.GlobalHistory.Record.economic record))
              (HSub.hSub.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.chargedServiceFee record) ledgerFee)))
          (Eq.{1}
            (BridgeSpec.GlobalHistory.Economic.unreleasedLiability (BridgeSpec.GlobalHistory.Record.economic next))
            (HSub.hSub.{0, 0, 0}
              (BridgeSpec.GlobalHistory.Economic.unreleasedLiability (BridgeSpec.GlobalHistory.Record.economic record))
              (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record)
                (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))))
```

### BridgeSpec.ClaimContracts.SigningCycleReserve

specification: 請求後もreserveを残す

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.SigningCycleReserve : Prop
∀ (liquid reserve signingCost callMargin charged : Nat),
  LE.le.{0} (HAdd.hAdd.{0, 0, 0} (HAdd.hAdd.{0, 0, 0} reserve signingCost) callMargin) liquid →
    LE.le.{0} charged (HAdd.hAdd.{0, 0, 0} signingCost callMargin) →
      LE.le.{0} reserve (HSub.hSub.{0, 0, 0} liquid charged)
```

### BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary

specification: minimumは非0でobservedはminimum以上

仕様: `docs/canister-state-machine.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalAdmissionBoundary : Prop
∀ {observed minimum : Nat},
  Eq.{1} (BridgeSpec.withdrawalIdAdmissible observed minimum) Bool.true →
    And (Eq.{1} (bne.{0} minimum 0) Bool.true) (LE.le.{0} minimum observed)
```

### BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum

specification: 二者が高さをattestし、identity版は高さとhashの二者一致

仕様: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalFinalityQuorum : Prop
And
  (∀ {first second third : Option.{0} Nat} {checkpoint : Nat},
    Eq.{1} (BridgeSpec.withdrawalFinalizedCheckpoint first second third) (Option.some.{0} checkpoint) →
      BridgeSpec.twoFinalizedHeadsAttest first second third checkpoint)
  (∀ {first second third : Option.{0} BridgeSpec.FinalizedIdentity} {checkpoint : BridgeSpec.FinalizedIdentity},
    Eq.{1} (BridgeSpec.withdrawalFinalizedIdentityQuorum first second third) (Option.some.{0} checkpoint) →
      BridgeSpec.twoFinalizedIdentitiesAttest first second third checkpoint)
```

### BridgeSpec.ClaimContracts.WithdrawalFinalization

specification: notifyには成功かつfinalized以下かつcanonicalが必要、finalized不在ならretry

仕様: `docs/bridge-flow.md`

```lean
BridgeSpec.ClaimContracts.WithdrawalFinalization : Prop
And
  (∀ {receiptSucceeded canonical : Bool} {receiptBlock finalizedBlock : Nat},
    Eq.{1}
        (BridgeSpec.decideWithdrawalFinalization receiptSucceeded receiptBlock (Option.some.{0} finalizedBlock)
          canonical)
        BridgeSpec.WithdrawalFinalizationDecision.notify →
      And (Eq.{1} receiptSucceeded Bool.true)
        (And (LE.le.{0} receiptBlock finalizedBlock) (Eq.{1} canonical Bool.true)))
  (∀ {receiptSucceeded canonical : Bool} {receiptBlock : Nat},
    Eq.{1} (BridgeSpec.decideWithdrawalFinalization receiptSucceeded receiptBlock Option.none.{0} canonical)
      BridgeSpec.WithdrawalFinalizationDecision.retry)
```

### BridgeSpec.GlobalHistory.AccountingInvariant

model-support: ID一意性・集計一致・backing・予約のrecord種別だけを拘束する

仕様: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.AccountingInvariant : BridgeSpec.GlobalHistory.GlobalState → Prop
fun state =>
  And (BridgeSpec.GlobalHistory.UniqueIds (BridgeSpec.GlobalHistory.GlobalState.records state))
    (And
      (Eq.{1} (BridgeSpec.GlobalHistory.GlobalState.accounting state)
        (BridgeSpec.GlobalHistory.summarize (BridgeSpec.GlobalHistory.GlobalState.records state)))
      (And (BridgeSpec.GlobalHistory.Backed (BridgeSpec.GlobalHistory.GlobalState.accounting state))
        (BridgeSpec.GlobalHistory.ReservationConsistent (BridgeSpec.GlobalHistory.GlobalState.records state))))
```

### BridgeSpec.GlobalHistory.Backed

specification: escrow=baseSupply+feeReserve+unmintedLiability+unreleasedLiability

仕様: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.Backed : BridgeSpec.GlobalHistory.Economic → Prop
fun economic =>
  Eq.{1} (BridgeSpec.GlobalHistory.Economic.escrow economic)
    (HAdd.hAdd.{0, 0, 0}
      (HAdd.hAdd.{0, 0, 0}
        (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Economic.baseSupply economic)
          (BridgeSpec.GlobalHistory.Economic.feeReserve economic))
        (BridgeSpec.GlobalHistory.Economic.unmintedLiability economic))
      (BridgeSpec.GlobalHistory.Economic.unreleasedLiability economic))
```

### BridgeSpec.GlobalHistory.eventDelta

model-support: 会計イベントの選択。callback phaseは実Ledger送金を認証しない

仕様: `verification/README.md`

```lean
BridgeSpec.GlobalHistory.eventDelta : BridgeSpec.GlobalHistory.Record → BridgeSpec.GlobalHistory.Event → Option.{0} BridgeSpec.GlobalHistory.Delta
fun record event =>
  ite.{1}
    (Or (Ne.{1} (BridgeSpec.GlobalHistory.Event.id event) (BridgeSpec.GlobalHistory.Record.id record))
      (Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal (BridgeSpec.GlobalHistory.Record.phase record)) Bool.true))
    Option.none.{0}
    (match event with
    | BridgeSpec.GlobalHistory.Event.installSignature id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.feeApplied record)) Bool.true)))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.signature (BridgeSpec.GlobalHistory.Record.chargedServiceFee record)))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.mint id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (And
              (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
                0)
              (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.mintApplied record)) Bool.true))))
        (Option.some.{0} (BridgeSpec.GlobalHistory.Delta.mint (BridgeSpec.GlobalHistory.Record.netAmount record)))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.refund id amount =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.funded)
            (And
              (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
                0)
              (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.refundApplied record)) Bool.true))))
        (Option.some.{0} (BridgeSpec.GlobalHistory.Delta.refund amount)) Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.cancel id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.pending)
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
              0)))
        (Option.some.{0} BridgeSpec.GlobalHistory.Delta.none) Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.payout id ledgerFee transferAmount destination =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.withdrawal)
          (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.phase record) BridgeSpec.GlobalHistory.Phase.committed)
            (And (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.payoutApplied record)) Bool.true)
              (And (LE.le.{0} ledgerFee (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))
                (And (Eq.{1} transferAmount (BridgeSpec.GlobalHistory.Record.netAmount record))
                  (Eq.{1} destination (BridgeSpec.GlobalHistory.Record.paymentDestination record)))))))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.payout
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record) ledgerFee)
            (HSub.hSub.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.chargedServiceFee record) ledgerFee)
            (HAdd.hAdd.{0, 0, 0} (BridgeSpec.GlobalHistory.Record.netAmount record)
              (BridgeSpec.GlobalHistory.Record.chargedServiceFee record))))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.releaseReservation id =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.kind record) BridgeSpec.GlobalHistory.RecordKind.deposit)
          (Eq.{1} (Bool.not (BridgeSpec.GlobalHistory.Record.reservationReleased record)) Bool.true))
        (Option.some.{0}
          (BridgeSpec.GlobalHistory.Delta.releaseReservation
            (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))))
        Option.none.{0}
    | BridgeSpec.GlobalHistory.Event.callback id generation nextPhase =>
      ite.{1}
        (And (Eq.{1} (BridgeSpec.GlobalHistory.Record.leaseGeneration record) (Option.some.{0} generation))
          (Or (Eq.{1} (BridgeSpec.GlobalHistory.Phase.terminal nextPhase) Bool.false)
            (Eq.{1} (BridgeSpec.GlobalHistory.Economic.reservedMint (BridgeSpec.GlobalHistory.Record.economic record))
              0)))
        (Option.some.{0} BridgeSpec.GlobalHistory.Delta.none) Option.none.{0})
```

### BridgeSpec.Liveness.AdmissibleUntilOccurs

model-support: 対象終端イベントが選択まで常時受理可能であるという仮定

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.AdmissibleUntilOccurs : BridgeSpec.Liveness.Execution → BridgeSpec.GlobalHistory.Event → Nat → Prop
fun execution event start =>
  ∀ (time : Nat),
    LE.le.{0} start time →
      Not (BridgeSpec.Liveness.OccurredBefore execution event start time) →
        Exists.{1} fun next =>
          Eq.{1} (BridgeSpec.GlobalHistory.step (BridgeSpec.Liveness.Execution.state execution time) event)
            (Option.some.{0} next)
```

### BridgeSpec.Liveness.CommonOperationalAssumptions

model-support: readyAt以降の継続的な可用性とweak fairnessをまとめた仮定

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.CommonOperationalAssumptions : BridgeSpec.Liveness.Execution → Nat → Type
-- 構造・帰納型の型。フィールド等の変更もソースdigestで通知する。
```

### BridgeSpec.Liveness.DepositTerminalProgressLemmas

model-support: mintの条件付き含意とrefundの条件付き含意の論理積。共通実行の二者択一ではない

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.DepositTerminalProgressLemmas : Prop
And BridgeSpec.Liveness.FundedDepositEventuallyMinted BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded
```

### BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded

model-support: 対象depositがrefundedへ到達

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id amount start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.KeeperActionAssumption execution
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.refund id amount)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.refunded)
        execution start
```

### BridgeSpec.Liveness.FundedDepositEventuallyMinted

model-support: 対象depositがmintedへ到達

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.FundedDepositEventuallyMinted : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.UserActionAssumption execution (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.mint id)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.minted)
        execution start
```

### BridgeSpec.Liveness.FundingFailureEventuallyCancelled

model-support: 対象depositがcancelledへ到達

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.FundingFailureEventuallyCancelled : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.AdmissibleUntilOccurs execution (BridgeSpec.GlobalHistory.Event.cancel id)
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.cancelled)
      execution start
```

### BridgeSpec.Liveness.WeakFair

model-support: 継続的に有効な具体イベントが将来選択されるという仮定

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.WeakFair : BridgeSpec.Liveness.Execution → Prop
fun execution =>
  ∀ (event : BridgeSpec.GlobalHistory.Event) (needsUser : Bool) (start : Nat),
    BridgeSpec.Liveness.EnabledUntilOccurs execution event needsUser start →
      Exists.{1} fun time =>
        And (LE.le.{0} start time)
          (Eq.{1} (BridgeSpec.Liveness.Execution.action execution time) (Option.some.{0} event))
```

### BridgeSpec.Liveness.WithdrawalEventuallyPaid

model-support: 対象withdrawalがpaidへ到達

仕様: `verification/conditional-liveness.md`

```lean
BridgeSpec.Liveness.WithdrawalEventuallyPaid : Prop
∀ (execution : BridgeSpec.Liveness.Execution) (id ledgerFee transferAmount destination start : Nat)
  (common : BridgeSpec.Liveness.CommonOperationalAssumptions execution start),
  BridgeSpec.Liveness.KeeperActionAssumption execution
      (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
    BridgeSpec.Liveness.AdmissibleUntilOccurs execution
        (BridgeSpec.GlobalHistory.Event.payout id ledgerFee transferAmount destination)
        (BridgeSpec.Liveness.CommonOperationalAssumptions.readyAt common) →
      BridgeSpec.Liveness.Eventually (BridgeSpec.Liveness.ReachesPhase id BridgeSpec.GlobalHistory.Phase.paid) execution
        start
```

### BridgeSpec.MintAuthorization.Authorization.valid

specification: 認可のdomain・epoch・金額・IC起点期限の一致。digestの暗号計算は対象外

仕様: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.Authorization.valid : BridgeSpec.MintAuthorization.Authorization → BridgeSpec.MintAuthorization.AuthorizationOrigin → Prop
fun authorization origin =>
  And (Ne.{1} (BridgeSpec.MintAuthorization.Authorization.recipient authorization) 0)
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.chainId authorization)
        (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedChainId origin))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization)
          (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedVerifyingContract origin))
        (And (Ne.{1} (BridgeSpec.MintAuthorization.Authorization.digest authorization) 0)
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.epoch authorization)
              (BridgeSpec.MintAuthorization.AuthorizationOrigin.expectedEpoch origin))
            (And (Ne.{1} (BridgeSpec.MintAuthorization.AuthorizationOrigin.finalizedHash origin) 0)
              (And
                (Eq.{1}
                  (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.Authorization.netAmount authorization)
                    (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                  (BridgeSpec.MintAuthorization.Authorization.grossAmount authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.Authorization.deadline authorization)
                    (HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin)
                      BridgeSpec.MintAuthorization.authorizationTtl))
                  (Eq.{1}
                    (BridgeSpec.MintAuthorization.deadlineFromIssuedAt
                      (BridgeSpec.MintAuthorization.AuthorizationOrigin.issuedAtTimestamp origin))
                    (Option.some.{0} (BridgeSpec.MintAuthorization.Authorization.deadline authorization))))))))))
```

### BridgeSpec.MintAuthorization.ExpiryEvidence.valid

specification: decode済み未処理証拠とstrict expiryの照合

仕様: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.ExpiryEvidence.valid : BridgeSpec.MintAuthorization.ExpiryEvidence →
  BridgeSpec.MintAuthorization.Authorization → BridgeSpec.MintAuthorization.AuthorizationOrigin → Prop
fun evidence authorization origin =>
  And
    (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositId evidence)
      (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.authorizationDigest evidence)
        (BridgeSpec.MintAuthorization.Authorization.digest authorization))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.chainId evidence)
          (BridgeSpec.MintAuthorization.Authorization.chainId authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.verifyingContract evidence)
            (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization))
          (And (Eq.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.depositProcessed evidence) Bool.false)
            (And
              (GE.ge.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedBlock evidence)
                (BridgeSpec.MintAuthorization.AuthorizationOrigin.finalizedBlock origin))
              (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedHash evidence) 0)
                (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.runtimeSha256 evidence) 0)
                  (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.rpcRequestDigest evidence) 0)
                    (And (Ne.{1} (BridgeSpec.MintAuthorization.ExpiryEvidence.rpcResponseDigest evidence) 0)
                      (GT.gt.{0} (BridgeSpec.MintAuthorization.ExpiryEvidence.finalizedTimestamp evidence)
                        (BridgeSpec.MintAuthorization.Authorization.deadline authorization)))))))))))
```

### BridgeSpec.MintAuthorization.MintEvidence.valid

specification: decode済みmint証拠のfield照合。非0hashの真正性は外部前提

仕様: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.MintEvidence.valid : BridgeSpec.MintAuthorization.MintEvidence → BridgeSpec.MintAuthorization.Authorization → Prop
fun evidence authorization =>
  And
    (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.depositId evidence)
      (BridgeSpec.MintAuthorization.Authorization.depositId authorization))
    (And
      (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.recipient evidence)
        (BridgeSpec.MintAuthorization.Authorization.recipient authorization))
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.authorizationDigest evidence)
          (BridgeSpec.MintAuthorization.Authorization.digest authorization))
        (And
          (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.chainId evidence)
            (BridgeSpec.MintAuthorization.Authorization.chainId authorization))
          (And
            (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.verifyingContract evidence)
              (BridgeSpec.MintAuthorization.Authorization.verifyingContract authorization))
            (And
              (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.grossAmount evidence)
                (BridgeSpec.MintAuthorization.Authorization.grossAmount authorization))
              (And
                (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.chargedServiceFee evidence)
                  (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                (And
                  (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.mintedAmount evidence)
                    (BridgeSpec.MintAuthorization.Authorization.netAmount authorization))
                  (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.transactionHash evidence) 0)
                    (And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptSucceeded evidence) Bool.true)
                      (And
                        (LE.le.{0} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlock evidence)
                          (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlock evidence))
                        (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.receiptBlockHash evidence) 0)
                          (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.finalizedBlockHash evidence) 0)
                            (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.rpcRequestDigest evidence) 0)
                              (And (Ne.{1} (BridgeSpec.MintAuthorization.MintEvidence.rpcResponseDigest evidence) 0)
                                (And (Eq.{1} (BridgeSpec.MintAuthorization.MintEvidence.exactEventCount evidence) 1)
                                  (Eq.{1}
                                    (HAdd.hAdd.{0, 0, 0}
                                      (BridgeSpec.MintAuthorization.Authorization.netAmount authorization)
                                      (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization))
                                    (BridgeSpec.MintAuthorization.Authorization.grossAmount
                                      authorization)))))))))))))))))
```

### BridgeSpec.MintAuthorization.installSignature

specification: 署名時の残り300秒、有限幅、fee一回性を検査する抽象遷移

仕様: `verification/README.md`

```lean
BridgeSpec.MintAuthorization.installSignature : BridgeSpec.MintAuthorization.DepositState → Nat → Option.{0} BridgeSpec.MintAuthorization.DepositState
fun state observedTimestamp =>
  match BridgeSpec.MintAuthorization.DepositState.authorization state with
  | Option.none.{0} => Option.none.{0}
  | Option.some.{0} authorization =>
    ite.{1}
      (And
        (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.phase state)
          BridgeSpec.MintAuthorization.DepositPhase.authorizationPending)
        (And (Eq.{1} (BridgeSpec.MintAuthorization.DepositState.feeCounted state) Bool.false)
          (And
            (LE.le.{0} (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization)
              (BridgeSpec.MintAuthorization.DepositState.pendingDepositLiability state))
            (Eq.{1}
              (BridgeSpec.signatureTimeAllowed observedTimestamp
                (BridgeSpec.MintAuthorization.Authorization.deadline authorization))
              Bool.true))))
      (Option.some.{0}
        { phase := BridgeSpec.MintAuthorization.DepositPhase.authorizationAvailable,
          authorization := BridgeSpec.MintAuthorization.DepositState.authorization state,
          escrow := BridgeSpec.MintAuthorization.DepositState.escrow state,
          baseSupply := BridgeSpec.MintAuthorization.DepositState.baseSupply state,
          feeReserve :=
            HAdd.hAdd.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.feeReserve state)
              (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization),
          pendingDepositLiability :=
            HSub.hSub.{0, 0, 0} (BridgeSpec.MintAuthorization.DepositState.pendingDepositLiability state)
              (BridgeSpec.MintAuthorization.Authorization.chargedServiceFee authorization),
          reservedMint := BridgeSpec.MintAuthorization.DepositState.reservedMint state, feeCounted := Bool.true,
          jobNextRun := BridgeSpec.MintAuthorization.DepositState.jobNextRun state,
          leaseGeneration := BridgeSpec.MintAuthorization.DepositState.leaseGeneration state })
      Option.none.{0}
```

### BridgeSpec.Protocol.Safe

model-support: 統合単一recordモデルの会計・quote・lease条件。全runtime安全性ではない

仕様: `verification/README.md`

```lean
BridgeSpec.Protocol.Safe : BridgeSpec.Protocol.ProtocolState → Prop
fun state =>
  And (BridgeSpec.Backed (BridgeSpec.Protocol.ProtocolState.economic state))
    (And
      (Eq.{1} (BridgeSpec.FeeState.reserve (BridgeSpec.Protocol.ProtocolState.fee state))
        (BridgeSpec.EconomicState.feeReserve (BridgeSpec.Protocol.ProtocolState.economic state)))
      (And
        (LE.le.{0} (BridgeSpec.FeeState.pendingPayout (BridgeSpec.Protocol.ProtocolState.fee state))
          (BridgeSpec.FeeState.reserve (BridgeSpec.Protocol.ProtocolState.fee state)))
        (And
          (LE.le.{0}
            (HAdd.hAdd.{0, 0, 0}
              (BridgeSpec.Protocol.WindowState.consumed (BridgeSpec.Protocol.ProtocolState.window state))
              (BridgeSpec.Protocol.WindowState.reserved (BridgeSpec.Protocol.ProtocolState.window state)))
            BridgeSpec.FiniteWidthModel.maxU128)
          (And
            (Eq.{1}
              (HAdd.hAdd.{0, 0, 0}
                (BridgeSpec.Protocol.DepositTrace.reserved (BridgeSpec.Protocol.ProtocolState.deposit state))
                (BridgeSpec.Protocol.DepositTrace.candidate (BridgeSpec.Protocol.ProtocolState.deposit state)))
              (BridgeSpec.Protocol.DepositTrace.requirement (BridgeSpec.Protocol.ProtocolState.deposit state)))
            (And
              (Eq.{1} (BridgeSpec.Withdrawal.destination (BridgeSpec.Protocol.ProtocolState.withdrawal state))
                (BridgeSpec.Protocol.ProtocolState.committedDestination state))
              (And
                (Eq.{1} (BridgeSpec.Withdrawal.amountOut (BridgeSpec.Protocol.ProtocolState.withdrawal state))
                  (BridgeSpec.Protocol.ProtocolState.committedAmountOut state))
                (And
                  (Eq.{1} (BridgeSpec.Protocol.ProtocolState.withdrawalFeeCounted state)
                    (BridgeSpec.Withdrawal.paid (BridgeSpec.Protocol.ProtocolState.withdrawal state)))
                  (BridgeSpec.Protocol.leaseBounded (BridgeSpec.Protocol.ProtocolState.lease state)))))))))
```

### BridgeSpec.Protocol.filterSafeStoredState

model-support: Safeを条件として受理する抽象フィルタ。本番decodeやmigrationではない

仕様: `verification/README.md`

```lean
BridgeSpec.Protocol.filterSafeStoredState : BridgeSpec.Protocol.ProtocolState → Option.{0} BridgeSpec.Protocol.ProtocolState
fun stored => ite.{1} (BridgeSpec.Protocol.Safe stored) (Option.some.{0} stored) Option.none.{0}
```

### BridgeSpec.signatureTimeAllowed

specification: u64期限・加算overflow拒否・最低残存300秒のpredicate

仕様: `verification/README.md`

```lean
BridgeSpec.signatureTimeAllowed : Nat → Nat → Bool
fun observedTimestamp deadline =>
  Decidable.decide
    (And (LE.le.{0} deadline (HSub.hSub.{0, 0, 0} (HPow.hPow.{0, 0, 0} 2 64) 1))
      (And (LE.le.{0} observedTimestamp (HSub.hSub.{0, 0, 0} (HSub.hSub.{0, 0, 0} (HPow.hPow.{0, 0, 0} 2 64) 1) 300))
        (LE.le.{0} (HAdd.hAdd.{0, 0, 0} observedTimestamp 300) deadline)))
```

## Source inventory (conservative change detection)

| Source | Lines | Declarations | SHA-256 |
|---|---:|---:|---|
| verification/lean/BridgeSpec/AuditExport.lean | 18 | 1 | 71f528152dcd1c0a250ae213e0d250e8c4a46d42002c38d579b13679dd7bf501 |
| verification/lean/BridgeSpec/ClaimContracts.lean | 633 | 106 | 3d8f15747089cba5ba9eda5a60b7492225698d5f45ad94196ce7eceb102d1c12 |
| verification/lean/BridgeSpec/Claims.lean | 226 | 28 | f74ffe05f86fbdfa4e94095bdc2dc64a1dd3ed94c947a4b800af6433dada3430 |
| verification/lean/BridgeSpec/ControlPlane.lean | 323 | 32 | 57e6656b42d4da34726d8e69bfe129939d7649070923beddc9c5fdce8876936c |
| verification/lean/BridgeSpec/DepositAuthorization.lean | 586 | 48 | 737633200787c8db2275d4a8408e8bad75754e04415c9cbc435b934660ca001e |
| verification/lean/BridgeSpec/DepositHistory.lean | 561 | 27 | 51536b20ba0fd5da8a26491d13faf56b5e38a3c300f16e319e99ee5c8dec6620 |
| verification/lean/BridgeSpec/FiniteWidthModel.lean | 137 | 31 | 0345c19c9df5a982a4f896205cbde5d01a40dd68a8bd1f4e3c58e9923e3cf0ea |
| verification/lean/BridgeSpec/GlobalHistory.lean | 889 | 67 | a1b6668db4ec35f997042da7fbfb73285d8606a2891084b4645542e2e8b7ea37 |
| verification/lean/BridgeSpec/LedgerBlockProvenance.lean | 337 | 28 | 23421e6d692d4651eb98d658e501e918e185f36d378f72ed5115fbaf06ba69c4 |
| verification/lean/BridgeSpec/Liveness.lean | 209 | 26 | 72402ad8473fc2e294a3725535ade0153ed8bd51270ebb22660d75d8706aeff0 |
| verification/lean/BridgeSpec/Model.lean | 323 | 60 | a9f07f9643e72cfb41bf9c1d14a34c5c6ab5108fc3cd54886e31216c0889ecd3 |
| verification/lean/BridgeSpec/ModelBoundaries.lean | 42 | 4 | 67b361fa752dfd66e101553b9a0ed6b49d177d49866b94239c8a6c214eba7d13 |
| verification/lean/BridgeSpec/ModelRefinement.lean | 161 | 24 | 101d62bf7e751cdcc14e0bccf61b9d6ba12326fa8cccdb70df63a51338649342 |
| verification/lean/BridgeSpec/Protocol.lean | 809 | 34 | 41678966a02226b6930bac1f74881d49b6d20f13c60671d7354da0bd90571043 |
| verification/lean/BridgeSpec/ProtocolPolicies.lean | 420 | 52 | a2702b5b210970ee5ac01e5c3109697c902775610de2e8982da60195d132f71c |
| verification/lean/BridgeSpec/Theorems.lean | 204 | 19 | 848b4f6361b2f097725a3ab084b9e7344d03c906030ca8ad004bb19095e6c4f4 |
| verification/lean/BridgeSpec/Vectors.lean | 439 | 41 | 00c33dad5b79d86ac6c9147f33352a412bc20f672e9c7ad33e25961a0a7e9325 |
| verification/lean/Main.lean | 10 | 1 | 286cd7fcd66afc4e7532fb8f2f0d7e0e15f858ffdad3de716c9f83bfc6b42a05 |
| verification/lean/fail/AccountingDeltaViolation.lean | 24 | 1 | 638f8837d8c221d828fed5162973b4215af39c61984c6aba3220f8b8b3dd7a08 |
| verification/lean/fail/AnonymousRefundRequest.lean | 8 | 0 | beae263600cfc9307feadae225b64983f34e11bfdfa04adae6316ec7a1869552 |
| verification/lean/fail/AuthorizationReissue.lean | 25 | 4 | ee67fee642f47c1d116fc94f8a5f9387e96525dfa5397646063d6fbc6451d508 |
| verification/lean/fail/BackingViolation.lean | 12 | 0 | 0ad4d6a1424518a8b8f9968310d0a88619133f86d4ce877fa7134a0f22b206d2 |
| verification/lean/fail/ConflictingFundingReplay.lean | 9 | 0 | fd5f6abf7f8812e64bf2cbf457991ca46fcea6cdcc71acd074a6b162317c7b96 |
| verification/lean/fail/DeadlineOverflow.lean | 6 | 0 | 864692b1294fc6b36f398fc91ea14695d07fea1c3defc14708eccc329aa74d5c |
| verification/lean/fail/DestinationMutation.lean | 20 | 3 | a3992470b4340e01eb81291efe4172fb9b3269141e987beea8238f02fded0cae |
| verification/lean/fail/DoubleDepositFeeTrace.lean | 7 | 0 | 9f00413132deded07cee5385e9a56384775d7b6b869bfad08b31c384059605ba |
| verification/lean/fail/DoubleFee.lean | 6 | 0 | 076049e9a5e13cd6c1e4ebab231ed452dffc9fcac994b7aa15bdd8645e304851 |
| verification/lean/fail/DriftedConfirmedActivationEvidence.lean | 6 | 0 | cce72323186747a19ebd7bd19393b75624c5b5f5a330eebb6a24b6c75aa1df5a |
| verification/lean/fail/EvidencelessMint.lean | 19 | 2 | 7d248eb1e8b57cfd2e9f2c3d3892e03b495934fbc920278146a056cca36b2c96 |
| verification/lean/fail/FinalizedTimestampDeadline.lean | 7 | 0 | 614a8705cfd818db1749f072723d51e18f07d03b750a882a3f8d1fdb2935a212 |
| verification/lean/fail/IncompleteAbsence.lean | 15 | 0 | 22ed9af027b854b148ed57fd103aa4cf9707a4366943ce43b4276e3ca4cbad62 |
| verification/lean/fail/IncompleteExpiryAudit.lean | 22 | 2 | 4c30cb6604648725bea1d9a376cd758efedc2bbbc029c26088ed070229be55bd |
| verification/lean/fail/IncompleteMintAudit.lean | 21 | 2 | 34a54671a3271bf8efb75d8ffa94483006cb795bca44a73eeccd29384ed3ec1c |
| verification/lean/fail/InvalidExecutionStep.lean | 10 | 1 | a6b48a8e639af5578312a150535c1b778d45877f353b449eca9559558c156213 |
| verification/lean/fail/ManualActiveLeaseBypass.lean | 6 | 0 | 05b63de3fa0532e200bdd0c2f8e75495f96b40683b11177827e16335ee5fa7a7 |
| verification/lean/fail/ManualClaimEconomicMutation.lean | 13 | 1 | 9b60fdf900a577eea35aa64585347ed44b629cceae79b2dbbc0ede270ad7722d |
| verification/lean/fail/MintFeeAlreadyCounted.lean | 19 | 2 | 1506eb9046d1280a73afc6a2e0203ad2ebc477b2eb62afc1e8360bbd6bbb4a68 |
| verification/lean/fail/MissingUserLiveness.lean | 16 | 1 | 285ee5520011c8603f7db684c1a6365bef69da98d9e38c6ed60a4f0b69bbe07e |
| verification/lean/fail/ProcessedExpiryRefund.lean | 21 | 2 | 4dcff1642961a5f622a762ac1b4748b990a5b4ee6a480d4e6a42abf76f7e0773 |
| verification/lean/fail/QuoteChangedDuringTrace.lean | 8 | 0 | 9a520139c90b0736b11e639ee117cd5e8d2b58e7f36ef7fe039ebbb7b72fa645 |
| verification/lean/fail/RefundAfterReservationReplay.lean | 6 | 0 | 398008e4f37597fc27cdc61909bfe32312895afa6bb84128a5d01a252b164afd |
| verification/lean/fail/RefundBeforeFunding.lean | 9 | 0 | 47ac92b520aef0fc257bf1a3fa956af2ce6130b9bf676b56d8936e62c2bd56f0 |
| verification/lean/fail/SignatureTooLate.lean | 4 | 0 | bdd74ab81077e94e7f16dedce8b2dd0bd5db6a71c6d31e57e6fae72dc9fbc106 |
| verification/lean/fail/StaleCrossRecordCallback.lean | 27 | 2 | 0394bec7a1db4833940a74de7386b6150db5753b09b2cae5b8d5ebb8c64330b1 |
| verification/lean/fail/StaleLeaseCallback.lean | 9 | 0 | 060d4a6c2845d57c76dc64427eca5be79f8444c0472b7191a2ffffe4f34c2dae |
| verification/lean/fail/TerminalAuthorizationReopen.lean | 12 | 1 | 33f763ac7516e6a73762e978616d1d89063aac597834af2f954f90446805c4b1 |
| verification/lean/fail/TerminalDepositIndexed.lean | 6 | 0 | 5a740f2449b4ebd506b9d3095378f2226f253b9fd4ec96442422a0adbcbd91d7 |
| verification/lean/fail/UnauthorizedConfirmationCaller.lean | 8 | 0 | ebb7016ca30fe6b8c28c6dc6533da01331be76d3cc479a886866213d52ce80d0 |
| verification/lean/fail/UnauthorizedOperationalConfigSeal.lean | 6 | 0 | 8d45c7d35059439e55d1e8b7eaec11c842f9918084a8d4410c70dc986f43423e |
| verification/lean/fail/UnfairLiveness.lean | 16 | 1 | e346efb2083754bc8793d27a176243cc79c29f55a54eed0af4778e85faff6eca |
| verification/lean/fail/UnsafeBootstrapPausePrincipalMigration.lean | 6 | 0 | ddd05423bdf6e17a1ca3f0be7a2eae1694f5e331b638d5939fef44dafaf485eb |
| verification/lean/lakefile.lean | 11 | 0 | ea0d55c075440c9541d28b7802430abfe5e99b6a2e5a0abb561b76963055950c |
