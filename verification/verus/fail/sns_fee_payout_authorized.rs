use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn unsafe_payout_is_allowed() ensures kernel::sns_fee_payout_authorized_spec(false,true,true,true,true) {} }
fn main() {}
