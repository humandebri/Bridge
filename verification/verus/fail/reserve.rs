use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn overflow_and_insufficient_eth_are_accepted()
    ensures kernel::checked_requirement_spec(340282366920938463463374607431768211455int, 1, 1) == Some(0int),
        kernel::resources_sufficient_spec(9, 10, 20, 20) {} }
fn main() {}
