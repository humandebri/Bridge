# Bridge鍵ceremony

1. Base Admin用hardware walletを1本生成し、backup復元を別端末で確認する。seed、private key、backup内容はリポジトリやcanisterへ保存しない。
2. Base Admin walletだけを72時間Timelockのproposer、canceller、executorにし、Timelock自身以外のadminが0件であることを確認する。
3. Runtime Administrator用hardware keyを別に生成する。Base Admin wallet、Bridge threshold signer、IC finance principalと重複させない。
4. IC finance principalを別hardware identityから導出する。pause principalは別hardware identity 2件とpause-watchdog canister IDの合計3件にする。
5. 全address/principalを2名が独立照合し、`bridge-profile validate`のcanonical hashへ署名する。秘密情報を含まないhashと公開addressだけをceremony記録へ残す。
6. 紛失・侵害時は新規Depositをpauseする。Base Admin walletとBase roleのrotationはTimelock、finance/pause rotationはKINIC Governanceを経由する。
