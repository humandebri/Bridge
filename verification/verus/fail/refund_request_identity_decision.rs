use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn owner_mismatch_is_allowed() -> (result: kernel::RefundRequestIdentityDecision)
    ensures result == kernel::RefundRequestIdentityDecision::Allow
{
    kernel::refund_request_identity_decision(true, Some(false))
}
}
fn main() {}
