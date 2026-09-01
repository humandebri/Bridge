# KINIC Bridge UI

React/Vite SPA for the KINIC 1:1 bridge between the Internet Computer and Base.
Cloudflare Workers serves only the generated static assets; the UI has no server-side state,
database, KV namespace, or secret.

The primary `/` route combines both directions and stores the selected flow in
`?direction=deposit|withdraw`. Wallet-specific activity is available at `/history`, while
reviewed runtime and settlement evidence is isolated at `/status`. There are intentionally no
legacy `/deposit` or `/withdraw` routes because this UI has not been deployed to production.

Production assets are built without an embedded deployment profile. The sorted per-file digest
receipt (`ui-assets.json`) is a mandatory Gate B artifact and is reproduced from the exact clean
source before activation and again before deployment. `deployment-profile.js` is excluded from
that generic code digest and is generated only from the UI runtime profile rendered by the
verified Gate B bundle. Production deployment rejects a dirty source tree, receipt drift, or a
runtime profile that differs from the reviewed release inputs.

## Requirements

- Node.js `24.14.0`
- pnpm `11.0.8`
- didc `0.5.4`

Install and verify:

```sh
pnpm install --frozen-lockfile
pnpm run codegen:abi:check
pnpm run codegen:candid:check
pnpm run typecheck
pnpm run lint
pnpm run test
pnpm run build
pnpm run e2e
pnpm run e2e:real
```

The checked-in `base-sepolia-preflight` profile is intentionally incomplete. Complete every
canister/contract identifier, expected Bridge signer, deployment block, and runtime bytecode hash before publishing a
working environment. At runtime the UI verifies wallet chain, contract
bytecode, `Bridge.bsns()`, the quorum-confirmed Finalized Bridge signer, token metadata, `get_runtime_binding`, and schema version immediately before a value-changing action. Any failure
disables approve, deposit, and withdrawal controls.

The deployment profile has no manual read-only flag or origin allowlist. Controls become available
when the profile is complete and runtime verification passes. The CSP limits browser connections,
but it is not canister authorization: direct canister calls remain possible.

OISY Wallet and Plug are the only supported IC wallets. MetaMask and other Base browser extensions
are discovered through EIP-6963, and Plug is excluded from the Base wallet list.
A production build also exposes WalletConnect when `VITE_WALLETCONNECT_PROJECT_ID` is configured.
The WalletConnect project must allowlist every deployed UI origin. Internet Identity
and delegated wallet identities are not used. The selected IC account is restored after a reload for
display and read-only access, but no signing authority is persisted: OISY reopens on the next explicit
wallet action. A withdrawal uses the IC recipient shown in its review dialog without reopening OISY before the Base transaction. After Base finality, the UI sends one `notify_withdrawal` update from a deployment-scoped Ed25519 browser identity; that identity can only consume the permissionless notification quota and has no owner, refund, or settlement authority. Restored
Plug requires an explicit first action and checks its current Principal before any write. Deposit history is read from the public canister index; anyone who knows
an owner Principal can enumerate its deposit IDs and correlate them with the Base recipients in
the corresponding deposit records. Withdrawal History scans Finalized Base logs in 5,000-block
chunks, with at most four RPC requests per refresh or manual `Scan older` action. The resumable
cursor is held only in the React Query cache and is not persisted in browser storage.

