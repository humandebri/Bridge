// These expression macros are the single source for the Cargo executable functions and their
// Verus spec views. Keep them free of allocation, traits, I/O, and canister APIs.
#[cfg(not(verus_keep_ghost))]
macro_rules! verus {
    (
        $(#[$struct_attr:meta])*
        pub struct SettlementDecision {
            pub escrow_debit: u128,
            pub reserve_credit: u128,
            pub liability_debit: u128,
        }

        $(#[$function_attr:meta])*
        pub fn settlement_decision(
            $amount_out:ident: u128,
            $ledger_fee:ident: u128,
            $service_fee:ident: u128,
        ) -> ($result:ident: Option<SettlementDecision>)
            ensures $ensures:expr,
        $body:block
    ) => {
        $(#[$struct_attr])*
        pub struct SettlementDecision {
            pub escrow_debit: u128,
            pub reserve_credit: u128,
            pub liability_debit: u128,
        }

        $(#[$function_attr])*
        pub fn settlement_decision(
            $amount_out: u128,
            $ledger_fee: u128,
            $service_fee: u128,
        ) -> Option<SettlementDecision>
        $body
    };
    (
        $(#[$function_attr:meta])*
        pub fn $name:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> ($result:ident: $return_type:ty)
            ensures $ensures:expr,
        $body:block
    ) => {
        $(#[$function_attr])*
        pub fn $name(
            $($argument: $argument_type),*
        ) -> $return_type
        $body
    };
    ($($tokens:tt)*) => {
        $($tokens)*
    };
}

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

macro_rules! withdrawal_finalization_decision_body {
    ($succeeded:expr, $receipt_block:expr, $finalized_block:expr, $retry:expr, $notify:expr, $discard:expr) => {
        match $finalized_block {
            None => $retry,
            Some(finalized) if finalized < $receipt_block => $retry,
            Some(_) if $succeeded => $notify,
            Some(_) => $discard,
        }
    };
}

macro_rules! restored_pending_blocked_body {
    ($existing:expr, $incoming:expr) => {
        match $existing {
            Some(blocked) => blocked,
            None => $incoming,
        }
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

macro_rules! fee_recipient_rotation_allowed_body {
    ($pending_payout_debit:expr, $zero:expr) => {
        $pending_payout_debit == $zero
    };
}

macro_rules! fee_recipient_rotation_decision_body {
    (
        $authorized:expr,
        $anonymous:expr,
        $role_collision:expr,
        $valid_subaccount:expr,
        $no_pending:expr,
        $allow:expr,
        $unauthorized:expr,
        $invalid:expr,
        $busy:expr
    ) => {
        if $anonymous || !$valid_subaccount {
            $invalid
        } else if !$authorized {
            $unauthorized
        } else if $role_collision {
            $invalid
        } else if !$no_pending {
            $busy
        } else {
            $allow
        }
    };
}

macro_rules! service_fee_change_allowed_body {
    ($service_fee:expr, $maximum_service_fee:expr) => {
        $service_fee <= $maximum_service_fee
    };
}

#[cfg(verus_keep_ghost)]
macro_rules! reserve_admission_preserves_requirement_body {
    ($before_reserved:expr, $before_candidate:expr, $after_reserved:expr, $after_candidate:expr) => {
        $before_reserved + $before_candidate == $after_reserved + $after_candidate
    };
}

macro_rules! lease_outcome_is_current_body {
    ($active_generation:expr, $outcome_generation:expr, $active:expr) => {
        $active && $active_generation == $outcome_generation
    };
}

macro_rules! hold_retry_allowed_body {
    ($exact_success:expr, $complete_absence:expr) => {
        $exact_success || $complete_absence
    };
}

macro_rules! hold_resolution_decision_body {
    ($exact_success:expr, $complete_absence:expr, $wait:expr, $success:expr, $absence:expr) => {
        if $exact_success {
            $success
        } else if $complete_absence {
            $absence
        } else {
            $wait
        }
    };
}

macro_rules! manual_claim_allowed_body {
    ($confirmation:expr, $scheduled:expr, $active:expr, $stopped:expr, $overdue:expr, $expired:expr) => {
        !$confirmation
            && (!$active || $expired)
            && (!$scheduled || $stopped || $overdue || $expired)
    };
}

macro_rules! manual_claim_decision_body {
    ($allowed:expr, $allow:expr, $pending:expr) => {
        if $allowed {
            $allow
        } else {
            $pending
        }
    };
}

macro_rules! notification_admission_body {
    ($caller_count:expr, $hash_count:expr, $caller_limit:expr, $hash_limit:expr) => {
        $caller_count < $caller_limit && $hash_count < $hash_limit
    };
}

macro_rules! lease_lane_claim_body {
    (
        $target_active:expr,
        $target_automatic:expr,
        $active_in_lane:expr,
        $capacity:expr,
        $allow:expr,
        $automatic_pending:expr,
        $busy:expr
    ) => {
        if $target_active {
            if $target_automatic {
                $automatic_pending
            } else {
                $busy
            }
        } else if $active_in_lane >= $capacity {
            $busy
        } else {
            $allow
        }
    };
}

macro_rules! funding_attempt_decision_body {
    ($kind:expr, $success:expr, $ambiguous:expr, $release:expr, $retain:expr) => {
        if $kind == 0u8 || $kind == 1u8 {
            $success
        } else if $kind == 2u8 {
            $ambiguous
        } else if $kind == 3u8 {
            $release
        } else {
            $retain
        }
    };
}

verus! {
    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum FeeRecipientRotationDecision {
        Allow,
        Unauthorized,
        InvalidInput,
        Busy,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum HoldResolutionDecision {
        Wait,
        ResolveSucceeded,
        ResolveAbsent,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum ManualClaimDecision {
        Allow,
        AutomaticProgressPending,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum LeaseLaneClaimDecision {
        Allow,
        AutomaticProgressPending,
        Busy,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum FundingAttemptDecision {
        PromoteSuccess,
        PromoteAmbiguous,
        Release,
        Retain,
    }
}

verus! {
    #[cfg_attr(
        not(verus_keep_ghost),
        derive(Clone, Copy, Debug, PartialEq, Eq)
    )]
    pub struct SettlementDecision {
        pub escrow_debit: u128,
        pub reserve_credit: u128,
        pub liability_debit: u128,
    }

    #[cfg_attr(not(verus_keep_ghost), allow(clippy::manual_map))]
    pub fn settlement_decision(
        amount_out: u128,
        ledger_fee: u128,
        service_fee: u128,
    ) -> (result: Option<SettlementDecision>)
        ensures
            match result {
                Some(decision) =>
                    ledger_fee <= service_fee
                        && amount_out <= u128::MAX - ledger_fee
                        && amount_out <= u128::MAX - service_fee
                        && decision.escrow_debit == amount_out + ledger_fee
                        && decision.reserve_credit == service_fee - ledger_fee
                        && decision.liability_debit == amount_out + service_fee,
                None =>
                    ledger_fee > service_fee
                        || amount_out > u128::MAX - ledger_fee
                        || amount_out > u128::MAX - service_fee,
            },
    {
        match outbound_settlement_body!(
            amount_out,
            ledger_fee,
            service_fee,
            u128::MAX
        ) {
            Some((escrow_debit, reserve_credit, liability_debit)) => {
                Some(SettlementDecision {
                    escrow_debit,
                    reserve_credit,
                    liability_debit,
                })
            }
            None => None,
        }
    }
}

verus! {
    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub struct DepositAdmissionDecision {
        pub net_amount: u128,
        pub next_window_total: u128,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub struct ReservationDecision {
        pub reserved: u128,
        pub candidate: u128,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub struct PayoutDecision {
        pub debit: u128,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum LeaseOutcomeDecision {
        Accept,
        Reject,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum WithdrawalFinalizationDecision {
        Retry,
        Notify,
        DiscardReverted,
    }
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
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr, $six:expr, $seven:expr, $eight:expr, $nine:expr) => {{
        if $state == $zero && $event == $zero {
            $one
        } else if $state == $zero && $event == $one {
            $five
        } else if $state == $zero && $event == $two {
            $nine
        } else if $state == $one && $event == $three {
            $two
        } else if $state == $one && $event == $four {
            $six
        } else if $state == $six && $event == $five {
            $eight
        } else if $state == $six && $event == $six {
            $seven
        } else if $state == $two && $event == $seven {
            $three
        } else if $state == $two && $event == $eight {
            $four
        } else if $state == $four && $event == $nine {
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
pub const fn withdrawal_finalization_decision(
    receipt_succeeded: bool,
    receipt_block: u64,
    finalized_block: Option<u64>,
) -> WithdrawalFinalizationDecision {
    withdrawal_finalization_decision_body!(
        receipt_succeeded,
        receipt_block,
        finalized_block,
        WithdrawalFinalizationDecision::Retry,
        WithdrawalFinalizationDecision::Notify,
        WithdrawalFinalizationDecision::DiscardReverted
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn restored_pending_blocked(existing: Option<bool>, incoming: bool) -> bool {
    restored_pending_blocked_body!(existing, incoming)
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

verus! {
    pub fn deposit_admission_decision(
        gross_amount: u128,
        service_fee: u128,
        maximum_service_fee: u128,
        per_deposit_limit: u128,
        consumed: u128,
        reserved: u128,
        window_limit: u128,
    ) -> (result: Option<DepositAdmissionDecision>)
        ensures
            match result {
                Some(decision) =>
                    service_fee <= maximum_service_fee
                        && service_fee < gross_amount
                        && decision.net_amount == gross_amount - service_fee
                        && decision.net_amount <= per_deposit_limit
                        && consumed <= u128::MAX - reserved
                        && consumed + reserved <= u128::MAX - decision.net_amount
                        && decision.next_window_total
                            == consumed + reserved + decision.net_amount
                        && decision.next_window_total <= window_limit,
                None =>
                    service_fee > maximum_service_fee
                        || service_fee >= gross_amount
                        || gross_amount - service_fee > per_deposit_limit
                        || consumed > u128::MAX - reserved
                        || consumed + reserved > u128::MAX - (gross_amount - service_fee)
                        || consumed + reserved + (gross_amount - service_fee) > window_limit,
            },
    {
        if service_fee > maximum_service_fee || service_fee >= gross_amount {
            return None;
        }
        let net_amount = gross_amount - service_fee;
        if net_amount > per_deposit_limit
            || consumed > u128::MAX - reserved
            || consumed + reserved > u128::MAX - net_amount
        {
            return None;
        }
        let next_window_total = consumed + reserved + net_amount;
        if next_window_total <= window_limit {
            Some(DepositAdmissionDecision {
                net_amount,
                next_window_total,
            })
        } else {
            None
        }
    }
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
pub const fn fee_recipient_rotation_allowed(pending_payout_debit: u128) -> bool {
    fee_recipient_rotation_allowed_body!(pending_payout_debit, 0u128)
}

verus! {
    pub fn fee_recipient_rotation_decision(
        authorized: bool,
        anonymous: bool,
        role_collision: bool,
        subaccount_len: usize,
        pending_payout_debit: u128,
    ) -> (result: FeeRecipientRotationDecision)
        ensures
            match result {
                FeeRecipientRotationDecision::Allow =>
                    authorized && !anonymous && !role_collision
                        && (subaccount_len == 0 || subaccount_len == 32)
                        && pending_payout_debit == 0,
                FeeRecipientRotationDecision::Unauthorized =>
                    !anonymous
                        && (subaccount_len == 0 || subaccount_len == 32)
                        && !authorized,
                FeeRecipientRotationDecision::InvalidInput =>
                    anonymous
                        || (subaccount_len != 0 && subaccount_len != 32)
                        || (authorized && role_collision),
                FeeRecipientRotationDecision::Busy =>
                    authorized && !anonymous && !role_collision
                        && (subaccount_len == 0 || subaccount_len == 32)
                        && pending_payout_debit != 0,
            },
    {
        fee_recipient_rotation_decision_body!(
            authorized,
            anonymous,
            role_collision,
            subaccount_len == 0 || subaccount_len == 32,
            pending_payout_debit == 0,
            FeeRecipientRotationDecision::Allow,
            FeeRecipientRotationDecision::Unauthorized,
            FeeRecipientRotationDecision::InvalidInput,
            FeeRecipientRotationDecision::Busy
        )
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn service_fee_change_allowed(service_fee: u128, maximum_service_fee: u128) -> bool {
    service_fee_change_allowed_body!(service_fee, maximum_service_fee)
}

#[cfg(not(verus_keep_ghost))]
pub const fn reserve_admission_preserves_requirement(
    before_reserved: u128,
    before_candidate: u128,
    after_reserved: u128,
    after_candidate: u128,
) -> bool {
    match (
        before_reserved.checked_add(before_candidate),
        after_reserved.checked_add(after_candidate),
    ) {
        (Some(before), Some(after)) => before == after,
        _ => false,
    }
}

verus! {
    pub fn reservation_decision(
        before_reserved: u128,
        candidate: u128,
    ) -> (result: Option<ReservationDecision>)
        ensures
            match result {
                Some(decision) =>
                    before_reserved <= u128::MAX - candidate
                        && decision.reserved == before_reserved + candidate
                        && decision.candidate == 0,
                None => before_reserved > u128::MAX - candidate,
            },
    {
        if before_reserved > u128::MAX - candidate {
            None
        } else {
            Some(ReservationDecision {
                reserved: before_reserved + candidate,
                candidate: 0,
            })
        }
    }
}

verus! {
    pub fn payout_decision(
        reserve: u128,
        pending: u128,
        amount: u128,
        fee: u128,
        confirmed_first_time: bool,
    ) -> (result: Option<PayoutDecision>)
        ensures
            match result {
                Some(decision) =>
                    pending <= reserve
                        && amount <= u128::MAX - fee
                        && amount + fee <= reserve - pending
                        && decision.debit
                            == if confirmed_first_time { amount + fee } else { 0 },
                None =>
                    pending > reserve
                        || amount > u128::MAX - fee
                        || amount + fee > reserve - pending,
            },
    {
        if pending > reserve
            || amount > u128::MAX - fee
            || amount + fee > reserve - pending
        {
            None
        } else {
            Some(PayoutDecision {
                debit: if confirmed_first_time { amount + fee } else { 0 },
            })
        }
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn lease_outcome_is_current(
    active_generation: u64,
    outcome_generation: u64,
    active: bool,
) -> bool {
    lease_outcome_is_current_body!(active_generation, outcome_generation, active)
}

verus! {
    pub fn lease_outcome_decision(
        active_generation: u64,
        outcome_generation: u64,
        active: bool,
    ) -> (result: LeaseOutcomeDecision)
        ensures
            match result {
                LeaseOutcomeDecision::Accept =>
                    active && active_generation == outcome_generation,
                LeaseOutcomeDecision::Reject =>
                    !active || active_generation != outcome_generation,
            },
    {
        if active && active_generation == outcome_generation {
            LeaseOutcomeDecision::Accept
        } else {
            LeaseOutcomeDecision::Reject
        }
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn hold_retry_allowed(exact_success: bool, complete_absence: bool) -> bool {
    hold_retry_allowed_body!(exact_success, complete_absence)
}

verus! {
    pub fn hold_resolution_decision(
        exact_success: bool,
        complete_absence: bool,
    ) -> (result: HoldResolutionDecision)
        ensures
            match result {
                HoldResolutionDecision::ResolveSucceeded => exact_success,
                HoldResolutionDecision::ResolveAbsent => !exact_success && complete_absence,
                HoldResolutionDecision::Wait => !exact_success && !complete_absence,
            },
    {
        hold_resolution_decision_body!(
            exact_success,
            complete_absence,
            HoldResolutionDecision::Wait,
            HoldResolutionDecision::ResolveSucceeded,
            HoldResolutionDecision::ResolveAbsent
        )
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn manual_claim_allowed(
    confirmation: bool,
    scheduled: bool,
    active: bool,
    stopped: bool,
    overdue: bool,
    expired: bool,
) -> bool {
    manual_claim_allowed_body!(confirmation, scheduled, active, stopped, overdue, expired)
}

verus! {
    pub fn notification_admission_allowed(
        caller_count: u8,
        hash_count: u8,
        caller_limit: u8,
        hash_limit: u8,
    ) -> (result: bool)
        ensures
            result == (caller_count < caller_limit && hash_count < hash_limit),
    {
        notification_admission_body!(caller_count, hash_count, caller_limit, hash_limit)
    }
}

verus! {
    pub fn lease_lane_claim_decision(
        target_active: bool,
        target_automatic: bool,
        active_in_lane: u64,
        capacity: u64,
    ) -> (result: LeaseLaneClaimDecision)
        ensures
            match result {
                LeaseLaneClaimDecision::Allow =>
                    !target_active && active_in_lane < capacity,
                LeaseLaneClaimDecision::AutomaticProgressPending =>
                    target_active && target_automatic,
                LeaseLaneClaimDecision::Busy =>
                    (target_active && !target_automatic)
                        || (!target_active && active_in_lane >= capacity),
            },
    {
        lease_lane_claim_body!(
            target_active,
            target_automatic,
            active_in_lane,
            capacity,
            LeaseLaneClaimDecision::Allow,
            LeaseLaneClaimDecision::AutomaticProgressPending,
            LeaseLaneClaimDecision::Busy
        )
    }
}

verus! {
    pub fn funding_attempt_decision(
        outcome_kind: u8,
    ) -> (result: FundingAttemptDecision)
        ensures
            match result {
                FundingAttemptDecision::PromoteSuccess =>
                    outcome_kind == 0u8 || outcome_kind == 1u8,
                FundingAttemptDecision::PromoteAmbiguous => outcome_kind == 2u8,
                FundingAttemptDecision::Release => outcome_kind == 3u8,
                FundingAttemptDecision::Retain =>
                    outcome_kind != 0u8
                        && outcome_kind != 1u8
                        && outcome_kind != 2u8
                        && outcome_kind != 3u8,
            },
    {
        funding_attempt_decision_body!(
            outcome_kind,
            FundingAttemptDecision::PromoteSuccess,
            FundingAttemptDecision::PromoteAmbiguous,
            FundingAttemptDecision::Release,
            FundingAttemptDecision::Retain
        )
    }
}

verus! {
    pub fn manual_claim_decision(
        confirmation: bool,
        scheduled: bool,
        active: bool,
        stopped: bool,
        overdue: bool,
        expired: bool,
    ) -> (result: ManualClaimDecision)
        ensures
            match result {
                ManualClaimDecision::Allow =>
                    !confirmation
                        && (!active || expired)
                        && (!scheduled || stopped || overdue || expired),
                ManualClaimDecision::AutomaticProgressPending =>
                    confirmation
                        || !(!active || expired)
                        || !(!scheduled || stopped || overdue || expired),
            },
    {
        manual_claim_decision_body!(
            manual_claim_allowed_body!(
                confirmation,
                scheduled,
                active,
                stopped,
                overdue,
                expired
            ),
            ManualClaimDecision::Allow,
            ManualClaimDecision::AutomaticProgressPending
        )
    }
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
    deposit_step_body!(state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn deposit_phase_allows(state: u8, event: u8) -> bool {
    deposit_phase_step(state, event) != state
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

    pub open spec fn withdrawal_finalization_decision_spec(
        receipt_succeeded: bool,
        receipt_block: int,
        finalized_block: Option<int>,
    ) -> int {
        let retry: int = 0;
        let notify: int = 1;
        let discard: int = 2;
        withdrawal_finalization_decision_body!(
            receipt_succeeded, receipt_block, finalized_block, retry, notify, discard)
    }

    pub open spec fn restored_pending_blocked_spec(
        existing: Option<bool>, incoming: bool,
    ) -> bool {
        restored_pending_blocked_body!(existing, incoming)
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

    pub open spec fn fee_recipient_rotation_allowed_spec(pending_payout_debit: int) -> bool {
        let zero: int = 0;
        fee_recipient_rotation_allowed_body!(pending_payout_debit, zero)
    }

    pub open spec fn service_fee_change_allowed_spec(
        service_fee: int, maximum_service_fee: int,
    ) -> bool {
        service_fee_change_allowed_body!(service_fee, maximum_service_fee)
    }

    pub open spec fn reserve_admission_preserves_requirement_spec(
        before_reserved: int,
        before_candidate: int,
        after_reserved: int,
        after_candidate: int,
    ) -> bool {
        reserve_admission_preserves_requirement_body!(
            before_reserved, before_candidate, after_reserved, after_candidate)
    }

    pub open spec fn lease_outcome_is_current_spec(
        active_generation: int, outcome_generation: int, active: bool,
    ) -> bool {
        lease_outcome_is_current_body!(active_generation, outcome_generation, active)
    }

    pub open spec fn hold_retry_allowed_spec(
        exact_success: bool, complete_absence: bool,
    ) -> bool {
        hold_retry_allowed_body!(exact_success, complete_absence)
    }

    pub open spec fn manual_claim_allowed_spec(
        confirmation: bool,
        scheduled: bool,
        active: bool,
        stopped: bool,
        overdue: bool,
        expired: bool,
    ) -> bool {
        manual_claim_allowed_body!(
            confirmation, scheduled, active, stopped, overdue, expired)
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
        let eight: int = 8; let nine: int = 9;
        deposit_step_body!(state, event, zero, one, two, three, four, five, six, seven, eight, nine)
    }

    pub open spec fn deposit_phase_allows_spec(state: int, event: int) -> bool {
        deposit_phase_step_spec(state, event) != state
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
        deposit_phase_step_spec(4, event) != 4 <==> event == 9
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
        if state == 2 && event == 7 { fee } else { 0 }
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
