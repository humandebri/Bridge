use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn confirmed_returns_to_queued() ensures kernel::monotone_spec(3, 0) {} }
fn main() {}
