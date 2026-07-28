use vstd::prelude::*;

// rustc's unused-macro lint does not see invocations expanded inside Verus spec functions.
// The macros remain exercised by both the executable kernel and the specs below.
#[allow(unused_macros)]
#[path = "../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {
pub open spec fn settlement_decision_view(
    result: Option<kernel::SettlementDecision>,
) -> Option<(int, int, int)> {
    match result {
        Some(decision) => Some((
            decision.escrow_debit as int,
            decision.reserve_credit as int,
            decision.liability_debit as int,
        )),
        None => None,
    }
}

pub open spec fn deposit_admission_decision_view(
    result: Option<kernel::DepositAdmissionDecision>,
) -> Option<(int, int)> {
    match result {
        Some(decision) =>
            Some((decision.net_amount as int, decision.next_window_total as int)),
        None => None,
    }
}

pub open spec fn reservation_decision_view(
    result: Option<kernel::ReservationDecision>,
) -> Option<(int, int)> {
    match result {
        Some(decision) => Some((decision.reserved as int, decision.candidate as int)),
        None => None,
    }
}

pub open spec fn payout_decision_view(
    result: Option<kernel::PayoutDecision>,
) -> Option<int> {
    match result {
        Some(decision) => Some(decision.debit as int),
        None => None,
    }
}

pub open spec fn fee_rotation_decision_view(
    result: kernel::FeeRecipientRotationDecision,
) -> int {
    match result {
        kernel::FeeRecipientRotationDecision::Allow => 0,
        kernel::FeeRecipientRotationDecision::Unauthorized => 1,
        kernel::FeeRecipientRotationDecision::InvalidInput => 2,
        kernel::FeeRecipientRotationDecision::Busy => 3,
    }
}

pub open spec fn lease_outcome_decision_view(result: kernel::LeaseOutcomeDecision) -> int {
    match result {
        kernel::LeaseOutcomeDecision::Accept => 0,
        kernel::LeaseOutcomeDecision::Reject => 1,
    }
}

pub open spec fn hold_resolution_decision_view(
    result: kernel::HoldResolutionDecision,
) -> int {
    match result {
        kernel::HoldResolutionDecision::Wait => 0,
        kernel::HoldResolutionDecision::ResolveSucceeded => 1,
        kernel::HoldResolutionDecision::ResolveAbsent => 2,
    }
}

pub open spec fn manual_claim_decision_view(result: kernel::ManualClaimDecision) -> int {
    match result {
        kernel::ManualClaimDecision::Allow => 0,
        kernel::ManualClaimDecision::AutomaticProgressPending => 1,
    }
}

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

proof fn only_current_refresh_owner_can_finish(current: int, claimant: int)
    ensures
        kernel::refresh_owner_matches_spec(Some(current), claimant) <==> current == claimant,
        !kernel::refresh_owner_matches_spec(None, claimant),
{}

proof fn refresh_generation_is_strictly_monotone(current: int)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint
    ensures
        kernel::refresh_generation_next_spec(current) == Some(current + 1),
        current + 1 > current,
        kernel::refresh_generation_next_spec(0xffff_ffff_ffff_ffffint) == None::<int>,
{}

proof fn reserve_token_rejects_any_drift(
    expected_withdrawals: int,
    expected_amount: int,
    expected_operations: int,
    expected_generation: int,
    current_withdrawals: int,
    current_amount: int,
    current_operations: int,
    current_generation: int,
)
    ensures kernel::reserve_token_matches_spec(
        expected_withdrawals,
        expected_amount,
        expected_operations,
        expected_generation,
        current_withdrawals,
        current_amount,
        current_operations,
        current_generation,
    ) <==> expected_withdrawals == current_withdrawals
        && expected_amount == current_amount
        && expected_operations == current_operations
        && expected_generation == current_generation
{}

proof fn lease_generation_is_strictly_monotone(current: int)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint
    ensures
        kernel::lease_generation_next_spec(current) == Some(current + 1),
        current + 1 > current,
        kernel::lease_generation_next_spec(0xffff_ffff_ffff_ffffint) == None::<int>,
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

