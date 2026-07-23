use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn withdrawal_sequence_charges_twice()
    ensures kernel::withdrawal_fee_total_spec(1, seq![2int, 2int], 7) == 14 {} }
fn main() {}