The bundled MetaMask fox icon comes from MetaMask's official
[Brand Assets](https://metamask.io/ja/assets) download.

The Bridge form reads a latest-state `Current bridge fee` quote. Before enabling a write, runtime
validation asks the Canister to refresh its quorum-backed Finalized observation and treats that observation
as authoritative. It requires the observed chain ID, Bridge signer, and runtime hash to match the
reviewed profile, verifies that the observed block is no newer than the browser RPC's Finalized head, and
binds every Base contract state and bytecode read to that canonical hash with EIP-1898. The
browser's single-RPC result is supplemental; it cannot make the form writable without the Canister
observation. The browser requires a Finalized block number and hash on the configured chain, but
does not use the block timestamp as a write-readiness gate. Deposit safety is revalidated by the
Canister's quorum-backed observation, and Withdrawal safety is enforced by the Base contract at
execution time. The update endpoint is globally rate-limited and single-flight so the first write
after deployment can establish an observation without turning refresh into an unbounded RPC path.
The open Bridge form performs the complete deployment validation once, then refreshes only the
Finalized head, fee guard, reviewed signer, current terms, and connected-wallet balances every
45 seconds while the tab is visible. Focus and reconnect trigger the same lightweight refresh.
Manual Refresh and every write-time gate still run the complete validation.

Deposit idempotency and recovery use the Canister's public owner sequence. `Refresh bridge data`
reads it once; the form does not generate a client request ID or persist a pending payload in
browser storage. While the Bridge page remains open, an uncertain response locks the exact owner
sequence, amount, wallets, and Base recipient. The user can retry only that same request, or confirm
that the sequence was not accepted before unlocking the form. Reloading clears this in-memory hint;
after a reload, the user explicitly refreshes History and the owner sequence before creating another
Deposit.

After a Base withdrawal reaches the Finalized head, the Bridge page automatically calls
`notify_withdrawal` once with the browser notification identity, without an IC wallet prompt or ICRC-21 consent call. A transport failure or `Busy` receives one five-second retry; other retryable failures stop automatic notification and expose `Retry IC notification` in Progress and History. `TransactionNotConfirmed` returns to Finalized-head monitoring and permits one further notification only after the head advances. History reconstructs Finalized burns from
Base events and exposes the saved failure reason and recovery action after a reload. The v7 pending confirmation record stores
the Base transaction hash, owner, settlement ID for deposits, and active deployment identifiers in
`localStorage`; it is scoped to the current Bridge deployment and contains no secret. There is
intentionally no periodic canister-side discovery fallback, so a withdrawal that is never notified
remains pending on Base. After ingestion, submitted EVM transactions are confirmed through the
frontend wallet flow and Canister Finalized revalidation; normal Ledger settlement is advanced by
Canister timers even when the browser is closed. History polls
queries every 60 seconds only while a visible record has an automatic check scheduled. `Retry
settlement` appears only after automatic progress has stopped; it never runs automatically in the UI.

`pnpm run e2e:real` downloads checksum-pinned DFINITY Ledger/Index Wasm artifacts, then starts an
actual ICRC-1/2 Ledger and Bridge canister in PocketIC plus the real Bridge/bSNS contracts in
Anvil. Its test-only Vite aliases are confined to `e2e-real/`; production adapters and builds do
not contain an E2E branch.

Deployment is manual:

```sh
VITE_WALLETCONNECT_PROJECT_ID=<reviewed-project-id> pnpm run deploy
```

Before Gate B exists, a separately reviewed fail-closed UI may be published with:

```sh
BRIDGE_UI_PREACTIVATION_RECEIPT=<clean-build-receipt> \
BRIDGE_UI_RUNTIME_PROFILE_FILE=<pre-activation-profile> \
pnpm run deploy:preactivation
```

Use `pnpm run deploy:preactivation:check` with the same variables for the non-mutating Wrangler
dry run immediately before requesting deployment approval.

This path accepts only a Base Mainnet production profile whose Gate B manifest hash is unset and
whose deployment block is zero. Runtime validation therefore disables all bridge writes. After
Gate B passes, replace it with the normal Gate-B-bound deployment above.

The WalletConnect project ID is public client configuration but must be injected through the
environment rather than committed. The normal deploy command fails unless it is present and the
checked-in UI profile explicitly sets
`testOnly: false`. Test-only profiles require the deliberately separate `pnpm run deploy:test`
command, which targets the `kinic-bridge-ui-test` Worker, and must not be used for a production release.

Complete the wallet compatibility checklist in
[`../docs/runbooks/ui-wallet-compatibility.md`](../docs/runbooks/ui-wallet-compatibility.md)
before enabling writes or publishing a release candidate.
