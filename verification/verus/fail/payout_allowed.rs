use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn excess_payout_allowed() ensures kernel::payout_allowed_spec(9, 0, 7, 3) {} }
fn main() {}
