# Security

## Reporting a vulnerability

Do not disclose an exploitable vulnerability, attack instructions, or credentials in a public issue or pull request.

If GitHub private vulnerability reporting is enabled, use the repository's Security tab to submit a private report. Otherwise, contact the repository maintainer through an established private channel to arrange a confidential report. This repository does not currently document a dedicated security email or a response-time commitment.

Include the affected revision and component, preconditions, reproduction steps, observed impact, and a suggested fix if available. Sanitize evidence and use a local or test environment; do not test attacks against production or move real assets without explicit authorization.

## Scope and evidence

The Bridge includes IC Canisters, Base contracts, the UI, and operational tooling. The [verification overview](verification/README.md) and [claim ledger](verification/claims.tsv) describe proof scope, implementation links, and external assumptions. Formal verification is evidence for the registered claims; it does not eliminate provider, governance, platform, or operational assumptions.

A dated audit or passing test does not establish that a different revision or current deployment has the same properties. Production authorization requires authenticated current-state checks and the complete current-source validation required by [repository policy](AGENTS.md).
