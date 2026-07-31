use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn active_job_decision_allows_manual_claim() -> (result: kernel::ManualClaimDecision)
    ensures result == kernel::ManualClaimDecision::Allow
{
    kernel::manual_claim_decision(false, true, false, false, false)
}
}
fn main() {}
