use vstd::prelude::*;

#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {
proof fn pending_authorization_cannot_release_reservation()
    ensures kernel::deposit_releases_reservation_spec(2, 6)
{}
}

fn main() {}
