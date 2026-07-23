use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn deposit_sequence_charges_twice()
    ensures kernel::deposit_fee_total_spec(2, seq![4int, 4int], 7) == 14 {} }
fn main() {}
