use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn active_scheduled_job_can_be_claimed()
    ensures kernel::manual_claim_allowed_spec(false, true, true, false, false, false)
{} }
fn main() {}
