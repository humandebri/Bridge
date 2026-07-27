use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn finalized_success_is_still_retry()
    ensures kernel::withdrawal_finalization_decision_spec(true, 10, Some(10)) == 0
{}
}
fn main() {}
