use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn unsupported_withdrawal_event_allowed() ensures kernel::withdrawal_phase_allows_spec(1, 5) {} }
fn main() {}
