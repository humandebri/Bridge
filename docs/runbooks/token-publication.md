# KINIC token publication

This runbook publishes the deployed KINIC ERC-20 on Base mainnet to BaseScan and registers official logo/project information. BaseScan submission/review does not authorize Gate A, Gate B, or activation and may occur after production asset admission begins.

## Fix the target

- `Bridge` manages Deposits, Withdrawals, pause, limits, fees, and roles.
- `BSNS` is the KINIC ERC-20 registered with wallets and BaseScan. Use `bsnsAddress` from the verified Gate B profile as the submission target, not the Bridge address.
- `BridgeTimelockController` delays administration operations that increase risk.
- Fix token metadata to `name = "KINIC"`, `symbol = "KINIC"`, and `decimals = 8`.

Use no guessed addresses, pre-deployment Gate A profiles with deployment block `0`, or staging profiles. Compare Gate B profile, Finalized `Bridge.bsns()`, and BSNS `bridge()` responses; stop submission if cross-references disagree.

## Pre-publication checks

1. Verify the following production UI URLs return `200` without authentication, cookies, or redirects.
   - `https://<official-domain>/kinic-token-logo.svg` (`Content-Type: image/svg+xml`)
   - `https://<official-domain>/kinic-token-logo-64.png` (`Content-Type: image/png`)
2. Verify SVG dimensions of 32×32 and transparent PNG dimensions of 64×64, rendering the same KINIC mark on light/dark backgrounds.
3. Finalize official website, project description, contact email, and full HTTPS social URLs. Use a neutral description without comparisons or exaggeration.
4. Store no private keys, seeds, keystore passwords, credentials, or credential-bearing RPC URLs in submission materials or the repository.

## Contract source verification

1. Verify source code on BaseScan in order: `BridgeTimelockController`, `Bridge`, `BSNS`.
2. Use repository-pinned Solidity compiler, optimizer, EVM target, and constructor arguments; confirm equality with deployed bytecode on BaseScan.
3. BSNS constructor arguments are `KINIC`, `KINIC`, `8`, and the verified Bridge address.
4. Confirm published source on all three Code pages and matching actual addresses/verification URLs. Follow [BaseScan's verification instructions](https://info.basescan.org/how-to-verify-contracts/) for source-verification prerequisites before Token Update.

## Ownership verification

`BSNS` is created by the Bridge constructor, not directly by an EOA. Do not assume a normal deployer EOA signature alone suffices. Follow [BaseScan's contract-created-by-contract guidance](https://info.basescan.org/what-is-contract-created-by-contract/), contacting support and proving involvement through the Bridge deployer or the signer specified by BaseScan.

Before signing, check the request domain, target BSNS address, BaseScan username, and timestamp. Never sign arbitrary messages not requested by BaseScan. Never enter private keys into web forms, support tickets, or the repository.

## Token Update submission

1. Submit exactly one Token Update for the ownership-verified BSNS address.
2. Enter metadata `KINIC`, `KINIC`, `8`, plus official website, complete social URLs, neutral description, and public SVG URL.
3. Verify logo, name, and symbol are approved KINIC brand assets and do not impersonate another project.
4. Recheck all fields before submission. Do not submit duplicates for the same address; answer additional-information requests in the original thread. [BaseScan Token Info Submission Guidelines](https://info.basescan.org/how-to-update-token-info/) are authoritative.

## Verify and record publication

On the BaseScan token page, check name, symbol, decimals, logo, website, social links, verified source, and `Add Token to MetaMask`. In wallets, select Base mainnet and verify the displayed contract address matches Gate B profile `bsnsAddress`.

Record only submission date, target BSNS address, three contract verification URLs, BaseScan reference number, and publication-verification date. Record no signed message, signature, private key, credentials, or personal information. If content is inaccurate, reply to the original submission requesting correction rather than submitting again.
