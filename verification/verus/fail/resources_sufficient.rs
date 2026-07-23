use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn insufficient_eth_is_sufficient() ensures kernel::resources_sufficient_spec(9, 10, 20, 20) {} }
fn main() {}
