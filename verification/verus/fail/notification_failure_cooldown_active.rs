#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn cooldown_remains_active_at_its_deadline()
    ensures kernel::notification_failure_cooldown_active_spec(true, 10, 10)
{}
}
fn main() {}
