use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn malformed_or_zero_minimum_is_admissible()
    ensures kernel::withdrawal_id_is_admissible_spec(false, true, true) {} }
fn main() {}
