# KINIC token publication

このrunbookは、Base mainnetへ配置済みのKINIC ERC-20をBaseScanへ公開し、公式ロゴとproject情報を登録する手順である。BaseScanの申請と審査はGate A、Gate B、activationの認可条件ではなく、本番資産受付の開始後に実施してよい。

## 対象の固定

- `Bridge`はdeposit、withdrawal、pause、limit、fee、roleを管理するcontractである。
- `BSNS`はウォレットとBaseScanへ登録するKINIC ERC-20 contractである。申請対象アドレスにはBridgeではなく、検証済みGate B profileの`bsnsAddress`を使う。
- `BridgeTimelockController`は危険方向の管理操作を遅延させるcontractである。
- token metadataは`name = "KINIC"`、`symbol = "KINIC"`、`decimals = 8`に固定する。

推測したアドレス、Gate Aのdeployment blockが`0`のpre-deploy profile、staging profileは使用しない。Gate B profile、`Bridge.bsns()`のFinalized応答、BSNSの`bridge()`応答を照合し、相互参照が一致しなければ申請を停止する。

## 公開前確認

1. production UIから次のURLが認証、cookie、redirectなしで`200`を返すことを確認する。
   - `https://<official-domain>/kinic-token-logo.svg`（`Content-Type: image/svg+xml`）
   - `https://<official-domain>/kinic-token-logo-64.png`（`Content-Type: image/png`）
2. SVGが32×32、PNGが透過64×64であり、明暗両背景で同じKINIC markとして表示されることを確認する。
3. official website、project description、contact email、SNSの完全なHTTPS URLを確定する。descriptionは比較表現や誇張を含まない中立文とする。
4. 秘密鍵、seed、keystore password、credential、credential入りRPC URLを申請資料またはrepositoryへ保存しない。

## Contract source verification

1. BaseScanで`BridgeTimelockController`、`Bridge`、`BSNS`の順にsource codeをverifyする。
2. repositoryで固定したSolidity compiler、optimizer、EVM target、constructor argumentsを使用し、BaseScan上のdeployed bytecodeとの一致を確認する。
3. BSNSのconstructor argumentsには`KINIC`、`KINIC`、`8`、検証済みBridge addressを使う。
4. 3 contractすべてのCode画面でsourceが公開され、実アドレスと検証URLが対応していることを確認する。Token Update申請前のsource verification要件は[BaseScanの説明](https://info.basescan.org/how-to-verify-contracts/)に従う。

## Ownership verification

`BSNS`はEOAが直接配置せず、Bridge constructorが生成する。そのため通常のdeployer EOA署名だけで完了すると仮定しない。[BaseScanのcontract-created-by-contract案内](https://info.basescan.org/what-is-contract-created-by-contract/)に従ってBaseScan supportへ連絡し、Bridge deployerまたはBaseScanが指定した主体による署名で関与を証明する。

署名要求のdomain、対象BSNS address、BaseScan username、日時を署名前に確認する。BaseScanが要求していない任意messageへ署名しない。秘密鍵をWebフォーム、support ticket、repositoryへ入力しない。

## Token Update申請

1. ownership確認済みのBSNS addressからToken Updateを1件だけ提出する。
2. metadataへ`KINIC`、`KINIC`、`8`を入力し、公式website、完全なSNS URL、中立なdescription、公開SVG URLを指定する。
3. ロゴ、名称、symbolが第三者projectを偽装せず、KINICの承認済みbrand assetであることを確認する。
4. 送信前に全項目を再確認する。申請後は同じaddressへ重複申請せず、追加情報の要求には元の申請threadで回答する。[BaseScan Token Info Submission Guidelines](https://info.basescan.org/how-to-update-token-info/)を正本とする。

## 掲載確認と記録

BaseScanのtoken pageで名称、symbol、decimals、ロゴ、website、SNS、verified source、`Add Token to MetaMask`導線を確認する。ウォレット側ではBase mainnetを選択し、表示されたcontract addressがGate B profileの`bsnsAddress`と一致することを確認する。

運用記録に残す値は、申請日、対象BSNS address、3 contractのverification URL、BaseScan受付番号、掲載確認日だけとする。署名本文、signature、private key、credential、個人情報は記録しない。掲載内容が不正確な場合は新規申請を重ねず、元の申請に返信して訂正を依頼する。
