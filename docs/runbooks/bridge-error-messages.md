# Bridge request errors

All user-facing messages are English. Error presentation must not authorize a
transaction, clear an unresolved intent, or infer acceptance from wallet text.

## Consent admission

The existing ICRC-21 `GenericError { error_code, description }` shape is unchanged.
These are application error codes, not HTTP status codes. No stable layout change
is required. Only an actual consent quota rejection uses code 429.

| Code | Meaning | Action |
| --- | --- | --- |
| 429 | Consent quota exhausted | Wait for the returned retry interval; it is measured when the Canister rejected the request. |
| 4601 | Available operating cycles do not exceed the required reserve | Operator investigates and tops up the execution balance if needed; not the user's wallet. |
| 4602 | Lifecycle does not permit the request | Check Bridge Status. |
| 4603 | Configuration cannot be loaded | Operator investigates configuration access. |
| 4604 | Internal storage operation failed | Operator investigates storage; do not label it a rate limit. |
| 4605 | Operating budget calculation failed | Operator investigates arithmetic or liability inputs; do not label it low cycles. |

The deployed v35 generic `Consent message capacity is temporarily unavailable.`
does not identify a cause. Until the Canister is upgraded, the UI must preserve
that uncertainty rather than interpreting code 429 alone as quota exhaustion.
This display mapping is not a fallback in production evidence or schema validation.

## Previous deposit outcome is unconfirmed

A saved request can exist before the wallet finishes consent or submission.
Reopening the page is not proof of failure or acceptance. New deposits remain
blocked until the existing owner-sequence and canonical-record checks resolve
the previous request. Retrying the same saved request is a distinct operation.

`Check previous deposit` only reads the previous request's status. It never
approves tokens or submits a transfer. On acceptance, recover the matching record;
on a mismatch or failed read, retain the protection. A prior token approval may
still be active even if deposit submission failed.

## Release boundary

The clearer unresolved-deposit UI can ship independently. Reason-specific consent
responses require a separately approved Canister upgrade. Do not bundle the
undeployed v36 changes into a v35 production hotfix without a reviewed upgrade
plan. Neither this message change nor a passing UI preflight proves the original
429 incident is fixed. Verify the actual OISY consent flow after approval to release.
