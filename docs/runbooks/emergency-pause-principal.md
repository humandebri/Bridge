# Emergency pause principal運用

永続的な人間管理資格情報は、Bridge Canisterの安全操作だけを呼べる単一のIC hardware identityである。release approver、IC finance principal、人間のEVM管理鍵は使用しない。

1. emergency pause用hardware identityを生成し、SNS Governance、fee recipient、controllerと異なるprincipalであることを確認する。
2. test canisterでIC pause、Base両flow pause、記録済みpending Timelock operationのcancelを演習する。
3. `monitor-drill.json`へprincipal、実request ID、audit sequence、audit digest、5/15/60時刻を記録する。秘密、seed、device backupは記録しない。
4. rotationはKINIC SNS Governanceの固定generic functionだけから実行する。旧principalはrotation完了後に権限を持たない。

Mint SignerとGovernance OperatorはBridge Canisterが別derivation pathから導出する。Mint SignerはEIP-712署名だけを行いETHを必要としない。ETH補充はGovernance Operatorへの送金だけを許可し、送金元へcontract roleを付与しない。
