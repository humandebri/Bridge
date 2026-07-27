use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn deposit_accepts_fee_above_user_maximum()
    -> (result: Option<kernel::DepositAdmissionDecision>)
    ensures result is Some
{
    kernel::deposit_admission_decision(10, 3, 2, 10, 0, 0, 10)
}
}
fn main() {}