proof fn candidate_reservation_increases_both_requirements(
    floor: int, unit: int, current: int,
)
    requires 0 <= floor, 0 <= unit, 0 <= current,
        floor <= 340282366920938463463374607431768211455int,
        unit <= (340282366920938463463374607431768211455int - floor) / (current + 1)
    ensures
        kernel::checked_requirement_spec(floor, unit, current + 1)
            == Some(floor + unit * (current + 1)),
        floor + unit * current <= floor + unit * (current + 1)
{
    assert(current <= current + 1);
    vstd::arithmetic::mul::lemma_mul_inequality(current, current + 1, unit);
}

proof fn fee_is_counted_exactly_on_first_transfer(fee: int)
    requires 0 <= fee
    ensures
        kernel::fee_delta_once_spec(false, true, fee) == fee,
        kernel::fee_delta_once_spec(true, true, fee) == 0,
        kernel::fee_delta_once_spec(false, false, fee) == 0
{}

proof fn release_transfer_requires_exact_amount_and_fee(
    transfer_amount: int, transfer_fee: int, amount_out: int, ledger_fee: int,
)
    ensures kernel::release_transfer_matches_spec(
        transfer_amount, transfer_fee, amount_out, ledger_fee)
            <==> transfer_amount == amount_out && transfer_fee == ledger_fee
{}

proof fn committed_quote_fixes_amount_out(
    amount: int, amount_out: int, service_fee: int,
)
    requires
        0 < amount_out,
        0 <= service_fee,
        amount == amount_out + service_fee,
        amount <= 340282366920938463463374607431768211455int,
    ensures kernel::committed_quote_matches_spec(amount, amount_out, service_fee)
{}

proof fn outbound_settlement_uses_the_committed_fee_once(
    amount_out: int, ledger_fee: int, service_fee: int,
)
    requires
        0 <= amount_out,
        0 <= ledger_fee <= service_fee,
        amount_out + service_fee <= 340282366920938463463374607431768211455int,
    ensures kernel::outbound_settlement_spec(amount_out, ledger_fee, service_fee)
        == Some((amount_out + ledger_fee, service_fee - ledger_fee,
            amount_out + service_fee))
{}

fn settlement_decision_returns_exact_checked_delta(
    amount_out: u128, ledger_fee: u128, service_fee: u128,
) -> (result: Option<kernel::SettlementDecision>)
    requires
        ledger_fee <= service_fee,
        amount_out as int + service_fee as int
            <= 340282366920938463463374607431768211455int,
    ensures settlement_decision_view(result)
        == Some((amount_out as int + ledger_fee as int,
            service_fee as int - ledger_fee as int,
            amount_out as int + service_fee as int))
{
    assert(ledger_fee as int <= service_fee as int);
    assert(amount_out as int + ledger_fee as int
        <= 340282366920938463463374607431768211455int);
    assert(amount_out <= u128::MAX - ledger_fee);
    assert(amount_out <= u128::MAX - service_fee);
    kernel::settlement_decision(amount_out, ledger_fee, service_fee)
}

fn deposit_admission_decision_checks_every_bound(
    gross: u128, fee: u128, maximum_fee: u128, per_deposit: u128,
    consumed: u128, reserved: u128, window_limit: u128,
) -> (result: Option<kernel::DepositAdmissionDecision>)
    requires
        fee <= maximum_fee,
        fee < gross,
        (gross - fee) <= per_deposit,
        consumed as int + reserved as int + (gross as int - fee as int)
            <= 340282366920938463463374607431768211455int,
        consumed as int + reserved as int + (gross as int - fee as int)
            <= window_limit as int,
    ensures deposit_admission_decision_view(result)
        == Some((gross as int - fee as int,
            consumed as int + reserved as int + (gross as int - fee as int)))
{
    assert((gross - fee) as int == gross as int - fee as int);
    assert(consumed <= u128::MAX - reserved);
    assert((consumed + reserved) as int == consumed as int + reserved as int);
    assert(consumed + reserved <= u128::MAX - (gross - fee));
    assert((consumed + reserved + (gross - fee)) as int
        == consumed as int + reserved as int + (gross as int - fee as int));
    kernel::deposit_admission_decision(
        gross, fee, maximum_fee, per_deposit, consumed, reserved, window_limit)
}

