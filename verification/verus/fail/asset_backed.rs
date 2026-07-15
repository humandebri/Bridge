use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn unbacked_supply_is_backed()
    ensures kernel::asset_backed_spec(9, 10, 0, 0, 0) {} }
fn main() {}
