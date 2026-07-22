use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn stale_reserve_token_matches() ensures kernel::reserve_token_matches_spec(1, 2, 3, 4, 1, 2, 3, 5) {} }
fn main() {}