fn reservation_decision_preserves_candidate_requirement(
    reserved: u128, candidate: u128,
) -> (result: Option<kernel::ReservationDecision>)
    requires
        reserved as int + candidate as int
            <= 340282366920938463463374607431768211455int,
    ensures reservation_decision_view(result)
        == Some((reserved as int + candidate as int, 0int))
{
    assert(reserved <= u128::MAX - candidate);
    kernel::reservation_decision(reserved, candidate)
}

fn payout_decision_returns_the_only_allowed_debit(
    reserve: u128, pending: u128, amount: u128, fee: u128,
    confirmed_first_time: bool,
) -> (result: Option<kernel::PayoutDecision>)
    requires
        pending <= reserve,
        amount as int + fee as int
            <= 340282366920938463463374607431768211455int,
        amount as int + fee as int <= reserve as int - pending as int,
    ensures payout_decision_view(result)
        == Some(if confirmed_first_time { amount as int + fee as int } else { 0int })
{
    assert(amount <= u128::MAX - fee);
    assert(amount + fee <= reserve - pending);
    kernel::payout_decision(reserve, pending, amount, fee, confirmed_first_time)
}

fn lease_outcome_decision_rejects_stale_or_inactive(
    active_generation: u64, outcome_generation: u64, active: bool,
) -> (result: kernel::LeaseOutcomeDecision)
    ensures
        lease_outcome_decision_view(result) == 0
            <==> active && active_generation == outcome_generation
{
    kernel::lease_outcome_decision(active_generation, outcome_generation, active)
}

proof fn active_counter_transition_preserves_classification(current: int, old: bool, new: bool)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint,
        old && !new ==> current > 0
    ensures
        old == new ==> kernel::checked_counter_transition_spec(current, old, new) == Some(current),
        !old && new ==> kernel::checked_counter_transition_spec(current, old, new) == Some(current + 1),
        old && !new ==> kernel::checked_counter_transition_spec(current, old, new) == Some(current - 1)
{}

proof fn canonical_probe_accepts_exact_block(receipt_block: int, snapshot_block: int)
    ensures kernel::canonical_probe_matches_spec(receipt_block, snapshot_block)
        <==> receipt_block == snapshot_block
{}

proof fn withdrawal_finalization_decision_is_fail_closed(
    receipt_succeeded: bool,
    receipt_block: int,
    finalized_block: Option<int>,
)
    ensures kernel::withdrawal_finalization_decision_spec(
        receipt_succeeded, receipt_block, finalized_block)
        == match finalized_block {
            None => 0int,
            Some(finalized) if finalized < receipt_block => 0int,
            Some(_) if receipt_succeeded => 1int,
            Some(_) => 2int,
        }
{}

proof fn pending_queue_restore_preserves_existing_block(
    existing_blocked: Option<bool>,
    incoming_blocked: bool,
)
    ensures kernel::restored_pending_blocked_spec(existing_blocked, incoming_blocked)
        == match existing_blocked {
            Some(blocked) => blocked,
            None => incoming_blocked,
        }
{}

proof fn withdrawal_liability_index_matches_nonterminal_phases(state: int)
    ensures kernel::withdrawal_liability_indexed_spec(state)
        <==> state == 0 || state == 1 || state == 3
{}

proof fn evm_operation_index_matches_pending_phases(state: int)
    ensures kernel::evm_operation_indexed_spec(state)
        <==> state == 0 || state == 1 || state == 2
{}

proof fn reconciliation_hold_index_matches_open_phase(state: int)
    ensures kernel::reconciliation_hold_indexed_spec(state) <==> state == 0
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

proof fn nonce_too_low_requires_provider_agreement_and_local_hash(
    provider_agreement: bool, local_hash_found: bool,
)
    ensures kernel::nonce_too_low_is_submitted_spec(provider_agreement, local_hash_found)
        <==> provider_agreement && local_hash_found
{}

