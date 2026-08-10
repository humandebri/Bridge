use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn anonymous_caller_is_allowed() -> (result: kernel::RefundRequestIdentityDecision)
    ensures result == kernel::RefundRequestIdentityDecision::Allow
{
    kernel::refund_request_identity_decision(false)
}
}
fn main() {}
