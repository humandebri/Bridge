# Bridge鍵ceremony

1. Base Admin用hardware walletを1本生成し、backup復元を別端末で確認する。seed、private key、backup内容はリポジトリやcanisterへ保存しない。
2. canceller専用hardware walletを別機種または別管理基盤で生成し、Base Admin、proposer、executor、Runtime Administrator、Bridge threshold signerのどれとも重複しないことを確認する。
3. Base Admin walletを72時間Timelockのproposerとexecutorにし、独立walletだけをcancellerにする。Timelock自身が`DEFAULT_ADMIN_ROLE`を保持し、それ以外のadminが0件であることを確認する。
4. Runtime Administrator用hardware keyを別に生成する。Base Admin wallet、canceller、Bridge threshold signer、IC finance principalと重複させない。
5. IC finance principalを別hardware identityから導出する。pause principalは別hardware identity 2件から導出する。
6. 全address/principal、公式EVM RPC Canister ID、監視演習証跡を照合し、専用release approver hardware walletからevidence bundleのcanonical manifest hashへ署名する。配下RPC providerの運営主体・基盤は監査対象にしない。秘密情報を含まないhashと公開情報だけをceremony記録へ残す。
7. 紛失・侵害時は新規Depositをpauseする。Bridge側のBase role rotationは現Timelockを経由する。Timelock proposer、executor、canceller自体の変更は、承認済み新role集合で新Timelockを配置し、現TimelockからBridge rotationを行う。finance/pause rotationはKINIC Governanceを経由する。
