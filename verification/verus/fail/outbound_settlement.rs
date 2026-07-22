use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn fee_inversion_is_accepted()
    ensures kernel::outbound_settlement_spec(90, 11, 10)
        == Some((101int, -1int, 100int)) {} }
fn main() {}
