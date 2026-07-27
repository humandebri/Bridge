use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn caller_limit_is_ignored() -> (result: bool)
    ensures result
{
    kernel::notification_admission_allowed(6, 0, 6, 3)
}
}
fn main() {}
