# Production upgrade checkpoint implementation status

The checkpoint migration is **not yet enabled for production**. The existing
upgrade and UI drivers still require their historical evidence. Do not remove
or make those archives unavailable in a real deployment.

Production cutover order: use the existing historical-evidence upgrade driver
to reach v36 from the deployed v35, generate and review the resulting checkpoint,
and pin its approved hash in a separate commit. Only then switch the production
upgrade and UI drivers to checkpoint evidence. An empty approval registry must
not become a prerequisite for the first v35-to-v36 upgrade.

Implemented foundations:

- `validate-production-checkpoint-candidate FILE` checks the typed candidate
  document and prints its byte-level SHA-256 with an `unapproved` label. It does
  not validate the original historical signatures or grant approval.
- `validate-production-checkpoint FILE` additionally requires the exact active
  source-controlled hash for the canister and deployment instance. The registry
  is currently empty; therefore no candidate is approved.
- Candidate input is bounded to 1 MiB, including during file reading. Unknown
  and duplicate fields are rejected recursively through typed deserialization.
- Legacy receipt validation now delegates to a shared iterator-based state
  machine. Runtime, module, signature, chunk, schema and replay validation stay
  in the same loop. The existing legacy chain limits have not changed.

Still required before enabling the contract:

1. Finish the continuation data contract, including initial-install replay
   identity, historical profile and live activation validation inputs.
2. Generate candidates and audit manifests from bounded, ordered receipt files;
   verify source ancestry and the actual archived production history.
3. Implement checkpoint plus suffix evidence, including empty suffix, rotation,
   and cross-checkpoint replay/chronology checks.
4. Connect upgrade preflight/execute/recover and UI rendering/live validation;
   bind frozen evidence and eliminate archive reads in those runtime paths.
5. Pass archive-inaccessible acceptance fixtures, review the actual candidate,
   and pin its exact hash in a separate approval commit.
6. Complete impacted and final clean-source release validation and assemble
   deployment artifacts. No production execution is authorized by these commands.

Checkpoint approval will attest to a reviewed historical validation result. It
will not replace current-source proof receipts, live canister validation, or
post-upgrade UI authorization. Formal historical receipts remain audit records.