proof fn payout_includes_fee_and_cannot_exceed_reserve(reserve: int, pending: int, amount: int, fee: int)
    requires 0 <= pending <= reserve, 0 <= amount, 0 <= fee,
        amount + fee <= 340282366920938463463374607431768211455int
    ensures kernel::payout_allowed_spec(reserve, pending, amount, fee)
        <==> amount + fee <= reserve - pending,
        kernel::payout_debit_spec(true, amount, fee) == Some(amount + fee),
        kernel::payout_debit_spec(false, amount, fee) == Some(0int)
{}

proof fn fee_recipient_rotation_requires_no_pending_payout(pending: int)
    requires 0 <= pending
    ensures kernel::fee_recipient_rotation_allowed_spec(pending) <==> pending == 0
{}

fn fee_recipient_rotation_decision_is_fail_closed(
    authorized: bool,
    anonymous: bool,
    role_collision: bool,
    subaccount_len: usize,
    pending_payout_debit: u128,
) -> (result: kernel::FeeRecipientRotationDecision)
    ensures
        fee_rotation_decision_view(result) == 0
            <==> authorized && !anonymous && !role_collision
                && (subaccount_len == 0 || subaccount_len == 32)
                && pending_payout_debit == 0,
{
    kernel::fee_recipient_rotation_decision(
        authorized, anonymous, role_collision, subaccount_len, pending_payout_debit)
}

proof fn service_fee_change_respects_immutable_maximum(service_fee: int, maximum: int)
    requires 0 <= service_fee, 0 <= maximum
    ensures kernel::service_fee_change_allowed_spec(service_fee, maximum)
        <==> service_fee <= maximum
{}

proof fn reserve_candidate_becomes_reservation_without_reducing_requirement(
    reserved: int, candidate: int,
)
    requires 0 <= reserved, 0 <= candidate
    ensures kernel::reserve_admission_preserves_requirement_spec(
        reserved, candidate, reserved + candidate, 0)
{}

proof fn stale_or_inactive_lease_outcome_is_rejected(
    active_generation: int, outcome_generation: int, active: bool,
)
    ensures kernel::lease_outcome_is_current_spec(
        active_generation, outcome_generation, active)
        <==> active && active_generation == outcome_generation
{}

proof fn hold_requires_success_or_complete_absence(success: bool, absence: bool)
    ensures kernel::hold_retry_allowed_spec(success, absence) <==> success || absence
{}

fn hold_resolution_decision_classifies_evidence(
    success: bool, absence: bool,
) -> (result: kernel::HoldResolutionDecision)
    ensures
        hold_resolution_decision_view(result) == 1 <==> success,
        hold_resolution_decision_view(result) == 2 <==> !success && absence,
        hold_resolution_decision_view(result) == 0 <==> !success && !absence,
{
    kernel::hold_resolution_decision(success, absence)
}

proof fn manual_claim_cannot_bypass_confirmation_or_active_schedule(
    confirmation: bool,
    scheduled: bool,
    active: bool,
    stopped: bool,
    overdue: bool,
    expired: bool,
)
    ensures kernel::manual_claim_allowed_spec(
        confirmation, scheduled, active, stopped, overdue, expired)
        <==> !confirmation
            && (!active || expired)
            && (!scheduled || stopped || overdue || expired)
{}

fn manual_claim_decision_matches_shared_guard(
    confirmation: bool,
    scheduled: bool,
    active: bool,
    stopped: bool,
    overdue: bool,
    expired: bool,
) -> (result: kernel::ManualClaimDecision)
    ensures manual_claim_decision_view(result) == 0
        <==> kernel::manual_claim_allowed_spec(
            confirmation, scheduled, active, stopped, overdue, expired),
{
    kernel::manual_claim_decision(
        confirmation, scheduled, active, stopped, overdue, expired)
}

proof fn role_action_matrix(action: int, pause: bool, governance: bool)
    ensures kernel::administrator_authorized_spec(action, pause, governance)
        <==> (action == 0 && pause)
            || ((action == 1 || action == 2 || action == 3) && governance)
{}

proof fn unprivileged_caller_has_no_action(action: int)
    ensures !kernel::administrator_authorized_spec(action, false, false)
{}

proof fn audit_sequence_is_strictly_monotone(current: int)
    requires 0 <= current < 0xffff_ffff_ffff_ffffint
    ensures kernel::audit_next_spec(current) == Some(current + 1), current + 1 > current
{}

