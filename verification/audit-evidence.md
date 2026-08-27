# External audit evidence matrix

監査時の正本は、`verification/claims.tsv`から決定的に生成されるschema 7の
`verification/output/claim-report.json`である。JSONの各`claims[]`要素が一つのclaimに
対応し、次を別々に表示する。

- `assurance_target`: release判定対象か、モデル補助証拠か。
- `required_strength`: 当該claimがreleaseに必要とする最低強度。
- `evidence_strength`: `abstract-proved`、`production-linked`、
  `implementation-proved`のいずれか。
- `typed_implementation_basis`: production symbol、transaction test、production-bound
  Verus、bounded conformance、supporting SMT/Halmosの種別付き一覧。
- `release_blockers`: production link、transaction test、要求強度の不足。
- `unproved_reasons`と`external`: 未結合proof境界と外部仮定。

`release-ready`は外部仮定やTCBがないことを意味しない。manifestとchecker側固定policyで
完全一致したtarget・最低強度、および宣言したproduction結合を満たすことだけを意味する。
現在の固定policyは38件すべてをrelease対象とし、20件へ`implementation-proved`、18件へ
`production-linked`を要求する。SMT、Halmos、生成vector、model-only
Verusはsupporting evidenceであり、それ単独で`implementation-proved`にならない。

条件付きliveness 5件は`claims[]`に含めず、`conditional_liveness[]`へ分離する。それらの
Lean定理、強い仮定、production未結合境界は`conditional-liveness.tsv`と
`conditional-liveness.md`を参照する。定理の完全修飾名、命題型、仮定集合は固定policyと
完全一致させ、Leanによる型検査とaxiom dependency検査を行う。

監査提出前には固定toolchainで`scripts/ci-local.sh proofs`を実行し、receipt schema 7の
source fingerprint、全10 stageの`pass`、`complete: true`、38件の`release-ready`、
`release-blocked: 0`、`model-support: 0`、19/18件の証拠強度区分を確認する。receipt自体はgit追跡せず、監査対象checkout
から再生成する。
