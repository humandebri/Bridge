// These expression macros are the single source for the Cargo executable functions and their
// Verus spec views. Keep them free of allocation, traits, I/O, and canister APIs.
macro_rules! scan_complete_body {
    ($next:expr, $tip:expr, $watermark:expr, $archives:expr, $matched:expr) => {
        $archives && !$matched && $watermark >= $tip && $next > $tip
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

macro_rules! refresh_owner_matches_body {
    ($current:expr, $claimant:expr) => {
        match $current {
            Some(owner) => owner == $claimant,
            None => false,
        }
    };
}

macro_rules! reserve_token_matches_body {
    (
        $expected_withdrawals:expr,
        $expected_amount:expr,
        $expected_operations:expr,
        $expected_generation:expr,
        $current_withdrawals:expr,
        $current_amount:expr,
        $current_operations:expr,
        $current_generation:expr
    ) => {
        $expected_withdrawals == $current_withdrawals
            && $expected_amount == $current_amount
            && $expected_operations == $current_operations
            && $expected_generation == $current_generation
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

macro_rules! fee_delta_once_body {
    ($was_transferred:expr, $is_transferred:expr, $fee:expr, $zero:expr) => {
        if !$was_transferred && $is_transferred {
            $fee
        } else {
            $zero
        }
    };
}

macro_rules! release_transfer_matches_body {
    ($transfer_amount:expr, $transfer_fee:expr, $amount_out:expr, $ledger_fee:expr) => {
        $transfer_amount == $amount_out && $transfer_fee == $ledger_fee
    };
}

macro_rules! committed_quote_matches_body {
    ($amount:expr, $amount_out:expr, $service_fee:expr, $max:expr) => {{
        $service_fee < $amount
            && $amount_out <= $max - $service_fee
            && $amount_out + $service_fee == $amount
    }};
}

macro_rules! outbound_settlement_body {
    ($amount_out:expr, $ledger_fee:expr, $service_fee:expr, $max:expr) => {{
        if $ledger_fee > $service_fee
            || $amount_out > $max - $ledger_fee
            || $amount_out > $max - $service_fee
        {
            None
        } else {
            Some((
                $amount_out + $ledger_fee,
                $service_fee - $ledger_fee,
                $amount_out + $service_fee,
            ))
        }
    }};
}

macro_rules! nonce_too_low_submitted_body {
    ($provider_agreement:expr, $local_hash_found:expr) => {
        $provider_agreement && $local_hash_found
    };
}

macro_rules! canonical_probe_matches_body {
    ($receipt_block:expr, $snapshot_block:expr) => {
        $receipt_block == $snapshot_block
    };
}

macro_rules! withdrawal_liability_indexed_body {
    ($state:expr, $observed:expr, $release_pending:expr, $reconciliation_hold:expr) => {
        $state == $observed || $state == $release_pending || $state == $reconciliation_hold
    };
}

macro_rules! evm_operation_indexed_body {
    ($state:expr, $queued:expr, $prepared:expr, $submitted:expr) => {
        $state == $queued || $state == $prepared || $state == $submitted
    };
}

macro_rules! reconciliation_hold_indexed_body {
    ($state:expr, $open:expr) => {
        $state == $open
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

macro_rules! deposit_refund_body {
    ($gross:expr, $ledger_fee:expr) => {{
        if $gross <= $ledger_fee {
            None
        } else {
            Some($gross - $ledger_fee)
        }
    }};
}

macro_rules! authorized_body {
    ($action:expr, $pause:expr, $governance:expr, $pause_action:expr, $resume_action:expr, $payout_action:expr, $rotate_action:expr) => {
        ($action == $pause_action && $pause)
            || (($action == $payout_action
                || $action == $resume_action
                || $action == $rotate_action)
                && $governance)
    };
}

macro_rules! deposit_step_body {
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr, $six:expr, $seven:expr, $eight:expr, $nine:expr, $ten:expr, $eleven:expr) => {{
        if $state == $zero && $event == $zero {
            $one
        } else if $state == $zero && $event == $one {
            $five
        } else if $state == $zero && $event == $two {
            $ten
        } else if $state == $one && $event == $three {
            $two
        } else if $state == $one && $event == $four {
            $six
        } else if $state == $six && $event == $five {
            $nine
        } else if $state == $six && $event == $six {
            $seven
        } else if $state == $six && $event == $seven {
            $eight
        } else if $state == $eight && $event == $eight {
            $six
        } else if $state == $two && $event == $nine {
            $three
        } else if $state == $two && $event == $ten {
            $four
        } else if $state == $four && $event == $eleven {
            $two
        } else {
            $state
        }
    }};
}

macro_rules! withdrawal_step_body {
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr) => {{
        if $state == $zero && $event == $zero {
            $one
        } else if $state == $one && $event == $one {
            $one
        } else if $state == $one && $event == $two {
            $two
        } else if $state == $one && $event == $three {
            $three
        } else if $state == $three && $event == $four {
            $two
        } else if $state == $three && $event == $five {
            $one
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
pub const fn refresh_owner_matches(current: Option<u64>, claimant: u64) -> bool {
    refresh_owner_matches_body!(current, claimant)
}

#[cfg(not(verus_keep_ghost))]
pub const fn refresh_generation_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
}

#[cfg(not(verus_keep_ghost))]
#[allow(clippy::too_many_arguments)]
pub const fn reserve_token_matches(
    expected_withdrawals: u64,
    expected_amount: u128,
    expected_operations: u64,
    expected_generation: u64,
    current_withdrawals: u64,
    current_amount: u128,
    current_operations: u64,
    current_generation: u64,
) -> bool {
    reserve_token_matches_body!(
        expected_withdrawals,
        expected_amount,
        expected_operations,
        expected_generation,
        current_withdrawals,
        current_amount,
        current_operations,
        current_generation
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn lease_generation_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
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
#[cfg(not(verus_keep_ghost))]
pub const fn fee_delta_once(was_transferred: bool, is_transferred: bool, fee: u128) -> u128 {
    fee_delta_once_body!(was_transferred, is_transferred, fee, 0u128)
}

/// Binds the ICRC transfer identity to the exact release settlement persisted for this attempt.
#[cfg(not(verus_keep_ghost))]
pub const fn release_transfer_matches(
    transfer_amount: u128,
    transfer_fee: u128,
    amount_out: u128,
    ledger_fee: u128,
) -> bool {
    release_transfer_matches_body!(transfer_amount, transfer_fee, amount_out, ledger_fee)
}

#[cfg(not(verus_keep_ghost))]
pub const fn committed_quote_matches(amount: u128, amount_out: u128, service_fee: u128) -> bool {
    committed_quote_matches_body!(amount, amount_out, service_fee, u128::MAX)
}

/// Returns `(escrow_debit, fee_reserve_credit, liability_debit)` for a successful
/// outbound Ledger transfer. `None` rejects fee inversion or arithmetic overflow.
#[cfg(not(verus_keep_ghost))]
pub const fn outbound_settlement(
    amount_out: u128,
    ledger_fee: u128,
    service_fee: u128,
) -> Option<(u128, u128, u128)> {
    outbound_settlement_body!(amount_out, ledger_fee, service_fee, u128::MAX)
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
pub const fn canonical_probe_matches(receipt_block: u64, snapshot_block: u64) -> bool {
    canonical_probe_matches_body!(receipt_block, snapshot_block)
}

#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_liability_indexed(state: u8) -> bool {
    withdrawal_liability_indexed_body!(state, 0u8, 1u8, 3u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn evm_operation_indexed(state: u8) -> bool {
    evm_operation_indexed_body!(state, 0u8, 1u8, 2u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn reconciliation_hold_indexed(state: u8) -> bool {
    reconciliation_hold_indexed_body!(state, 0u8)
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
pub const fn deposit_refund_amount(gross: u128, ledger_fee: u128) -> Option<u128> {
    deposit_refund_body!(gross, ledger_fee)
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
pub const fn administrator_authorized(action: u8, is_pause: bool, is_governance: bool) -> bool {
    authorized_body!(action, is_pause, is_governance, 0u8, 1u8, 2u8, 3u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn audit_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
}

/// Compact phase transition used by the rich Deposit state machine.
#[cfg(not(verus_keep_ghost))]
pub const fn deposit_phase_step(state: u8, event: u8) -> u8 {
    deposit_step_body!(state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn deposit_phase_allows(state: u8, event: u8) -> bool {
    deposit_phase_step(state, event) != state || (state == 6 && event == 7)
}

/// Compact phase transition used by the rich Withdrawal state machine.
#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_phase_step(state: u8, event: u8) -> u8 {
    withdrawal_step_body!(state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8)
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

    pub open spec fn refresh_owner_matches_spec(current: Option<int>, claimant: int) -> bool {
        refresh_owner_matches_body!(current, claimant)
    }

    pub open spec fn refresh_generation_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
    }

    pub open spec fn reserve_token_matches_spec(
        expected_withdrawals: int,
        expected_amount: int,
        expected_operations: int,
        expected_generation: int,
        current_withdrawals: int,
        current_amount: int,
        current_operations: int,
        current_generation: int,
    ) -> bool {
        reserve_token_matches_body!(
            expected_withdrawals,
            expected_amount,
            expected_operations,
            expected_generation,
            current_withdrawals,
            current_amount,
            current_operations,
            current_generation
        )
    }

    pub open spec fn lease_generation_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
    }


    pub open spec fn checked_requirement_spec(floor: int, unit: int, count: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        let zero: int = 0;
        checked_requirement_body!(floor, unit, count, max, zero)
    }

    pub open spec fn resources_sufficient_spec(eth: int, required_eth: int, cycles: int, required_cycles: int) -> bool {
        resources_sufficient_body!(eth, required_eth, cycles, required_cycles)
    }

    pub open spec fn fee_delta_once_spec(was_transferred: bool, is_transferred: bool, fee: int) -> int {
        let zero: int = 0;
        fee_delta_once_body!(was_transferred, is_transferred, fee, zero)
    }

    pub open spec fn release_transfer_matches_spec(
        transfer_amount: int,
        transfer_fee: int,
        amount_out: int,
        ledger_fee: int,
    ) -> bool {
        release_transfer_matches_body!(transfer_amount, transfer_fee, amount_out, ledger_fee)
    }

    pub open spec fn committed_quote_matches_spec(
        amount: int,
        amount_out: int,
        service_fee: int,
    ) -> bool {
        let max: int = 340282366920938463463374607431768211455;
        committed_quote_matches_body!(amount, amount_out, service_fee, max)
    }

    pub open spec fn outbound_settlement_spec(
        amount_out: int,
        ledger_fee: int,
        service_fee: int,
    ) -> Option<(int, int, int)> {
        let max: int = 340282366920938463463374607431768211455;
        outbound_settlement_body!(amount_out, ledger_fee, service_fee, max)
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

    pub open spec fn canonical_probe_matches_spec(receipt_block: int, snapshot_block: int) -> bool {
        canonical_probe_matches_body!(receipt_block, snapshot_block)
    }

    pub open spec fn withdrawal_liability_indexed_spec(state: int) -> bool {
        let observed: int = 0;
        let release_pending: int = 1;
        let reconciliation_hold: int = 3;
        withdrawal_liability_indexed_body!(state, observed, release_pending, reconciliation_hold)
    }

    pub open spec fn evm_operation_indexed_spec(state: int) -> bool {
        let queued: int = 0;
        let prepared: int = 1;
        let submitted: int = 2;
        evm_operation_indexed_body!(state, queued, prepared, submitted)
    }

    pub open spec fn reconciliation_hold_indexed_spec(state: int) -> bool {
        let open: int = 0;
        reconciliation_hold_indexed_body!(state, open)
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

    pub open spec fn deposit_refund_amount_spec(gross: int, ledger_fee: int) -> Option<int> {
        deposit_refund_body!(gross, ledger_fee)
    }

    pub open spec fn payout_debit_spec(confirmed_first_time: bool, amount: int, fee: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        if !confirmed_first_time { Some(0) }
        else if amount > max - fee { None }
        else { Some(amount + fee) }
    }

    pub open spec fn administrator_authorized_spec(action: int, pause: bool, governance: bool) -> bool {
        let pause_action: int = 0;
        let resume_action: int = 1;
        let payout_action: int = 2;
        let rotate_action: int = 3;
        authorized_body!(action, pause, governance, pause_action, resume_action, payout_action, rotate_action)
    }

    pub open spec fn audit_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
    }

    pub open spec fn deposit_phase_step_spec(state: int, event: int) -> int {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5; let six: int = 6; let seven: int = 7;
        let eight: int = 8; let nine: int = 9; let ten: int = 10; let eleven: int = 11;
        deposit_step_body!(state, event, zero, one, two, three, four, five, six, seven, eight, nine, ten, eleven)
    }

    pub open spec fn deposit_phase_allows_spec(state: int, event: int) -> bool {
        deposit_phase_step_spec(state, event) != state || (state == 6 && event == 7)
    }

    pub open spec fn withdrawal_phase_step_spec(state: int, event: int) -> int {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5;
        withdrawal_step_body!(state, event, zero, one, two, three, four, five)
    }

    pub open spec fn withdrawal_phase_allows_spec(state: int, event: int) -> bool {
        withdrawal_phase_step_spec(state, event) != state
            || (state == 1 && event == 1)
    }

    pub open spec fn reverted_phase_recovery_spec(event: int) -> bool {
        deposit_phase_step_spec(4, event) != 4 <==> event == 11
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
        if state == 2 && event == 9 { fee } else { 0 }
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
            (escrow - amount_out - ledger_fee, fee_reserve + service_fee - ledger_fee,
                unreleased - amount_out - service_fee)
        } else {
            (escrow, fee_reserve, unreleased)
        }
    }
}
