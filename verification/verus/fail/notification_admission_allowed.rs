use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn at_least_one_exhausted_limit_is_ignored() -> (result: bool)
    ensures result
{
    kernel::notification_admission_allowed(48, 0, 48, 6)
        || kernel::notification_admission_allowed(1, 6, 48, 6)
}
}
fn main() {}
