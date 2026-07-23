use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn resolved_hold_is_indexed() ensures kernel::reconciliation_hold_indexed_spec(1) {} }
fn main() {}
