use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn refund_does_not_pay_the_ledger_fee()
    ensures kernel::deposit_refund_amount_spec(20_000, 10_000) == Some(20_000int) {} }
fn main() {}
