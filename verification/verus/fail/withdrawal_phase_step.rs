use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn lock_finalization_skips_release() ensures kernel::withdrawal_phase_step_spec(1, 1) == 3int {} }
fn main() {}
