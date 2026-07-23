use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn happened_transfer_leaves_world_unchanged()
    ensures kernel::ambiguous_outbound_world_spec(true, 100, 10, 30, 15, 2, 3)
        == (100int, 10int, 30int) {} }
fn main() {}
