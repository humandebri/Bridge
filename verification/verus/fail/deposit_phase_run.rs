use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn terminal_deposit_reopens()
    ensures kernel::deposit_phase_run_spec(3, seq![0int, 1int, 2int]) == 0 {} }
fn main() {}
