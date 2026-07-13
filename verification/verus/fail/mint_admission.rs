use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn pending_reservation_is_ignored() ensures
    kernel::mint_admission_total_spec(90, 9, 2) == Some(92int) {} }
fn main() {}
