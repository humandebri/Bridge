// These expression macros are the single source for the Cargo executable functions and their
// Verus spec views. Keep them free of allocation, traits, I/O, and canister APIs.
macro_rules! scan_complete_body {
    ($next:expr, $tip:expr, $watermark:expr, $archives:expr, $matched:expr) => {
        $archives && !$matched && $watermark >= $tip && $next > $tip
    };
}

macro_rules! refund_allowed_body {
    ($pending:expr, $proven_absent:expr) => {
        $pending && $proven_absent
    };
}

macro_rules! next_attempt_body {
    ($attempt:expr, $max:expr, $one:expr) => {
        if $attempt == $max {
            None
        } else {
            Some($attempt + $one)
        }
    };
}

macro_rules! counter_delta_body {
    ($was:expr, $is:expr, $zero:expr, $one:expr, $minus_one:expr) => {
        if $was == $is {
            $zero
        } else if $is {
            $one
        } else {
            $minus_one
        }
    };
}

macro_rules! evidence_matches_body {
    ($request:expr, $hold:expr, $transfer:expr, $open_or_retry:expr, $evidence:expr) => {
        $request && $hold && $transfer && $open_or_retry && $evidence
    };
}

macro_rules! replay_body {
    ($same:expr) => {
        $same
    };
}

macro_rules! monotone_body {
    ($old:expr, $new:expr) => {
        $new >= $old
    };
}

macro_rules! checked_requirement_body {
    ($floor:expr, $unit:expr, $count:expr, $max:expr, $zero:expr) => {
        if $count != $zero && $unit > ($max - $floor) / $count {
            None
        } else {
            Some($floor + $unit * $count)
        }
    };
}

macro_rules! resources_sufficient_body {
    ($eth:expr, $required_eth:expr, $cycles:expr, $required_cycles:expr) => {
        $eth >= $required_eth && $cycles >= $required_cycles
    };
}

macro_rules! bad_fee_reprice_amount_body {
    ($gross:expr, $service_fee:expr, $ledger_fee:expr, $minimum:expr, $definitive:expr, $uncertain:expr) => {{
        if !$definitive
            || $uncertain
            || $service_fee > $gross
            || $ledger_fee > $gross - $service_fee
        {
            None
        } else {
            let amount_out = $gross - $service_fee - $ledger_fee;
            if amount_out < $minimum {
                None
            } else {
                Some(amount_out)
            }
        }
    }};
}

macro_rules! fee_delta_once_body {
    ($was_transferred:expr, $is_transferred:expr, $fee:expr, $zero:expr) => {
        if !$was_transferred && $is_transferred {
            $fee
        } else {
            $zero
        }
    };
}

macro_rules! terminal_liability_residual_body {
    ($liability:expr, $amount_out:expr, $service_fee:expr, $ledger_fee:expr, $max:expr) => {{
        if $amount_out > $max - $service_fee {
            None
        } else {
            let partial = $amount_out + $service_fee;
            if $ledger_fee > $max - partial {
                None
            } else {
                let discharged = partial + $ledger_fee;
                if discharged > $liability {
                    None
                } else {
                    Some($liability - discharged)
                }
            }
        }
    }};
}

macro_rules! refund_operation_binding_body {
    ($kind:expr, $operation:expr, $payload:expr, $calldata:expr) => {
        $kind && $operation && $payload && $calldata
    };
}

macro_rules! nonce_too_low_submitted_body {
    ($provider_agreement:expr, $local_hash_found:expr) => {
        $provider_agreement && $local_hash_found
    };
}

macro_rules! mint_admission_total_body {
    ($consumed:expr, $reserved:expr, $candidate:expr, $max:expr) => {{
        if $reserved > $max || $consumed > $max - $reserved {
            None
        } else {
            let committed = $consumed + $reserved;
            if $candidate > $max - committed {
                None
            } else {
                Some(committed + $candidate)
            }
        }
    }};
}

macro_rules! payout_allowed_body {
    ($reserve:expr, $pending:expr, $amount:expr, $fee:expr, $max:expr) => {{
        if $amount > $max - $fee {
            false
        } else {
            let debit = $amount + $fee;
            $pending <= $reserve && debit <= $reserve - $pending
        }
    }};
}

