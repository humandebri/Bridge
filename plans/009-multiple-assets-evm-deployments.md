# Plan 009: 既存Base版KINICを維持した複数資産とEVMチェーンへの拡張

状態: 設計方針合意済み、実装未着手。
2026-09-08のユーザー指示に基づく。

## 対象と配置単位

ICP上の複数資産を複数のEVMチェーンへ橋渡しする。
**配置単位**は、1つのICP Ledgerと1つのEVMチェーンに対応するBridge Canister、Bridgeコントラクト、発行トークンの組とする。
**既存配置**はデプロイ済みのBase版KINICの組を指し、**追加配置**は今後作成する別の組を指す。
同じ資産を別チェーンへ追加する場合もCanisterを分ける。
共通UIは、検証済みの配置から資産と接続先を選択する。

非EVMチェーン、EVM間の直接転送、1つのCanisterによる複数資産の保管は対象外とする。
追加する具体的な資産とチェーンは未指定であり、対応済みとは扱わない。

## 既存配置の維持条件

[Bridge.sol](../contracts/src/Bridge.sol)はconstructorでトークンを生成し、`bsns`をimmutableに保持する。
[BSNS.sol](../contracts/src/BSNS.sol)はmintとburnの権限を生成元Bridgeへimmutableに固定する。
したがって、既存トークンを新Bridgeへ付け替える移行は計画に含めない。
既存配置のアドレス、ABI、イベント、EIP-712 domainと型、署名検証、トークンの8桁表現を維持する。
資金移動、トークン交換、再install、既存配置の再activationを拡張の前提にしない。

実装前に、既存配置のdeployment artifactと配置時のsource revisionを特定し、runtime hash、ABI、署名の既知ベクトルを固定する。
現在のソースが配置済みbytecodeと一致すると推定しない。
実装時には、その固定した証拠に対して既存配置の接続テストを実施する。

本番Canisterはschema v35、現在のソースは未配置のv36という既存方針を維持する。
既存UIのv35証拠系列への認可制限と、通常の現行release Gate Bがv36だけを受理する制限を緩和しない。
追加配置に対する証拠は配置ごとに発行し、既存配置のreceiptを転用しない。
詳細は[AGENTS.md](../AGENTS.md)のproduction compatibility policyに従う。

## 追加配置の設計

Canisterの保管口座、会計、未完了処理、履歴、pause、上限、cyclesを配置ごとに分離する。
同じLedgerを使う場合も、別Canisterが所有する口座で裏付けを分ける。
各配置の既存会計不変条件に、mint予約、返金債務、出金債務、手数料を含めて適用する。
別配置の残高で不足を補填する処理は追加しない。

追加配置のinstall時に、Ledgerと必要なIndex、chain ID、Bridgeとトークンのアドレス、runtime hash、RPC集合、署名鍵の導出パス、トークン情報、手数料方針、上限を束縛する。
実行中の接続先変更機能は設けない。
既存配置の署名鍵を変更せず、追加配置には配置を区別できる導出パスを使う。
chain IDとverifying contractによる署名束縛を維持し、別配置の認可、receipt、履歴を受理しない。

追加資産は、現在利用しているLedger操作と履歴照合を満たすものに限定する。
ICRC対応という名称だけで、承認送金、重複検知、archive discovery、Indexの互換性を仮定しない。
初期範囲ではICP側とEVM側のdecimalsを一致させ、桁変換と丸めを導入しない。
固定されたKINIC Ledger手数料は資産別の明示的な方針へ置き換えるが、手数料変更時の照合と再試行の仕様を決めてから実装する。
扱える金額は既存のu128制約を満たすものとする。

追加チェーンは、使用opcode、署名方式、receiptとeventの形式、Finalizedの意味、RPC quorum、governance transactionのfee計算を検証して採用する。
Base固有のL1 fee前提を他チェーンに流用しない。
[ADR 0024](../docs/adr/0024-validate-rpc-chain-binding-before-runtime.md)の固定provider前提と配置前chain binding検査を維持する。
具体的な接続先が決まった時点で、各チェーンとRPCの現在の一次資料を確認する。

