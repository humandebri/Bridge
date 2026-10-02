use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn suspended_notification_keeps_ambient_ownership()
    ensures kernel::notification_poll_owner_spec(false, false)
{}
}

fn main() {}
