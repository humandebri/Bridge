use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn bootstrap_allows_asset_operations()
    ensures kernel::asset_operations_allowed_spec(false) {} }
fn main() {}
