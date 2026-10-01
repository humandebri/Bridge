# Security

## Reporting a vulnerability

Do not disclose an exploitable vulnerability, attack instructions, or credentials in a public issue or pull request.

Submit a confidential report through [GitHub private vulnerability reporting](https://github.com/humandebri/Bridge/security/advisories/new). Reporting is enabled for this repository as of October 1, 2026; a GitHub account is required. Do not use public issues for security reports.

This repository does not currently document a dedicated security email or a response-time commitment. The publication audit verified the setting and report-entry endpoint without submitting a test report; notification delivery and maintainer response have not been tested.

Include the affected revision and component, preconditions, reproduction steps, observed impact, and a suggested fix if available. Sanitize evidence and use a local or test environment; do not test attacks against production or move real assets without explicit authorization.

## Scope and evidence

The Bridge includes IC Canisters, Base contracts, the UI, and operational tooling. The [verification overview](verification/README.md) and [claim ledger](verification/claims.tsv) describe proof scope, implementation links, and external assumptions. Formal verification is evidence for the registered claims; it does not eliminate provider, governance, platform, or operational assumptions.

A dated audit or passing test does not establish that a different revision or current deployment has the same properties. Production authorization requires authenticated current-state checks and the complete current-source validation required by [repository policy](AGENTS.md).
