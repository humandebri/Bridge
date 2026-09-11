use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn unauthorized_low_balance_request_is_allowed()
    ensures kernel::cycles_top_up_request_allowed_spec(0, 2_000_000_000_000, false, false),
{}
proof fn overlapping_request_is_allowed()
    ensures kernel::cycles_top_up_request_allowed_spec(0, 2_000_000_000_000, true, true),
{}
proof fn above_threshold_request_is_allowed()
    ensures kernel::cycles_top_up_request_allowed_spec(2_000_000_000_001, 2_000_000_000_000, false, true),
{}
}
fn main() {}
