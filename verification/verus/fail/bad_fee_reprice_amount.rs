use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn ambiguous_bad_fee_reprices() ensures kernel::bad_fee_reprice_amount_spec(100, 10, 5, 80, true, true) == Some(85int) {} }
fn main() {}