proof fn audit_overflow_is_rejected()
    ensures kernel::audit_next_spec(0xffff_ffff_ffff_ffffint) == None::<int>
{}

proof fn deposit_terminal_phase_absorbs_any_sequence(state: int, events: Seq<int>)
    requires state == 3 || state == 9 || state == 10
    ensures kernel::deposit_phase_run_spec(state, events) == state
    decreases events.len()
{
    if events.len() > 0 {
        assert(kernel::deposit_phase_step_spec(state, events[0]) == state);
        deposit_terminal_phase_absorbs_any_sequence(state, events.drop_first());
    }
}

proof fn withdrawal_terminal_phase_absorbs_any_sequence(state: int, events: Seq<int>)
    requires state == 2
    ensures kernel::withdrawal_phase_run_spec(state, events) == state
    decreases events.len()
{
    if events.len() > 0 {
        assert(kernel::withdrawal_phase_step_spec(state, events[0]) == state);
        withdrawal_terminal_phase_absorbs_any_sequence(state, events.drop_first());
    }
}

proof fn deposit_phase_allowance_matches_a_state_change(state: int, event: int)
    ensures kernel::deposit_phase_allows_spec(state, event)
        <==> kernel::deposit_phase_step_spec(state, event) != state
{}

proof fn withdrawal_phase_allowance_matches_a_transition(state: int, event: int)
    ensures kernel::withdrawal_phase_allows_spec(state, event)
        <==> kernel::withdrawal_phase_step_spec(state, event) != state
            || (state == 1 && event == 1)
{}

proof fn reverted_phases_only_reopen_through_recovery_events(event: int)
    ensures kernel::reverted_phase_recovery_spec(event)
{}

proof fn deposit_fee_delta_occurs_only_on_mint(state: int, event: int, fee: int)
    ensures kernel::deposit_fee_delta_spec(state, event, fee)
        == if state == 2 && event == 9 { fee } else { 0 }
{}

proof fn deposit_refund_debits_exactly_gross(gross: int, ledger_fee: int)
    requires 0 <= ledger_fee < gross
    ensures kernel::deposit_refund_amount_spec(gross, ledger_fee) == Some(gross - ledger_fee),
        (gross - ledger_fee) + ledger_fee == gross
{}

proof fn refund_hold_cannot_reopen_through_a_deposit_event(event: int)
    ensures kernel::deposit_phase_step_spec(7, event) == 7
{}

proof fn refund_retry_requires_matching_evidence(request: bool, hold: bool, transfer: bool, open_or_retry: bool)
    ensures !kernel::evidence_matches_spec(request, hold, transfer, open_or_retry, false)
{}

proof fn withdrawal_fee_delta_occurs_only_on_release(state: int, event: int, fee: int)
    ensures kernel::withdrawal_fee_delta_spec(state, event, fee)
        == if state == 1 && event == 2 { fee } else { 0 }
{}

proof fn deposit_post_mint_never_charges(state: int, events: Seq<int>, fee: int)
    requires state == 3 || state == 9 || state == 10, 0 <= fee
    ensures kernel::deposit_fee_total_spec(state, events, fee) == 0
    decreases events.len()
{
    if events.len() > 0 {
        assert(kernel::deposit_fee_delta_spec(state, events[0], fee) == 0);
        assert(kernel::deposit_phase_step_spec(state, events[0]) == state);
        deposit_post_mint_never_charges(state, events.drop_first(), fee);
    }
}

proof fn withdrawal_post_transfer_never_charges(state: int, events: Seq<int>, fee: int)
    requires state == 2,
        0 <= fee
    ensures kernel::withdrawal_fee_total_spec(state, events, fee) == 0
    decreases events.len()
{
    if events.len() > 0 {
        let next = kernel::withdrawal_phase_step_spec(state, events[0]);
        assert(kernel::withdrawal_fee_delta_spec(state, events[0], fee) == 0);
        assert(next == 2);
        withdrawal_post_transfer_never_charges(next, events.drop_first(), fee);
    }
}

