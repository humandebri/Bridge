use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn ledger_fee_is_not_deducted_from_the_liability_twice()
    ensures kernel::ambiguous_outbound_world_spec(true, 100, 10, 30, 15, 2, 3)
        == (83int, 13int, 10int) {} }
fn main() {}
