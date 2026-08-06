use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn refund_does_not_pay_charged_fees()
    ensures kernel::deposit_refund_amount_spec(30_000, 10_000, 10_000) == Some(30_000int) {} }
fn main() {}
