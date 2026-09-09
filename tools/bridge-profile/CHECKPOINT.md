# Production upgrade checkpoint implementation status

The checkpoint migration is **not yet enabled for production**. The existing
upgrade and UI drivers still require their historical evidence. Do not remove
or make those archives unavailable in a real deployment.

Production was queried on 2026-09-09: schema v36, module SHA-256
`6192841b3c2b5c28c6decea307e7c8e23700ab9bb00e6e593d74857478563c75`.
This release prepares a corrected v36 upgrade, not the initial v35-to-v36
migration. Generate and review the historical checkpoint, pin its approved hash
in a separate commit, then switch the production upgrade and UI drivers.

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

1. Review the typed continuation data and historical profile. Replay identities
   come from all historical upgrade receipts; the original Gate A install receipt
   does not contain its signed request identity, so no genesis ID is invented.
2. Run `generate-production-checkpoint-candidate INPUT OUTPUT AUDIT` against the
   actual archived history. INPUT supplies ordered raw receipt paths and the
   number of receipts before activation. Generation requires clean committed
   source and validates historical attestation freshness at receipt time, not
   against the current clock. It grants no live authorization.
3. Exercise `make-production-checkpoint-evidence CHECKPOINT OUTPUT [RECEIPT...]`
   and `rotate-production-checkpoint-candidate EVIDENCE OUTPUT AUDIT` after
   approval. Empty suffix, rotation, replay/chronology and source checks are
   implemented; neither command registers a trusted hash.
4. Connect upgrade preflight/execute/recover and UI rendering/live validation;
   bind frozen evidence and eliminate archive reads in those runtime paths.
5. Pass archive-inaccessible acceptance fixtures, review the actual candidate,
   and pin its exact hash in a separate approval commit.
6. Complete impacted and final clean-source release validation and assemble
deployment artifacts. No production execution is authorized by these commands.

Live activation attestation freshness remains five minutes. Refreshing it is a
production update call, not a read-only query; obtain execution authorization.

Checkpoint approval will attest to a reviewed historical validation result. It
will not replace current-source proof receipts, live canister validation, or
post-upgrade UI authorization. Formal historical receipts remain audit records.
