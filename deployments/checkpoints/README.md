# 承認済みproduction checkpoint

`lb5i5-ziaaa-aaaar-qcgwq-cai-58a14e60.json` はsource内の承認registryと同一のbyte列である。

- SHA-256: `58a14e603e51e9a6e00feaad1c5c2b4be965b8031b5300f7138224e207a890a9`
- 承認commit: `0f046dc`
- checkpoint終端module: `6192841b3c2b5c28c6decea307e7c8e23700ab9bb00e6e593d74857478563c75`、schema v36
- 現在のdeploy終端はこのcheckpointに後続receiptを加えて検証する。ファイル配置だけでは承認・rotationされない。

公開principal、address、hash、型付き運用値だけを保持する。ECDSA key名・derivation pathは鍵そのものではない。秘密鍵、認証token、credential付きURL、ローカルpath、署名済みenvelopeは格納しない。移設時は全fieldを確認し、承認hashとの一致と既存の厳密parserによる検証を行う。整形だけでもhashが変わるため、このJSONを編集しない。

大きな証跡・秘密を含み得る設定はここへ追加せず、[配置手順](../README.md#production証跡のローカル配置)に従う。
