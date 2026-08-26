use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn unauthenticated_caller_is_allowed() -> (result: kernel::DepositContinuationDecision)
    ensures result == kernel::DepositContinuationDecision::Allow
{
    kernel::deposit_continuation_decision(false, true, true)
}
}
fn main() {}
