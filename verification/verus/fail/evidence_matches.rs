use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn missing_evidence_matches() ensures kernel::evidence_matches_spec(true, true, true, true, false) {} }
fn main() {}
