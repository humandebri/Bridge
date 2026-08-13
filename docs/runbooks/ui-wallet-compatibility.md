# Bridge UI wallet compatibility runbook

Record one completed copy of this checklist for every write-enabled staging profile and release
candidate. The test must use the reviewed staging URL, real Chrome, the published Plug extension,
and the production OISY signer. Automated test adapters do not satisfy this gate.

## Environment evidence

- Date and tester:
- Git commit:
- Staging URL:
- Chrome version:
- Plug version:
- OISY version/build:
- Deployment profile environment and chain ID:
- Bridge canister ID and schema version:
- Bridge and bSNS addresses/runtime bytecode hashes:
- KINIC ledger and index canister IDs:

Attach the runtime-verification screen and record that every check is fresh and passing. Stop the
test if bridge controls become unavailable or any identifier differs from the reviewed profile.
The browser RPC must return the configured chain and a Finalized block number and hash. Browser
block timestamps do not gate writes: Deposit safety uses the Canister's quorum-backed Finalized
observation, while Withdrawal safety is enforced by the Base contract at execution time. The
60-second lifetime of a successfully fetched runtime or status result remains unchanged.

## Plug

- Connect Plug with only the KINIC ledger and Bridge canister in the whitelist.
- Confirm the displayed Principal and default subaccount match Plug immediately before approval.
- Approve the exact gross amount plus transfer-from fee with current allowance and 30-minute expiry.
- Confirm Plug displays the Bridge canister's ICRC-21 message, including Base recipient, amounts,
  maximum service fee, minimum received amount, and the bSNS governance disclosure.
- Submit a deposit and verify canister history and the Finalized Base mint.
- Reject one approval and one Bridge call; verify no subsequent step is sent.
- Close one wallet popup; verify the UI reports cancellation without inventing success.
- Disconnect/reconnect and change account once; verify the stale confirmation is invalidated.

## OISY

- Connect OISY and select a non-default ICRC subaccount when available.
- Confirm the selected owner/subaccount remains the approval and deposit source.
- Approve the exact gross amount plus transfer-from fee with current allowance and 30-minute expiry.
- Confirm OISY displays the Bridge canister's ICRC-21 message and the bSNS governance disclosure.
- Keep an OISY approval or Bridge confirmation open for at least 10 seconds, then approve it and
  confirm the same popup completes approval and deposit without a false signer disconnect.
- Submit a deposit and verify canister history and the Finalized Base mint.
- Reject one approval and one Bridge call, close one popup, then reconnect.
- Change the selected account after confirmation; verify the transaction is aborted and must be
  reviewed again.

## Base wallets

- With MetaMask and Rabby installed, verify both appear by name and logo and the generic browser
  wallet choice is hidden.
- Connect each detected wallet in turn and confirm the header shows the selected wallet logo,
  name, and shortened address.
- Open WalletConnect, scan the QR code on a mobile wallet, approve Base or Base Sepolia, and
  confirm the same connected-wallet summary is shown.
- Reject one browser-wallet request and one WalletConnect request; verify the selection dialog
  remains usable and no transaction is submitted.
- Disconnect and reconnect each connection type, then change the active account and Base chain;
  verify the bridge revalidates both immediately.
- Confirm the browser reports no Content-Security-Policy violations during WalletConnect pairing,
  signing, or disconnect.

## Shared failure checks

- Change the Base chain and verify approve/deposit/withdraw remain disabled.
- Disconnect the Base wallet after confirmation and verify submission is aborted.
- Verify an altered runtime bytecode hash, contract address, ledger metadata, or public canister
  config makes bridge controls unavailable.
- Retry an indeterminate deposit with the same `owner_sequence` and identical payload; verify one
  history entry. Alter the payload with the same sequence and verify `DepositConflict`; use a skipped
  or stale sequence and verify `SequenceMismatch`.
- After approve succeeds but deposit fails, record the remaining allowance, expiry, and retry path.
- Force one withdrawal notification failure and verify a later explicit History refresh reconstructs
  the Finalized event and exposes `Check status` again.
- Reload the page and verify both wallet summaries remain connected without a new IC wallet prompt;
  verify a saved retryable withdrawal notification resumes without reopening either wallet.
- Start a withdrawal and verify the reviewed IC recipient is submitted to Base without reopening OISY.
- After Base finality, verify the browser notification Identity submits exactly one
  `notify_withdrawal` update without an OISY/Plug prompt or ICRC-21 consent call.
- Select `Check status` and verify one receipt check and at most one notification call are made.
- After successful ingestion, close the wallet and browser; verify canister timers complete the IC
  release without another Base transaction or wallet prompt. Reopen History and verify
  `Confirming automatically` is shown only while scheduled, and `Retry settlement` only after a stop.

## Result

- Outcome: `PASS` / `FAIL`
- Evidence links:
- Deposit operation/Ledger/Base references for Plug and OISY:
- Withdrawal operation/Ledger/Base references for MetaMask and Rabby:
- Same-Wasm before/after state SHA-256 and `storage_integrity_check()` result:
- Deviations or browser/extension issues:
- Reviewer and review date:

A `FAIL`, missing evidence, or incomplete runtime profile blocks the release.
