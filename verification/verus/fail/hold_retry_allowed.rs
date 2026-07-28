use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn retry_without_evidence_is_allowed()
    ensures kernel::hold_retry_allowed_spec(false, false) {} }
fn main() {}
