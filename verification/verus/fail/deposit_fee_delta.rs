use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn deposit_refund_charges_service_fee()
    ensures kernel::deposit_fee_delta_spec(6, 5, 7) == 7 {} }
fn main() {}
