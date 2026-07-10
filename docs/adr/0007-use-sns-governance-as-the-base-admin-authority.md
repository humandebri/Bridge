---
status: superseded by ADR-0009
---

# SNS GovernanceをBase adminの権限主体にする

Base contractのlimit変更、role rotation、unpauseを承認するGovernance AuthorityはSNS Governanceとする。標準SNS Governance canisterへEVM実装を追加せず、採択済みcustom proposalをEVM transactionへ変換するだけのGovernance Executorを置く。

## Considered Options

- operational Bridge canisterがadmin transactionも署名する案は、Bridge侵害時にBase側の安全制限も失うため不採用とする。
- SNS Governance canisterが直接EVM transactionを構築・署名する案は、標準SNS Governanceの責務と実装範囲を変更するため不採用とする。
- SNS Governanceを唯一の承認主体とし、専用Executorを技術的adapterとして使用する案を採用する。

## Consequences

- Governance Executorのvalidate methodとexecute methodをSNS custom proposalとして登録する。
- execute methodはcallerが設定済みSNS Governance principalと一致する場合だけ処理する。匿名callerとoperational Bridge canisterを拒否する。
- Governance ExecutorはSNS Rootの管理下へ置く。
- Base contractの`DEFAULT_ADMIN_ROLE`はGovernance Executor専用のthreshold ECDSA addressが保持する。
- Governance Executorが送信できるtarget、chain ID、contract address、function selectorを固定allowlistへ限定し、任意calldata転送を許可しない。
- operational Bridge canisterとGovernance Executorはcanister principalが異なるため、相互のthreshold ECDSA addressとして署名できない。
- Verusでcaller認可、allowlist、proposal payload検証、admin transaction生成の対応関係を証明する。
- SNS Governanceは権限主体、Governance Executorは実行adapterであり、独立したadmin組織やmultisigを導入しない。
