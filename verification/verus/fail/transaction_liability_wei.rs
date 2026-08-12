use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn overflowing_governance_liability_wraps()
    ensures kernel::transaction_liability_wei_spec(
        1,
        340282366920938463463374607431768211455int,
        1,
        0,
    ) == Some(0int)
{}
}
fn main() {}
