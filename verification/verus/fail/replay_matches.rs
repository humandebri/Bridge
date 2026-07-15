use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn conflicting_replay_matches() ensures kernel::replay_matches_spec(false) {} }
fn main() {}
