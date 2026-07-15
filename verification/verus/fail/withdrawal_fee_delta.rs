use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn withdrawal_retry_charges_fee()
    ensures kernel::withdrawal_fee_delta_spec(3, 3, 7) == 7 {} }
fn main() {}
