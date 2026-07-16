use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn inconsistent_committed_quote_matches()
    ensures kernel::committed_quote_matches_spec(100, 91, 10) {} }
fn main() {}
