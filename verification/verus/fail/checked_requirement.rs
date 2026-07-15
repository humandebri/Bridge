use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn reserve_overflow_wraps() ensures kernel::checked_requirement_spec(340282366920938463463374607431768211455int, 1, 1) == Some(0int) {} }
fn main() {}
