use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn missing_hold_evidence_resolves() -> (result: kernel::HoldResolutionDecision)
    ensures result == kernel::HoldResolutionDecision::ResolveAbsent
{
    kernel::hold_resolution_decision(false, false)
}
}
fn main() {}
