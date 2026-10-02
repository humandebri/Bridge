use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn overflowing_notification_slots_wrap_to_zero()
    ensures kernel::notification_reservation_slots_spec(9223372036854775808int) == Some(0int)
{}
}

fn main() {}
