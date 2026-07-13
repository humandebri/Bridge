# AGENTS.md

## Pre-deployment compatibility policy

- This repository has not been deployed to production yet. Until the first production deployment, do not preserve backward compatibility for obsolete public APIs, configuration shapes, stable-memory schemas, fixtures, or upgrade paths.
- Prefer replacing pre-deployment formats directly and updating all callers, tests, fixtures, and documentation in the same change. Do not add legacy migrations, compatibility shims, dual-read paths, or fallbacks unless the user explicitly requests them.
- Unknown or obsolete stable schema versions must fail closed. Test upgrades and stable-memory reopen behavior only for the current schema unless an earlier schema has actually been deployed.
- Revisit and explicitly tighten this policy when the first production deployment is approved.
