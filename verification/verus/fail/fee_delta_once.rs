use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn transferred_retry_charges_again() ensures kernel::fee_delta_once_spec(true, true, 9) == 9int {} }
fn main() {}