proof fn deposit_fee_is_charged_at_most_once(state: int, events: Seq<int>, fee: int)
    requires 0 <= state <= 10, 0 <= fee
    ensures 0 <= kernel::deposit_fee_total_spec(state, events, fee) <= fee
    decreases events.len()
{
    if events.len() > 0 {
        let event = events[0];
        let next = kernel::deposit_phase_step_spec(state, event);
        if state == 2 && event == 9 {
            deposit_post_mint_never_charges(3, events.drop_first(), fee);
        } else {
            assert(0 <= next <= 10);
            deposit_fee_is_charged_at_most_once(next, events.drop_first(), fee);
        }
    }
}

proof fn withdrawal_fee_is_charged_at_most_once(state: int, events: Seq<int>, fee: int)
    requires 0 <= state <= 3, 0 <= fee
    ensures 0 <= kernel::withdrawal_fee_total_spec(state, events, fee) <= fee
    decreases events.len()
{
    if events.len() > 0 {
        let event = events[0];
        let next = kernel::withdrawal_phase_step_spec(state, event);
        if state == 1 && event == 2 {
            withdrawal_post_transfer_never_charges(2, events.drop_first(), fee);
        } else {
            assert(0 <= next <= 3);
            withdrawal_fee_is_charged_at_most_once(next, events.drop_first(), fee);
        }
    }
}

proof fn deposit_mint_preserves_one_to_one_backing(
    escrow: int, supply: int, fees: int, unminted: int, unreleased: int,
    gross: int, net: int, service_fee: int,
)
    requires kernel::asset_backed_spec(escrow, supply, fees, unminted, unreleased),
        0 <= service_fee, 0 <= net, gross == net + service_fee, gross <= unminted
    ensures kernel::asset_backed_spec(
        escrow, supply + net, fees + service_fee, unminted - gross, unreleased)
{}

proof fn withdrawal_observation_preserves_one_to_one_backing(
    escrow: int, supply: int, fees: int, unminted: int, unreleased: int, amount: int,
)
    requires kernel::asset_backed_spec(escrow, supply, fees, unminted, unreleased),
        0 <= amount <= supply
    ensures kernel::asset_backed_spec(
        escrow, supply - amount, fees, unminted, unreleased + amount)
{}

proof fn withdrawal_release_preserves_one_to_one_backing(
    escrow: int, supply: int, fees: int, unminted: int, unreleased: int,
    amount_out: int, ledger_fee: int, service_fee: int,
)
    requires kernel::asset_backed_spec(escrow, supply, fees, unminted, unreleased),
        0 <= amount_out, 0 <= ledger_fee, 0 <= service_fee,
        ledger_fee <= service_fee,
        amount_out + service_fee <= unreleased
    ensures kernel::asset_backed_spec(
        escrow - amount_out - ledger_fee, supply, fees + service_fee - ledger_fee, unminted,
        unreleased - amount_out - service_fee)
{}

proof fn fee_payout_preserves_one_to_one_backing(
    escrow: int, supply: int, fees: int, unminted: int, unreleased: int, debit: int,
)
    requires kernel::asset_backed_spec(escrow, supply, fees, unminted, unreleased),
        0 <= debit <= fees
    ensures kernel::asset_backed_spec(
        escrow - debit, supply, fees - debit, unminted, unreleased)
{}

proof fn pending_payout_is_bounded_by_confirmed_fees(fees: int, pending: int)
    requires 0 <= pending <= fees
    ensures kernel::payout_reserved_spec(fees, pending)
{}

proof fn ambiguous_outbound_resolution_preserves_a_possible_world(
    happened: bool, escrow: int, supply: int, fees: int, unminted: int, unreleased: int,
    amount_out: int, ledger_fee: int, service_fee: int,
)
    requires kernel::asset_backed_spec(escrow, supply, fees, unminted, unreleased),
        0 <= amount_out, 0 <= ledger_fee, 0 <= service_fee,
        ledger_fee <= service_fee,
        amount_out + service_fee <= unreleased,
        amount_out + ledger_fee <= escrow
    ensures ({
        let resolved = kernel::ambiguous_outbound_world_spec(
            happened, escrow, fees, unreleased, amount_out, ledger_fee, service_fee);
        kernel::asset_backed_spec(resolved.0, supply, resolved.1, unminted, resolved.2)
    })
{}
}

fn main() {}
