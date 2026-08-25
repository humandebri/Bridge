use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn a_provider_below_the_checkpoint_can_supply_the_second_vote()
    ensures kernel::withdrawal_finalized_checkpoint_quorum_spec(
        Some((90int, 1int)),
        Some((100int, 2int)),
        Some((110int, 3int)),
        Some((100int, 0xaaint)),
        Some((100int, 0xaaint)),
        Some((100int, 0xbbint)),
        100int,
    ) == Some((100int, 0xaaint))
{}
}
fn main() {}
