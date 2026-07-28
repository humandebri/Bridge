use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn terminal_deposit_allows_pull() ensures kernel::deposit_phase_allows_spec(10, 0) {} }
fn main() {}
