#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn refund_can_start_without_policy()
    ensures kernel::refund_start_allowed_spec(true, false)
{}
}
fn main() {}
