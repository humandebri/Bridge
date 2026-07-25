# KINIC Bridge UI

React/Vite SPA for the KINIC 1:1 bridge between the Internet Computer and Base.
Cloudflare Workers serves only the generated static assets; the UI has no server-side state,
database, KV namespace, or secret.

The primary `/` route combines both directions and stores the selected flow in
`?direction=deposit|withdraw`. Wallet-specific activity is available at `/history`, while
reviewed runtime and settlement evidence is isolated at `/status`. There are intentionally no
legacy `/deposit` or `/withdraw` routes because this UI has not been deployed to production.

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
bytecode, `Bridge.bsns()`, the quorum-confirmed Finalized Bridge signer, token metadata, `get_public_config`, and schema version. Any failure
disables approve, deposit, and withdrawal controls.

The deployment profile has no manual read-only flag or origin allowlist. Controls become available
when the profile is complete and runtime verification passes. The CSP limits browser connections,
but it is not canister authorization: direct canister calls remain possible.

OISY Wallet and Plug are the only supported IC wallets. MetaMask is always offered for Base,
other Base wallets are discovered through EIP-6963, and Plug is excluded from the Base wallet list.
A production build also exposes WalletConnect when `VITE_WALLETCONNECT_PROJECT_ID` is configured.
The WalletConnect project must allowlist every deployed UI origin. Internet Identity
and delegated browser identities are not used. Deposit history is read from the public canister index; anyone who knows
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
observation. The update endpoint is globally rate-limited and single-flight so the first write after
deployment can establish an observation without turning refresh into an unbounded RPC path.
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
`notify_withdrawal` with the connected IC wallet. History reconstructs Finalized burns from
Base events and exposes `Check and notify` after a wallet rejection, reload, or RPC failure. No
recovery cursor is persisted in browser storage. The pending confirmation record stores
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

The WalletConnect project ID is public client configuration but must be injected through the
environment rather than committed. The normal deploy command fails unless it is present and the
checked-in UI profile explicitly sets
`testOnly: false`. Test-only profiles require the deliberately separate `pnpm run deploy:test`
command, which targets the `kinic-bridge-ui-test` Worker, and must not be used for a production release.

Complete the wallet compatibility checklist in
[`../docs/runbooks/ui-wallet-compatibility.md`](../docs/runbooks/ui-wallet-compatibility.md)
before enabling writes or publishing a release candidate.
