use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn candidate_disappears_without_reservation()
    ensures kernel::reserve_admission_preserves_requirement_spec(7, 1, 7, 0) {} }
fn main() {}
