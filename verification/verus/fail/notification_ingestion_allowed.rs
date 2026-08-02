use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn ingestion_limit_is_ignored() -> (result: bool)
    ensures result
{
    kernel::notification_ingestion_allowed(24, 24)
}
}
fn main() {}
