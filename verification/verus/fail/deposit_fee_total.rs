use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn deposit_sequence_charges_twice()
    ensures kernel::deposit_fee_total_spec(2, seq![7int, 7int], 7) == 14 {} }
fn main() {}
