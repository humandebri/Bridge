use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn payout_can_exceed_confirmed_fees()
    ensures kernel::payout_reserved_spec(9, 10) {} }
fn main() {}
