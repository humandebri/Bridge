use vstd::prelude::*;

#[path = "../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {
proof fn incomplete_scan_cannot_prove_absence(next: int, tip: int, watermark: int)
    requires next <= tip || watermark < tip
    ensures !kernel::scan_complete_spec(next, tip, watermark, true, false)
{}

proof fn complete_scan_is_nonempty_witness(tip: int)
    ensures kernel::scan_complete_spec(tip + 1, tip, tip, true, false)
{}

proof fn matched_transfer_cannot_be_absent(next: int, tip: int, watermark: int)
    ensures !kernel::scan_complete_spec(next, tip, watermark, true, true)
{}

proof fn refund_requires_pending_and_no_attempt(pending: bool, attempted: bool)
    ensures kernel::refund_allowed_spec(pending, attempted) ==> pending && !attempted
{}

proof fn attempt_is_strictly_monotone(attempt: int)
    requires 0 <= attempt < 0xffff_ffff_ffff_ffffint
    ensures kernel::next_attempt_spec(attempt) == Some(attempt + 1), attempt + 1 > attempt
{}

proof fn attempt_overflow_is_rejected()
    ensures kernel::next_attempt_spec(0xffff_ffff_ffff_ffffint) == None::<int>
{}

proof fn counter_delta_matches_classification(old: bool, new: bool)
    ensures
        old == new ==> kernel::counter_delta_spec(old, new) == 0,
        !old && new ==> kernel::counter_delta_spec(old, new) == 1,
        old && !new ==> kernel::counter_delta_spec(old, new) == -1,
{}

proof fn terminal_retry_never_charges_again(fee: int)
    requires fee >= 0
    ensures kernel::terminal_retry_fee_spec(false, fee) == 0
{}

proof fn first_application_charges_exact_fee(fee: int)
    requires fee >= 0
    ensures kernel::terminal_retry_fee_spec(true, fee) == fee
{}

proof fn hold_resolution_requires_every_binding(
    request: bool, hold: bool, transfer: bool, open_or_retry: bool, evidence: bool,
)
    ensures kernel::evidence_matches_spec(request, hold, transfer, open_or_retry, evidence)
        ==> request && hold && transfer && open_or_retry && evidence
{}

proof fn missing_hold_evidence_is_rejected(request: bool, hold: bool, transfer: bool)
    ensures !kernel::evidence_matches_spec(request, hold, transfer, true, false)
{}

proof fn identical_payload_is_idempotent_witness()
    ensures kernel::replay_matches_spec(true)
{}

proof fn conflicting_payload_is_rejected()
    ensures !kernel::replay_matches_spec(false)
{}

proof fn evm_rank_never_decreases(old: int, new: int)
    requires 0 <= old <= new <= 3
    ensures kernel::monotone_spec(old, new)
{}

proof fn reserve_requirement_is_monotone(floor: int, unit: int, small: int, large: int)
    requires 0 <= floor, 0 <= unit, 0 <= small, 0 <= large, small <= large,
        floor <= 340282366920938463463374607431768211455int,
        small == 0 || unit <= (340282366920938463463374607431768211455int - floor) / small,
        large == 0 || unit <= (340282366920938463463374607431768211455int - floor) / large
    ensures kernel::checked_requirement_spec(floor, unit, small) == Some(floor + unit * small),
        kernel::checked_requirement_spec(floor, unit, large) == Some(floor + unit * large),
        floor + unit * small <= floor + unit * large
{
    vstd::arithmetic::mul::lemma_mul_inequality(small, large, unit);
}

proof fn reserve_exact_boundary_and_independent_resources(required_eth: int, required_cycles: int)
    requires 0 <= required_eth, 0 <= required_cycles
    ensures kernel::resources_sufficient_spec(required_eth, required_eth, required_cycles, required_cycles),
        !kernel::resources_sufficient_spec(required_eth - 1, required_eth, required_cycles, required_cycles),
        !kernel::resources_sufficient_spec(required_eth, required_eth, required_cycles - 1, required_cycles)
{}

proof fn reserve_overflow_is_rejected()
    ensures kernel::checked_requirement_spec(340282366920938463463374607431768211455int, 1, 1) == None::<int>
{}

proof fn mint_admission_counts_consumed_reserved_and_candidate(
    consumed: int, reserved: int, candidate: int,
)
    requires 0 <= consumed, 0 <= reserved, 0 <= candidate,
        consumed + reserved + candidate <= 340282366920938463463374607431768211455int
    ensures kernel::mint_admission_total_spec(consumed, reserved, candidate)
        == Some(consumed + reserved + candidate)
{}

proof fn mint_admission_overflow_is_rejected()
    ensures kernel::mint_admission_total_spec(
        340282366920938463463374607431768211455int, 1, 0) == None::<int>
{}

proof fn settlement_precedes_mint(settlement_id: int, mint_id: int)
    requires 0 <= settlement_id, 0 <= mint_id
    ensures kernel::candidate_precedes_spec(
        kernel::scheduler_priority_spec(0), settlement_id,
        kernel::scheduler_priority_spec(2), mint_id)
{}

proof fn same_priority_uses_smallest_id(left: int, right: int)
    requires 0 <= left < right
    ensures kernel::candidate_precedes_spec(0, left, 0, right)
{}

proof fn prepared_blocks_nonce_assignment()
    ensures
        !kernel::can_assign_nonce_spec(false, false),
        !kernel::can_assign_nonce_spec(true, true),
        kernel::can_assign_nonce_spec(true, false)
{}

proof fn nonce_is_strictly_monotone(current: int)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint
    ensures kernel::nonce_next_spec(current) == Some(current + 1), current + 1 > current
{}

proof fn nonce_overflow_is_rejected()
    ensures kernel::nonce_next_spec(0xffff_ffff_ffff_ffffint) == None::<int>
{}

proof fn payout_includes_fee_and_cannot_exceed_reserve(reserve: int, pending: int, amount: int, fee: int)
    requires 0 <= pending <= reserve, 0 <= amount, 0 <= fee,
        amount + fee <= 340282366920938463463374607431768211455int
    ensures kernel::payout_allowed_spec(reserve, pending, amount, fee)
        <==> amount + fee <= reserve - pending,
        kernel::payout_debit_spec(true, amount, fee) == Some(amount + fee),
        kernel::payout_debit_spec(false, amount, fee) == Some(0int)
{}

proof fn role_action_matrix(action: int, pause: bool, finance: bool, governance: bool)
    ensures kernel::administrator_authorized_spec(action, pause, finance, governance)
        <==> (action == 0 && pause)
            || ((action == 2 || action == 3) && finance)
            || ((action == 1 || action == 4) && governance)
{}

proof fn unprivileged_caller_has_no_action(action: int)
    ensures !kernel::administrator_authorized_spec(action, false, false, false)
{}

proof fn audit_sequence_is_strictly_monotone(current: int)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint
    ensures kernel::audit_next_spec(current) == Some(current + 1), current + 1 > current
{}

proof fn audit_overflow_is_rejected()
    ensures kernel::audit_next_spec(0xffff_ffff_ffff_ffffint) == None::<int>
{}
}

fn main() {}
