use vstd::prelude::*;
#[allow(unused_macros)]
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn false_quota_claim()
    ensures kernel::notification_quota_release_spec(1) == 1
{}
}
fn main() {}
