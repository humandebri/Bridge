use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn automatic_retry_allows_the_third_failure()
    ensures kernel::automatic_retry_allowed_spec(true, 3, 3)
{}
}

fn main() {}
