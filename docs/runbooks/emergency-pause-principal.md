# Emergency pause principal運用

永続的な人間管理資格情報は、Bridge Canisterの安全操作だけを呼べる単一のIC hardware identityである。release approver、IC finance principal、人間のEVM管理鍵は使用しない。

1. emergency pause用hardware identityを生成し、SNS Governance、fee recipient、controllerと異なるprincipalであることを確認する。
2. 同じidentityから`emergency_pause`とgovernance relayerの`drain-emergency`を実行し、test canisterでIC pause、Base両flow pause、記録済みpending Timelock operationのcancelを演習する。
3. schema v4の`monitor-drill.json`へprincipal、実request ID、audit sequence、audit digest、障害起点・検知・確認・両側pauseの順序付き時刻を記録する。5/15/60は公開後の監視目標であり、演習の本番ゲートはpause/cancel経路の成功とする。秘密、seed、device backupは記録しない。
4. rotationはKINIC SNS Governanceの固定generic functionだけから実行する。旧principalはrotation完了後に権限を持たない。

Mint Signer、Governance Operator、Runtime Administrator、Independent CancellerはBridge Canisterがrole別derivationから導出する。Mint SignerはEIP-712署名だけを行いETHを必要としない。ETH補充はBase transactionを送信する各control-plane roleへの送金だけを許可し、送金元へcontract roleを付与しない。
pause principalはBase pauseと記録済みTimelock cancelの署名準備、署名成果物取得、明示replacement、Finalized確定通知だけを実行できる。Service Fee、activation schedule/execute、resumeは実行できない。
