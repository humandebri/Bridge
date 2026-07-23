use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn reverted_deposit_reopens_without_recovery()
    ensures !kernel::reverted_phase_recovery_spec(0) {} }
fn main() {}
