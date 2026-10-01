# Contributing

Start with the [architecture](docs/architecture.md), [development guide](docs/development.md), and [repository instructions](AGENTS.md). Use English for documentation, comments, user-facing text, issues, and pull requests. Preserve exact API names, claim IDs, principals, addresses, hashes, and evidence identifiers.

## Development workflow

1. Install pinned tools and lockfile dependencies, including initialized submodules.
2. Keep a change focused and run the applicable fast checks.
3. Update callers, tests, fixtures, and documentation together when changing an undeployed API. Compatibility fallbacks require an explicit design decision.
4. Describe the problem, resulting behavior, validation results, and remaining limitations in the pull request.

Safety-related changes must identify affected claims and preserve the production-kernel refinement link. Follow `AGENTS.md` and [verification ownership](verification/proof-impact.tsv); run the manifest checks, impacted proof stages, and applicable negative, refinement, and transaction tests. An impacted receipt is not a complete production proof receipt.

Documentation changes should update affected links and distinguish current specifications from historical evidence. Update source registries and regenerate derived verification documentation instead of editing generated files directly.

## Production boundaries

Local tests use test Canisters and local chains. A contribution does not authorize production deployment, SNS proposal submission, funding, publication, or asset transfers. Production changes follow the operations runbooks and their explicit approval boundaries.

## Bugs and vulnerabilities

For ordinary bugs, include reproduction steps, tool versions, expected behavior, and sanitized logs in an issue. Do not include private keys, seed phrases, identity PEMs, authenticated RPC URLs, or private operational artifacts.

Report suspected vulnerabilities privately using [SECURITY.md](SECURITY.md).
