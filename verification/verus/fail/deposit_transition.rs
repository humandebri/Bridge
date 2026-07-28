use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn terminal_mint_reopens()
    ensures kernel::deposit_transition_spec(10, 0) == Some(1int) {} }
fn main() {}
