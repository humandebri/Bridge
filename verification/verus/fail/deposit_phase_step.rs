use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn pull_success_stays_pending() ensures kernel::deposit_phase_step_spec(0, 0) == 0int {} }
fn main() {}
