use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn stale_refresh_owner_is_accepted() ensures kernel::refresh_owner_matches_spec(Some(7int), 8) {} }
fn main() {}
