use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn released_withdrawal_becomes_refunded()
    ensures kernel::withdrawal_phase_run_spec(6, seq![10int, 11int]) == 9 {} }
fn main() {}
