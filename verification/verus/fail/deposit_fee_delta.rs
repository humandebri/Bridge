use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn deposit_retry_charges_fee()
    ensures kernel::deposit_fee_delta_spec(3, 4, 7) == 7 {} }
fn main() {}