macro_rules! authorized_body {
    ($action:expr, $pause:expr, $finance:expr, $governance:expr, $pause_action:expr, $resume_action:expr, $recipient_action:expr, $payout_action:expr, $rotate_action:expr) => {
        ($action == $pause_action && $pause)
            || (($action == $recipient_action || $action == $payout_action) && $finance)
            || (($action == $resume_action || $action == $rotate_action) && $governance)
    };
}

macro_rules! deposit_step_body {
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr, $six:expr) => {{
        if $state == $zero && $event == $zero {
            $one
        } else if $state == $zero && $event == $one {
            $five
        } else if $state == $zero && $event == $two {
            $six
        } else if $state == $one && $event == $three {
            $two
        } else if $state == $two && $event == $four {
            $three
        } else if $state == $two && $event == $five {
            $four
        } else {
            $state
        }
    }};
}

macro_rules! withdrawal_step_body {
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr, $six:expr, $seven:expr, $eight:expr, $nine:expr, $ten:expr, $eleven:expr) => {{
        if $state == $zero && $event == $zero {
            $one
        } else if $state == $zero && $event == $four {
            $ten
        } else if $state == $one && $event == $one {
            $one
        } else if $state == $one && $event == $two {
            $two
        } else if $state == $one && $event == $three {
            $nine
        } else if $state == $one && $event == $four {
            $ten
        } else if $state == $ten && $event == $five {
            $eleven
        } else if $state == $two && $event == $six {
            $three
        } else if $state == $three && $event == $seven {
            $five
        } else if $state == $three && $event == $eight {
            $four
        } else if ($state == $zero || $state == $eleven) && $event == $nine {
            $six
        } else if $state == $six && $event == $ten {
            $eight
        } else if $state == $six && $event == $eleven {
            $seven
        } else {
            $state
        }
    }};
}

#[cfg(verus_keep_ghost)]
macro_rules! asset_backed_body {
    ($escrow:expr, $supply:expr, $fees:expr, $unminted:expr, $unreleased:expr) => {
        $escrow == $supply + $fees + $unminted + $unreleased
    };
}

#[cfg(not(verus_keep_ghost))]
pub const fn scan_complete(
    next: u128,
    tip: u128,
    watermark: u128,
    archives: bool,
    matched: bool,
) -> bool {
    scan_complete_body!(next, tip, watermark, archives, matched)
}

#[cfg(not(verus_keep_ghost))]
pub const fn refund_allowed(base_pending: bool, release_transfer_proven_absent: bool) -> bool {
    refund_allowed_body!(base_pending, release_transfer_proven_absent)
}

#[cfg(not(verus_keep_ghost))]
pub const fn next_attempt(attempt: u64) -> Option<u64> {
    next_attempt_body!(attempt, u64::MAX, 1u64)
}

