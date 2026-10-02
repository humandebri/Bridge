use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn owner_can_spend_another_notifications_slots()
    ensures kernel::notification_reservation_excluding_owner_spec(4int, true) == Some(0int)
{}
}

fn main() {}
