use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn refund_without_absence() ensures kernel::refund_allowed_spec(true, false) {} }
fn main() {}
