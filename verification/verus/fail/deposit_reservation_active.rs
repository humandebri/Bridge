use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn minted_deposit_still_reserves()
    ensures kernel::deposit_reservation_active_spec(10) {} }
fn main() {}
