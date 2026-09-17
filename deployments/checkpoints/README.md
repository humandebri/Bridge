# Approved production checkpoint

`lb5i5-ziaaa-aaaar-qcgwq-cai-ddde10ae.json` is byte-identical to the approval registry in source.

- SHA-256: `ddde10ae0f73d3af735eee84d0d2619bc2c565bf4501a36f842983f7bed4498b`
- source revision: `16cd903af92878ffe13294cf1dc577550ba1340c`
- Checkpoint terminal module: `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`, schema v36.
- Verified suffix: two upgrades from `6192841b...` to `d48d4737...`, then `bf047794...`.
- Rotation input evidence SHA-256: `a8bee156e29e4d30b6d430248c42e10657a221b4aa136fe85f3d55474fee0894`.
- Verify the current deployment terminal by adding subsequent receipts to this checkpoint. File placement alone does not approve or rotate it; it must change together with the registry in `tools/bridge-profile/src/production_checkpoint.rs`.

Store only public principals, addresses, hashes, and typed operating values. ECDSA key names and derivation paths are not keys themselves. Store no private keys, authentication tokens, credential-bearing URLs, local paths, or signed envelopes. When relocating, inspect every field, verify the approved hash, and validate with the existing strict parser. Do not edit this JSON: formatting alone changes its hash.

Keep large signed upgrade evidence outside the repository and archive it for audit following the [placement procedure](../README.md#local-production-evidence-layout). Track the old `58a14e60...` checkpoint through Git history and the rotated install/receipt digest sequence.
