use vstd::prelude::*;
#[allow(unused_macros)]
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn false_quota_claim()
    ensures kernel::notification_quota_acquire_spec(14, 0, 14, 2) == Some((15int, 1int))
{}
}
fn main() {}
