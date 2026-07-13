use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn failed_and_excess_payout_debit() ensures
    kernel::payout_allowed_spec(9, 0, 7, 3),
    kernel::payout_debit_spec(false, 7, 3) == Some(10int) {} }
fn main() {}
