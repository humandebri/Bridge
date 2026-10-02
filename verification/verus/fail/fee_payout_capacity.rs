use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn unsafe_payout_is_allowed() ensures kernel::fee_payout_capacity_spec(100, 0, 1) == 100 {} }
fn main() {}
