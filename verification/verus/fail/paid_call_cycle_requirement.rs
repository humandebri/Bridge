use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn paid_call_requirement_overflow_wraps()
    ensures kernel::paid_call_cycle_requirement_spec(
        340282366920938463463374607431768211455int,
        1,
        0,
    ) == Some(0int)
{}
}

fn main() {}