#[cfg(not(verus_keep_ghost))]
pub const fn counter_delta(was_active: bool, is_active: bool) -> i8 {
    counter_delta_body!(was_active, is_active, 0i8, 1i8, -1i8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn evidence_matches(
    request: bool,
    hold: bool,
    transfer: bool,
    open_or_retry: bool,
    evidence: bool,
) -> bool {
    evidence_matches_body!(request, hold, transfer, open_or_retry, evidence)
}

#[cfg(not(verus_keep_ghost))]
pub const fn replay_matches(same_payload: bool) -> bool {
    replay_body!(same_payload)
}

#[cfg(not(verus_keep_ghost))]
pub const fn monotone(old_rank: u8, new_rank: u8) -> bool {
    monotone_body!(old_rank, new_rank)
}

#[cfg(not(verus_keep_ghost))]
pub const fn checked_requirement(floor: u128, unit: u128, count: u128) -> Option<u128> {
    checked_requirement_body!(floor, unit, count, u128::MAX, 0u128)
}

#[cfg(not(verus_keep_ghost))]
pub const fn resources_sufficient(
    eth: u128,
    required_eth: u128,
    cycles: u128,
    required_cycles: u128,
) -> bool {
    resources_sufficient_body!(eth, required_eth, cycles, required_cycles)
}

#[cfg(not(verus_keep_ghost))]
pub const fn bad_fee_reprice_amount(
    gross: u128,
    service_fee: u128,
    ledger_fee: u128,
    minimum: u128,
    definitive_bad_fee: bool,
    uncertain_outcome_seen: bool,
) -> Option<u128> {
    bad_fee_reprice_amount_body!(
        gross,
        service_fee,
        ledger_fee,
        minimum,
        definitive_bad_fee,
        uncertain_outcome_seen
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn fee_delta_once(was_transferred: bool, is_transferred: bool, fee: u128) -> u128 {
    fee_delta_once_body!(was_transferred, is_transferred, fee, 0u128)
}

/// Returns the liability left after a release settlement, rejecting arithmetic overflow and
/// over-discharge. A valid economic terminal release must return `Some(0)`.
#[cfg(not(verus_keep_ghost))]
pub const fn terminal_liability_residual(
    liability: u128,
    amount_out: u128,
    service_fee: u128,
    ledger_fee: u128,
) -> Option<u128> {
    terminal_liability_residual_body!(liability, amount_out, service_fee, ledger_fee, u128::MAX)
}

#[cfg(not(verus_keep_ghost))]
pub const fn refund_operation_binding(
    refund_kind: bool,
    operation_id_matches: bool,
    payload_hash_matches: bool,
    calldata_matches: bool,
) -> bool {
    refund_operation_binding_body!(
        refund_kind,
        operation_id_matches,
        payload_hash_matches,
        calldata_matches
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn checked_counter_transition(
    current: u64,
    was_active: bool,
    is_active: bool,
) -> Option<u64> {
    match counter_delta(was_active, is_active) {
        1 => next_attempt_body!(current, u64::MAX, 1u64),
        -1 if current == 0 => None,
        -1 => Some(current - 1),
        _ => Some(current),
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn nonce_too_low_is_submitted(provider_agreement: bool, local_hash_found: bool) -> bool {
    nonce_too_low_submitted_body!(provider_agreement, local_hash_found)
}

#[cfg(not(verus_keep_ghost))]
pub const fn mint_admission_total(consumed: u128, reserved: u128, candidate: u128) -> Option<u128> {
    mint_admission_total_body!(consumed, reserved, candidate, u128::MAX)
}

#[cfg(not(verus_keep_ghost))]
pub const fn nonce_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
}

#[cfg(not(verus_keep_ghost))]
pub const fn can_assign_nonce(nonce_initialized: bool, has_prepared: bool) -> bool {
    nonce_initialized && !has_prepared
}

#[cfg(not(verus_keep_ghost))]
pub const fn payout_allowed(reserve: u128, pending: u128, amount: u128, fee: u128) -> bool {
    payout_allowed_body!(reserve, pending, amount, fee, u128::MAX)
}

#[cfg(not(verus_keep_ghost))]
pub const fn payout_debit(confirmed_first_time: bool, amount: u128, fee: u128) -> Option<u128> {
    if !confirmed_first_time {
        Some(0)
    } else if amount > u128::MAX - fee {
        None
    } else {
        Some(amount + fee)
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn administrator_authorized(
    action: u8,
    is_pause: bool,
    is_finance: bool,
    is_governance: bool,
) -> bool {
    authorized_body!(
        action,
        is_pause,
        is_finance,
        is_governance,
        0u8,
        1u8,
        2u8,
        3u8,
        4u8
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn audit_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
}

/// Compact phase transition used by the rich Deposit state machine.
#[cfg(not(verus_keep_ghost))]
pub const fn deposit_phase_step(state: u8, event: u8) -> u8 {
    deposit_step_body!(state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn deposit_phase_allows(state: u8, event: u8) -> bool {
    deposit_phase_step(state, event) != state
}

/// Compact phase transition used by the rich Withdrawal state machine.
#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_phase_step(state: u8, event: u8) -> u8 {
    withdrawal_step_body!(
        state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_phase_allows(state: u8, event: u8) -> bool {
    withdrawal_phase_step(state, event) != state || (state == 1 && event == 1)
}

#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg(verus_keep_ghost)]
verus! {
    pub open spec fn scan_complete_spec(next: int, tip: int, watermark: int, archives: bool, matched: bool) -> bool {
        scan_complete_body!(next, tip, watermark, archives, matched)
    }

    pub open spec fn refund_allowed_spec(base_pending: bool, release_transfer_proven_absent: bool) -> bool {
        refund_allowed_body!(base_pending, release_transfer_proven_absent)
    }

    pub open spec fn next_attempt_spec(attempt: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(attempt, max, one)
    }

    pub open spec fn counter_delta_spec(was_active: bool, is_active: bool) -> int {
        let zero: int = 0;
        let one: int = 1;
        let minus_one: int = -1;
        counter_delta_body!(was_active, is_active, zero, one, minus_one)
    }

    pub open spec fn evidence_matches_spec(request: bool, hold: bool, transfer: bool, open_or_retry: bool, evidence: bool) -> bool {
        evidence_matches_body!(request, hold, transfer, open_or_retry, evidence)
    }

    pub open spec fn replay_matches_spec(same_payload: bool) -> bool {
        replay_body!(same_payload)
    }


    pub open spec fn monotone_spec(old_rank: int, new_rank: int) -> bool {
        monotone_body!(old_rank, new_rank)
    }


    pub open spec fn checked_requirement_spec(floor: int, unit: int, count: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        let zero: int = 0;
        checked_requirement_body!(floor, unit, count, max, zero)
    }

    pub open spec fn resources_sufficient_spec(eth: int, required_eth: int, cycles: int, required_cycles: int) -> bool {
        resources_sufficient_body!(eth, required_eth, cycles, required_cycles)
    }

    pub open spec fn bad_fee_reprice_amount_spec(
        gross: int,
        service_fee: int,
        ledger_fee: int,
        minimum: int,
        definitive_bad_fee: bool,
        uncertain_outcome_seen: bool,
    ) -> Option<int> {
        bad_fee_reprice_amount_body!(
            gross,
            service_fee,
            ledger_fee,
            minimum,
            definitive_bad_fee,
            uncertain_outcome_seen
        )
    }

    pub open spec fn fee_delta_once_spec(was_transferred: bool, is_transferred: bool, fee: int) -> int {
        let zero: int = 0;
        fee_delta_once_body!(was_transferred, is_transferred, fee, zero)
    }

    pub open spec fn terminal_liability_residual_spec(
        liability: int,
        amount_out: int,
        service_fee: int,
        ledger_fee: int,
    ) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        terminal_liability_residual_body!(
            liability, amount_out, service_fee, ledger_fee, max)
    }

    pub open spec fn refund_operation_binding_spec(
        refund_kind: bool,
        operation_id_matches: bool,
        payload_hash_matches: bool,
        calldata_matches: bool,
    ) -> bool {
        refund_operation_binding_body!(
            refund_kind, operation_id_matches, payload_hash_matches, calldata_matches)
    }

    pub open spec fn checked_counter_transition_spec(
        current: int,
        was_active: bool,
        is_active: bool,
    ) -> Option<int> {
        if was_active == is_active { Some(current) }
        else if is_active {
            let max: int = 18446744073709551615;
            let one: int = 1;
            next_attempt_body!(current, max, one)
        } else if current == 0 { None }
        else { Some(current - 1) }
    }

    pub open spec fn nonce_too_low_is_submitted_spec(
        provider_agreement: bool,
        local_hash_found: bool,
    ) -> bool {
        nonce_too_low_submitted_body!(provider_agreement, local_hash_found)
    }

    pub open spec fn mint_admission_total_spec(consumed: int, reserved: int, candidate: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        mint_admission_total_body!(consumed, reserved, candidate, max)
    }

    pub open spec fn nonce_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
    }

    pub open spec fn can_assign_nonce_spec(nonce_initialized: bool, has_prepared: bool) -> bool {
        nonce_initialized && !has_prepared
    }

    pub open spec fn payout_allowed_spec(reserve: int, pending: int, amount: int, fee: int) -> bool {
        let max: int = 340282366920938463463374607431768211455;
        payout_allowed_body!(reserve, pending, amount, fee, max)
    }

    pub open spec fn payout_debit_spec(confirmed_first_time: bool, amount: int, fee: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        if !confirmed_first_time { Some(0) }
        else if amount > max - fee { None }
        else { Some(amount + fee) }
    }

    pub open spec fn administrator_authorized_spec(action: int, pause: bool, finance: bool, governance: bool) -> bool {
        let pause_action: int = 0;
        let resume_action: int = 1;
        let recipient_action: int = 2;
        let payout_action: int = 3;
        let rotate_action: int = 4;
        authorized_body!(action, pause, finance, governance, pause_action, resume_action, recipient_action, payout_action, rotate_action)
    }

    pub open spec fn audit_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
    }

    pub open spec fn deposit_phase_step_spec(state: int, event: int) -> int {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5; let six: int = 6;
        deposit_step_body!(state, event, zero, one, two, three, four, five, six)
    }

    pub open spec fn deposit_phase_allows_spec(state: int, event: int) -> bool {
        deposit_phase_step_spec(state, event) != state
    }

    pub open spec fn withdrawal_phase_step_spec(state: int, event: int) -> int {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5; let six: int = 6; let seven: int = 7;
        let eight: int = 8; let nine: int = 9; let ten: int = 10; let eleven: int = 11;
        withdrawal_step_body!(state, event, zero, one, two, three, four, five, six,
            seven, eight, nine, ten, eleven)
    }

    pub open spec fn withdrawal_phase_allows_spec(state: int, event: int) -> bool {
        withdrawal_phase_step_spec(state, event) != state || (state == 1 && event == 1)
    }

    pub open spec fn deposit_phase_run_spec(state: int, events: Seq<int>) -> int
        decreases events.len()
    {
        if events.len() == 0 { state }
        else { deposit_phase_run_spec(deposit_phase_step_spec(state, events[0]), events.drop_first()) }
    }

    pub open spec fn withdrawal_phase_run_spec(state: int, events: Seq<int>) -> int
        decreases events.len()
    {
        if events.len() == 0 { state }
        else { withdrawal_phase_run_spec(withdrawal_phase_step_spec(state, events[0]), events.drop_first()) }
    }

    pub open spec fn deposit_fee_delta_spec(state: int, event: int, fee: int) -> int {
        if state == 2 && event == 4 { fee } else { 0 }
    }

    pub open spec fn withdrawal_fee_delta_spec(state: int, event: int, fee: int) -> int {
        if state == 1 && event == 2 { fee } else { 0 }
    }

    pub open spec fn deposit_fee_total_spec(state: int, events: Seq<int>, fee: int) -> int
        decreases events.len()
    {
        if events.len() == 0 { 0 }
        else {
            deposit_fee_delta_spec(state, events[0], fee)
                + deposit_fee_total_spec(
                    deposit_phase_step_spec(state, events[0]), events.drop_first(), fee)
        }
    }

    pub open spec fn withdrawal_fee_total_spec(state: int, events: Seq<int>, fee: int) -> int
        decreases events.len()
    {
        if events.len() == 0 { 0 }
        else {
            withdrawal_fee_delta_spec(state, events[0], fee)
                + withdrawal_fee_total_spec(
                    withdrawal_phase_step_spec(state, events[0]), events.drop_first(), fee)
        }
    }

    pub open spec fn asset_backed_spec(
        escrow: int,
        base_supply: int,
        fee_reserve: int,
        confirmed_unminted_deposits: int,
        unreleased_withdrawals: int,
    ) -> bool {
        asset_backed_body!(
            escrow,
            base_supply,
            fee_reserve,
            confirmed_unminted_deposits,
            unreleased_withdrawals
        )
    }

    pub open spec fn payout_reserved_spec(fee_reserve: int, pending_payout_debit: int) -> bool {
        0 <= pending_payout_debit <= fee_reserve
    }

    pub open spec fn ambiguous_outbound_world_spec(
        transfer_happened: bool,
        escrow: int,
        fee_reserve: int,
        unreleased: int,
        amount_out: int,
        ledger_fee: int,
        service_fee: int,
    ) -> (int, int, int) {
        if transfer_happened {
            (escrow - amount_out - ledger_fee, fee_reserve + service_fee,
                unreleased - amount_out - ledger_fee - service_fee)
        } else {
            (escrow, fee_reserve, unreleased)
        }
    }
}
