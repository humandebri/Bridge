use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn duplicate_confirmed_hash_is_accepted()
    ensures kernel::confirmed_activation_attempt_is_unique_spec(true, true)
{}
}
fn main() {}
