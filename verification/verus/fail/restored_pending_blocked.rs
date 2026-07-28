use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn restore_overwrites_an_existing_block()
    ensures kernel::restored_pending_blocked_spec(Some(true), false) == false
{}
}
fn main() {}
