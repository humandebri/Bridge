---
status: superseded
---

# Base adminを即時停止と遅延回復の権限に分割する

このADRの人間wallet構成はPlan 006のSNS中心・Canister操作型権限モデルで置換された。現行構成は人間のBase Admin/Canceller walletを置かず、Bridge Canisterが別derivationからGovernance Operator、Runtime Administrator、Independent Cancellerを生成する。

Base contractのMint limitとwindow長はdeploy時に固定する。Runtime Administratorはpauseと上限内Service Fee変更を即時実行できる。unpauseとrole rotationはtimelockを経由するBase Admin hardware walletだけが実行でき、cancellerは独立hardware walletへ分離する。本決定はADR 0007をsupersedeし、Governance Executorを導入しない。

## Considered Options

- SNS Governanceを承認主体としGovernance Executorをadapterとする案（ADR 0007）は復活させない。ICP側にEVM署名用の専用canisterと固定allowlistの維持を要求し、実装と監査の範囲が管理操作の頻度に見合わないためである。
- adminを置かずBase contractの全パラメータとroleをimmutableにする案は、bridge signerのrotationが不可能になり、canister reinstallでthreshold ECDSA addressが変わった時点でBridgeが恒久停止するため不採用とする。
- 単一のadmin鍵からBridgeを直接管理する案は、鍵漏洩時にunpauseとrole rotationが即時実行されるため不採用とする。
- operational Bridge canisterがadmin transactionも署名する案は、ADR 0007が却下した理由（Bridge侵害時にBase側の安全制限も失う）が引き続き有効であり不採用とする。
- 即時pauseとService Fee変更をRuntime Administrator、遅延回復をtimelock付きBase Admin walletへ分ける案を採用する。

## Consequences

- Runtime AdministratorのBase側権限はpauseと`MAX_SERVICE_FEE`以内のService Fee変更に限定する。
- Base Adminのunpauseとrole rotationはtimelock（初期値72時間）の待機を経て実行する。
- Base Admin walletはtimelockのproposerとexecutorを担い、cancellerは独立hardware walletだけが担う。timelock遅延の短縮は拒否し、構築後のTimelock role集合は凍結する。role変更は新Timelockの配置とBridge rotationで行う。
- Per-Deposit Limit、Mint Throughput Limit、window長を変更するselectorは公開しない。
- Base Admin walletはSNS Governanceの外に立つ運用主体である。ICP側の信頼主体（ADR 0008のSNS Governance）と対称でないことをUIと文書で明示する。
- `MAX_SERVICE_FEE`などimmutableと定めた値はBase Adminでも変更できない。
- Base Adminはmint、refund、escrow資産への権限を持たない。
- Base Admin Timelockの具体構成はADR 0016に従う。
