---
status: accepted
---

# Bridge constructorでbSNSを生成する

Bridge contractはconstructor内でbSNSを生成し、bSNSの供給操作権限を生成元Bridgeへimmutableに固定する。個別deploy後のinitializerは一時的な未設定状態を作り、予測address方式はdeploy手順へ不要な循環依存を持ち込むため採用しない。
