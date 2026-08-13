use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn one_provider_is_enough()
    ensures kernel::withdrawal_finalized_checkpoint_spec(Some(102int), None, None) == Some(102int)
{}
}
fn main() {}