追加配置用コントラクトでは、トークン名、symbol、decimalsを配置時に確定できるようにする。
既存配置との共通化は、実際のABIと署名仕様を保てる範囲に限定する。
未配置の設定形式は呼出元とfixtureをまとめて置換し、旧形式のfallbackを追加しない。
本番の固定KINIC検査を単に削除して任意設定を許可する変更は行わず、検証済み配置の認可条件に置き換える。

## UIでの配置選択

資産とチェーンの選択から、完全な配置profileを一意に解決する。
symbolだけを資産の識別子に使わず、Ledger、chain ID、Canister、契約、deployment instanceの束縛を検証する。
ユーザー入力の任意RPCや契約を本番配置として受理しない。

履歴、pending transaction、query cache、非同期応答は配置ごとに分離する。
選択を切り替えた後も、進行中の処理は開始時の配置に結び付ける。
古い画面の応答が新しい配置で送金や確認を起動しないことをテストする。
walletの接続チェーンが選択先と一致した場合だけ、EVM transactionを要求する。
既存Base版KINICの履歴と回復操作は継続して利用できることを完了条件に含める。

## 実装順序と検証

1. 既存配置のartifact、ABI、署名ベクトル、profile認可、履歴の回帰基準を固定する。配置済み証拠を特定できない場合は既存接続に影響する変更を始めない。
2. 追加候補のLedgerとEVMチェーンを選定し、互換性、手数料方針、運用主体、配置数に応じたcycles負担を確定する。
3. 配置時に固定する設定と認可条件を実装し、単一資産を扱うCanisterとコントラクトを追加配置へ適用する。最初はlocal fixtureで検証する。
4. 共通UIの配置選択と履歴分離を実装する。
5. 同じ資産の別チェーン配置と、別資産の同じチェーン配置をlocalで検証し、配置間の混同を拒否することを確認する。
6. sourceを固定し、追加配置ごとにstagingとreleaseの証拠を取得する。外部deployと既存Canisterのupgradeは、それぞれ具体的な対象と証拠を揃えた別操作とする。

安全性ロジックを変更する前に、[proof-impact.tsv](../verification/proof-impact.tsv)と[claims.tsv](../verification/claims.tsv)から影響claim IDを列挙する。
各claimの抽象定理、production kernel、proof obligation、negative fixture、refinementまたはadapter test、transaction test、vector consumer、外部仮定を対応付ける。
本計画ではロジックを変更していないため、claimの証明状態を更新しない。

実装時にはmanifest検査と影響stageのproofを実行する。
PR検証には`python3 scripts/check_proof_impact.py`、`python3 scripts/check_claim_manifest.py`、`scripts/ci-local.sh proofs-impacted <changed-paths.json>`および対象テストを含める。
UI変更はPlaywright CLIで確認する。
release candidateは`scripts/ci-local.sh all`、production driverは完全なcurrent-source receiptを伴う`scripts/ci-local.sh proofs`を実行する。
高コスト検証の前提となる軽量検査、単一writer、入力固定、receipt検証は既存のAGENTS.mdに従う。

受入テストには、既存配置の双方向処理と回復、追加配置の双方向処理、異なるdecimals、手数料不一致、別配置署名の拒否、chain不一致、receipt混同、UI切替中の非同期応答、配置単位のpauseを含める。
新しいチェーンのfinalityとproviderの独立性、Ledgerの外部動作、鍵とgovernanceの信頼は外部仮定として残し、未証明のclaimをcompleteに引き上げない。

## 今回の成果と未実施事項

今回の変更は本計画と索引の追加のみである。
追加資産と接続先の選定、実装、テスト実行、chain上の現状照会、deploy、Canister upgradeは未実施である。
作業ツリーに存在する進行中のコード変更は本計画の成果に含めない。
