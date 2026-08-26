# Certora advisory verification

This directory contains advisory CVL proofs for the deployed Solidity contract
boundary. They complement the release-required SMTChecker, Halmos, Foundry,
Lean, and Verus evidence; they are not a stage in the schema-v7 proof receipt.

## Pinned environment

- `certora-cli==8.17.1` from `uv.lock`
- Certora Prover `release/15June2026`
- Solidity `0.8.36`
- EVM `prague`, via-IR enabled, optimizer enabled with 200 runs
- Solidity 0.8.36's reviewed `DefaultYulOptimiserSteps` is explicit because
  certora-cli 8.17.1 only has built-in step tables through solc 0.8.35
- production `DeploymentPolicy` remapping

Install the locked client and compiler:

```sh
uv sync --project verification/certora --frozen
scripts/install-certora-solc.sh
```

Build a fresh Solidity AST, validate the tracked evidence, and compile all three
CVL jobs without submitting source to the cloud:

```sh
forge build --root contracts
python3 scripts/check_certora_manifest.py
scripts/run_certora_advisory.sh compile all
```

Run one private cloud job after exporting `CERTORAKEY`:

```sh
scripts/run_certora_advisory.sh cloud bridge
scripts/run_certora_advisory.sh cloud bsns
scripts/run_certora_advisory.sh cloud timelock
```

Results are written below `verification/output/certora/`, which is ignored by
Git. Cloud PASS is derived from the private machine-readable rule results, not
the CLI exit code. The runner rejects leaked anonymous keys and checks that the
dedicated `certora-advisory-v1` fingerprint is unchanged during each job. That
fingerprint covers the CVL/config/tooling inputs, production Solidity and its
OpenZeppelin implementation, the shared Timelock transaction fixture, and the
claim/assumption ledgers. Certora-only changes do not stale the schema-v7 release
proof receipt; production Solidity, shared tests, and release-gate policy remain
in the release fingerprint.

For local or trusted-PR structural validation without cloud credentials, run:

```sh
scripts/ci-local.sh certora
```

Certora-only paths select this advisory CI area. Production Solidity and the
shared Timelock Foundry test continue to select the existing contracts/proofs
areas. The complete `main` repository gate remains unchanged.

## Assurance boundary

`Bridge.spec` links the immutable `Bridge.bsns` reference to the real `BSNS`
implementation. `ECDSA.tryRecoverCalldata` is summarized as a nondeterministic,
side-effect-free authentication boundary. Ghost state records the summary call,
recovered signer, and recovery error so the rejecting-path witness is tied to the
production signature check rather than a later mint-policy rejection. Successful
and rejecting paths remain nontrivial, while elliptic-curve correctness stays
outside the theorem under the registered `cryptographic_authenticity` assumption.

CVL opcode hooks can observe event topics but cannot read the unindexed memory
payload of `LOG` instructions. The Certora mint and withdrawal rules therefore
check the indexed event identity together with exact storage and token-supply
deltas. Exact unindexed `DepositMinted` amount equality remains covered by the
release-required Halmos `mint_commit_supply` obligation.

CVL 8.17.1 also cannot materialize a dynamic `bytes` value read directly from
contract storage. The withdrawal rule checks the record's fixed-width fields,
existence, burn, identifier, counter, and rollback, while exact persistence of
`Withdrawal.owner` remains covered by the existing Foundry transaction tests.
No production getter or verification harness is added to bypass this boundary.

The timelock counter proof observes each `_timestamps` lifecycle transition and
each counter storage write. It therefore permits an `execute` target to schedule
a nested operation: the outer call can have net delta zero while both operation
updates remain individually accounted for. A verification-only target and a
Foundry transaction test exercise that role-holding contract case.

The tracked Prover branch is a bootstrap candidate, not accepted evidence. A
private bootstrap run must expose one exact backend commit SHA in job metadata;
the follow-up pin replaces the branch with `commit_sha1` and reruns all targets
before any result is called accepted.

## Promotion criteria

Do not add Certora to `REQUIRED_STAGES` until all three jobs pass three times on
the same pinned environment, each job completes within 30 minutes and the
matrix within 45 minutes, and seven negative fixtures are killed: mint replay,
mint supply delta, withdrawal atomicity, authorization nonce, frozen timelock
roles, pending-operation count, and delay bounds. Promotion must add a
`certora-and-negative` stage, bump the receipt and claim schemas, register the
mutants, and fail closed on timeout, unknown, vacuity, unresolved assertion
dependencies, or cloud failure.
