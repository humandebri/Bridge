#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn an_expired_authorization_is_accepted()
    ensures
        kernel::mint_authorization_before_deadline_spec(1000, 1000)
            || kernel::mint_authorization_before_deadline_spec(
                18446744073709551615, 18446744073709551615)
{}
}
fn main() {}
