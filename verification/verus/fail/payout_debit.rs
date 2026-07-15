use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn failed_payout_debits() ensures kernel::payout_debit_spec(false, 7, 3) == Some(10int) {} }
fn main() {}
