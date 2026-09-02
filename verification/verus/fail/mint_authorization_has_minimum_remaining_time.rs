#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn a_short_or_overflowing_authorization_window_is_accepted()
    ensures
        kernel::mint_authorization_has_minimum_remaining_time_spec(701, 1000)
            || kernel::mint_authorization_has_minimum_remaining_time_spec(
                18446744073709551615, 18446744073709551615)
{}
}
fn main() {}
