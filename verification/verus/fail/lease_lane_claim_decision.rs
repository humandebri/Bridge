use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn active_automatic_target_is_claimed() -> (result: kernel::LeaseLaneClaimDecision)
    ensures result == kernel::LeaseLaneClaimDecision::Allow
{
    kernel::lease_lane_claim_decision(true, true, 0, 4)
}
}
fn main() {}
