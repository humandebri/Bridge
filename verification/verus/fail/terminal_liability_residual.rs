use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn incomplete_settlement_clears_liability()
    ensures kernel::terminal_liability_residual_spec(100, 80, 10, 5) == Some(0int) {} }
fn main() {}
