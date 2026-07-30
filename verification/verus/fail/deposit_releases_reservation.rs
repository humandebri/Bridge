use vstd::prelude::*;

#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {
proof fn deadline_equality_cannot_release_reservation()
    ensures kernel::deposit_releases_reservation_spec(2, 5)
{}
}

fn main() {}
