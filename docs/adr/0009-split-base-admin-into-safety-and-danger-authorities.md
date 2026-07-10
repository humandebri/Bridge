---
status: accepted
---

# Base adminを安全方向と危険方向の権限に分割する

Base contractの管理操作を安全方向と危険方向に分け、権限主体を分離する。pauseとlimitの引き下げはRuntime Administratorが即時実行できる。unpause、limitの引き上げ、role rotationはtimelockを経由するBase Admin（Safe multisig）だけが実行できる。本決定はADR 0007をsupersedeし、Governance Executorを導入しない。

## Considered Options

- SNS Governanceを承認主体としGovernance Executorをadapterとする案（ADR 0007）は復活させない。ICP側にEVM署名用の専用canisterと固定allowlistの維持を要求し、実装と監査の範囲が管理操作の頻度に見合わないためである。
- adminを置かずBase contractの全パラメータとroleをimmutableにする案は、bridge signerのrotationが不可能になり、canister reinstallでthreshold ECDSA addressが変わった時点でBridgeが恒久停止するため不採用とする。
- 単一のadmin鍵へ全操作を集約する案は、鍵漏洩時にunpauseとlimit引き上げまで即時実行されるため不採用とする。
- operational Bridge canisterがadmin transactionも署名する案は、ADR 0007が却下した理由（Bridge侵害時にBase側の安全制限も失う）が引き続き有効であり不採用とする。
- 安全方向を高速パス、危険方向をtimelock付きmultisigとする分割案を採用する。

## Consequences

- Runtime AdministratorのBase側権限はpauseとlimitの引き下げに限定する。この鍵の漏洩による被害は停止と制限強化にとどまり、資産流出に至らない。
- Base Adminの操作はtimelock（初期値72時間）の待機を経て実行される。待機中に不審なqueueを検知した場合、Runtime Administratorがpauseして被害を防ぐ。timelock遅延が事実上の拒否権ウィンドウとして機能する。
- timelock遅延の短縮とBase Admin signerの変更自体もtimelockを経由する。
- Base AdminはSNS Governanceの外に立つ運用組織への信頼追加である。ICP側の信頼主体（ADR 0008のSNS Governance）と対称でないことを受け入れ、UIと文書で明示する。
- `MAX_SERVICE_FEE`などimmutableと定めた値はBase Adminでも変更できない。
- Base Adminはmint、refund、escrow資産への権限を持たない。
