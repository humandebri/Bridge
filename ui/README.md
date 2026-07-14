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

The checked-in `base-sepolia-preflight` profile is intentionally incomplete and read-only.
Do not enable writes until every canister/contract identifier, deployment block, and runtime
bytecode hash is reviewed and committed. At runtime the UI also verifies wallet chain, contract
bytecode, `Bridge.bsns()`, token metadata, `get_public_config`, and schema version. Any failure
locks approve, deposit, and withdrawal controls.

Write controls are also locked unless `window.location.origin` exactly matches a checked-in
`allowedOrigins` entry. This and the CSP reduce accidental use from copied frontends but are not
canister authorization: direct canister calls remain possible.

OISY and Plug are the only supported IC wallets. Internet Identity and delegated browser
identities are not used. Deposit history is read from the public canister index; anyone who knows
an owner Principal can enumerate its deposit IDs and correlate them with the Base recipients in
the corresponding deposit records.

After a Base withdrawal reaches the finalized head, the connected IC wallet calls
`notify_withdrawal`. Pending notification v2 records retain the transaction hash, IC owner and
subaccount, Base requester, chain ID, and Bridge address in browser local storage. They are retried
only when every saved value matches the active wallets and deployment. Legacy v1 records are
discarded without migration. There is intentionally no periodic canister-side discovery
fallback, so a withdrawal that is never notified remains pending on Base.

`pnpm run e2e:real` downloads checksum-pinned DFINITY Ledger/Index Wasm artifacts, then starts an
actual ICRC-1/2 Ledger and Bridge canister in PocketIC plus the real Bridge/bSNS contracts in
Anvil. Its test-only Vite aliases are confined to `e2e-real/`; production adapters and builds do
not contain an E2E branch.

Deployment is manual:

```sh
pnpm run deploy
```

Complete the wallet compatibility checklist in
[`../docs/runbooks/ui-wallet-compatibility.md`](../docs/runbooks/ui-wallet-compatibility.md)
before enabling writes or publishing a release candidate.
