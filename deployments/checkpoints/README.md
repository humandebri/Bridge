# 承認済みproduction checkpoint

`lb5i5-ziaaa-aaaar-qcgwq-cai-ddde10ae.json` はsource内の承認registryと同一のbyte列である。

- SHA-256: `ddde10ae0f73d3af735eee84d0d2619bc2c565bf4501a36f842983f7bed4498b`
- source revision: `16cd903af92878ffe13294cf1dc577550ba1340c`
- checkpoint終端module: `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`、schema v36
- 検証済みsuffix: `6192841b...`から`d48d4737...`、`bf047794...`への2 upgrade
- rotation入力evidence SHA-256: `a8bee156e29e4d30b6d430248c42e10657a221b4aa136fe85f3d55474fee0894`
- 現在のdeploy終端はこのcheckpointに後続receiptを加えて検証する。ファイル配置だけでは承認・rotationされず、`tools/bridge-profile/src/production_checkpoint.rs`のregistryと同一変更でなければならない。

公開principal、address、hash、型付き運用値だけを保持する。ECDSA key名・derivation pathは鍵そのものではない。秘密鍵、認証token、credential付きURL、ローカルpath、署名済みenvelopeは格納しない。移設時は全fieldを確認し、承認hashとの一致と既存の厳密parserによる検証を行う。整形だけでもhashが変わるため、このJSONを編集しない。

大きな署名済みupgrade evidenceはrepositoryへ追加せず、[配置手順](../README.md#production証跡のローカル配置)に従って監査保管する。旧`58a14e60...` checkpointはGit履歴とrotationされたinstall/receipt digest列で追跡する。
