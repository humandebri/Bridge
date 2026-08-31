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

verus! {
pub const MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS: u64 = 300;
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

macro_rules! deposit_ledger_block_transition_body {
    ($funding:expr, $refund:expr, $event:expr, $block:expr, $preserve:expr, $funded:expr, $refunded:expr) => {{
        if $event == $preserve {
            Some(($funding, $refund))
        } else if $event == $funded {
            match $funding {
                None => Some((Some($block), $refund)),
                Some(current) if current == $block => Some(($funding, $refund)),
                Some(_) => None,
            }
        } else if $event == $refunded {
            match ($funding, $refund) {
                (Some(_), None) => Some(($funding, Some($block))),
                (Some(_), Some(current)) if current == $block => Some(($funding, $refund)),
                _ => None,
            }
        } else {
            None
        }
    }};
}

macro_rules! withdrawal_ledger_block_transition_body {
    ($release:expr, $event:expr, $block:expr, $preserve:expr, $released:expr) => {{
        if $event == $preserve {
            Some($release)
        } else if $event == $released {
            match $release {
                None => Some(Some($block)),
                Some(current) if current == $block => Some($release),
                Some(_) => None,
            }
        } else {
            None
        }
    }};
}

macro_rules! refresh_owner_matches_body {
    ($current:expr, $claimant:expr) => {
        match $current {
            Some(owner) => owner == $claimant,
            None => false,
        }
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

macro_rules! signing_cycle_requirement_body {
    ($required_reserve:expr, $signing_cost:expr, $call_margin:expr, $max:expr) => {{
        if $signing_cost > $max - $required_reserve {
            None
        } else {
            let reserve_and_signing = $required_reserve + $signing_cost;
            if $call_margin > $max - reserve_and_signing {
                None
            } else {
                Some(reserve_and_signing + $call_margin)
            }
        }
    }};
}

macro_rules! transaction_liability_body {
    ($gas_limit:expr, $max_fee_per_gas:expr, $l1_fee:expr, $value:expr, $max:expr, $zero:expr) => {{
        if $value > $max - $l1_fee {
            None
        } else {
            let fixed = $l1_fee + $value;
            if $gas_limit != $zero && $max_fee_per_gas > ($max - fixed) / $gas_limit {
                None
            } else {
                Some(fixed + $gas_limit * $max_fee_per_gas)
            }
        }
    }};
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

macro_rules! canonical_probe_matches_body {
    ($receipt_block:expr, $snapshot_block:expr) => {
        $receipt_block == $snapshot_block
    };
}

macro_rules! withdrawal_id_is_admissible_body {
    ($well_formed_len:expr, $minimum_nonzero:expr, $observed_ge_minimum:expr) => {
        $well_formed_len && $minimum_nonzero && $observed_ge_minimum
    };
}

macro_rules! activation_base_preflight_matches_body {
    ($signer_matches:expr, $deposits_paused:expr, $withdrawals_paused:expr) => {
        $signer_matches && $deposits_paused && $withdrawals_paused
    };
}

macro_rules! activation_postcondition_matches_body {
    ($deposits_paused:expr, $withdrawals_paused:expr) => {
        !$deposits_paused && !$withdrawals_paused
    };
}

macro_rules! withdrawal_finalized_identity_quorum_body {
    ($first:expr, $second:expr, $third:expr) => {{
        match ($first, $second, $third) {
            (Some(a), Some(b), Some(c)) if a == b || a == c => Some(a),
            (Some(_), Some(b), Some(c)) if b == c => Some(b),
            (Some(a), Some(b), None) | (Some(a), None, Some(b)) | (None, Some(a), Some(b)) => {
                if a == b {
                    Some(a)
                } else {
                    None
                }
            }
            _ => None,
        }
    }};
}

macro_rules! withdrawal_finalized_checkpoint_attestation_body {
    ($head:expr, $observation:expr, $checkpoint:expr) => {{
        match ($head, $observation) {
            (Some(head), Some(observation))
                if head.block_number >= $checkpoint && observation.block_number == $checkpoint =>
            {
                Some(observation)
            }
            _ => None,
        }
    }};
}

macro_rules! runtime_attestation_matches_body {
    ($observation_present:expr, $chain_id_matches:expr, $runtime_hash_matches:expr) => {
        $observation_present && $chain_id_matches && $runtime_hash_matches
    };
}

macro_rules! withdrawal_liability_indexed_body {
    ($state:expr, $observed:expr, $release_pending:expr, $reconciliation_hold:expr) => {
        $state == $observed || $state == $release_pending || $state == $reconciliation_hold
    };
}

macro_rules! reconciliation_hold_indexed_body {
    ($state:expr, $open:expr) => {
        $state == $open
    };
}

macro_rules! deposit_nonterminal_indexed_body {
    ($state:expr, $refunded:expr, $cancelled:expr, $minted:expr) => {
        $state != $refunded && $state != $cancelled && $state != $minted
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
    ($service_fee:expr, $minimum_service_fee:expr, $maximum_service_fee:expr) => {
        $minimum_service_fee <= $service_fee && $service_fee <= $maximum_service_fee
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
    ($scheduled:expr, $active:expr, $stopped:expr, $overdue:expr, $expired:expr) => {
        (!$active || $expired) && (!$scheduled || $stopped || $overdue || $expired)
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

macro_rules! refund_request_identity_decision_body {
    ($authenticated:expr, $allow:expr, $anonymous:expr) => {
        if !$authenticated {
            $anonymous
        } else {
            $allow
        }
    };
}

macro_rules! deposit_continuation_decision_body {
    (
        $authenticated:expr,
        $authorization_phase:expr,
        $retryable_stop:expr,
        $allow:expr,
        $anonymous:expr,
        $wrong_state:expr
    ) => {
        if !$authenticated {
            $anonymous
        } else if $authorization_phase && $retryable_stop {
            $allow
        } else {
            $wrong_state
        }
    };
}

macro_rules! confirmation_caller_authorized_body {
    ($non_anonymous:expr, $relayer:expr, $governance:expr, $pause:expr) => {
        $non_anonymous && ($relayer || $governance || $pause)
    };
}

macro_rules! confirmation_roles_distinct_body {
    ($relayer_governance:expr, $relayer_pause:expr, $governance_pause:expr, $allow_staging:expr) => {
        !$governance_pause && !$relayer_pause && ($allow_staging || !$relayer_governance)
    };
}

macro_rules! notification_admission_body {
    ($global_count:expr, $caller_count:expr, $global_limit:expr, $caller_limit:expr) => {
        $global_count < $global_limit && $caller_count < $caller_limit
    };
}

macro_rules! notification_failure_cooldown_active_body {
    ($hash_matches:expr, $now_ns:expr, $retry_after_ns:expr) => {
        $hash_matches && $now_ns < $retry_after_ns
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

macro_rules! funding_reconciliation_decision_body {
    (
        $complete_absence:expr,
        $final_scan:expr,
        $dedup_expired:expr,
        $wait:expr,
        $restart_fresh:expr,
        $release:expr
    ) => {
        if !$complete_absence {
            $wait
        } else if !$final_scan {
            $restart_fresh
        } else if $dedup_expired {
            $release
        } else {
            $wait
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
    pub enum RefundRequestIdentityDecision {
        Allow,
        AnonymousCaller,
    }

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum DepositContinuationDecision {
        Allow,
        AnonymousCaller,
        WrongState,
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

    #[cfg_attr(not(verus_keep_ghost), derive(Clone, Copy, Debug, PartialEq, Eq))]
    pub enum FundingReconciliationDecision {
        Wait,
        RestartFresh,
        Release,
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
    pub enum DepositIdentityDecision {
        Allow,
        Conflict,
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

}

macro_rules! deposit_refund_body {
    ($gross:expr, $service_fee:expr, $ledger_fee:expr) => {{
        if $gross <= $service_fee || $gross - $service_fee <= $ledger_fee {
            None
        } else {
            Some($gross - $service_fee - $ledger_fee)
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

macro_rules! deposit_transition_body {
    ($state:expr, $event:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr, $six:expr, $seven:expr, $eight:expr, $nine:expr, $ten:expr, $eleven:expr) => {{
        if $state == $zero && $event == $zero {
            Some($one)
        } else if $state == $zero && $event == $one {
            Some($five)
        } else if $state == $zero && $event == $two {
            Some($nine)
        } else if $state == $one && $event == $three {
            Some($two)
        } else if $state == $one && $event == $four {
            Some($four)
        } else if $state == $two && $event == $four {
            Some($four)
        } else if $state == $two && $event == $five {
            Some($three)
        } else if $state == $three && $event == $four {
            Some($four)
        } else if $state == $four && $event == $six {
            Some($ten)
        } else if $state == $four && $event == $seven {
            Some($six)
        } else if $state == $six && $event == $eight {
            Some($eight)
        } else if $state == $six && $event == $nine {
            Some($seven)
        } else {
            None
        }
    }};
}

macro_rules! authorization_commit_allowed_body {
    ($identity:expr, $amounts:expr, $domain:expr, $origin:expr, $pristine:expr, $deadline:expr) => {
        $identity && $amounts && $domain && $origin && $pristine && $deadline
    };
}

macro_rules! expiry_refund_allowed_body {
    ($binding:expr, $processed:expr, $timestamp:expr, $deadline:expr) => {
        $binding && !$processed && $timestamp > $deadline
    };
}

macro_rules! mint_finalization_allowed_body {
    ($binding:expr, $receipt_succeeded:expr, $receipt_block:expr, $finalized_block:expr) => {
        $binding && $receipt_succeeded && $receipt_block <= $finalized_block
    };
}

macro_rules! mint_authorization_has_minimum_remaining_time_body {
    ($observed_timestamp:expr, $deadline:expr, $minimum_remaining:expr, $maximum:expr) => {{
        $observed_timestamp <= $maximum - $minimum_remaining
            && $observed_timestamp + $minimum_remaining <= $deadline
    }};
}

macro_rules! signature_install_allowed_body {
    (
        $dispatched:expr, $absent:expr, $length:expr, $observed_timestamp:expr,
        $deadline:expr, $minimum_remaining:expr, $maximum:expr
    ) => {
        $dispatched
            && $absent
            && $length
            && mint_authorization_has_minimum_remaining_time_body!(
                $observed_timestamp,
                $deadline,
                $minimum_remaining,
                $maximum
            )
    };
}

macro_rules! refund_start_allowed_body {
    ($attempt:expr, $policy:expr) => {
        $attempt && $policy
    };
}

macro_rules! deposit_reservation_active_body {
    ($state:expr, $authorization_pending:expr, $authorization_available:expr) => {
        $state == $authorization_pending || $state == $authorization_available
    };
}

macro_rules! deposit_charge_fee_body {
    ($state:expr, $event:expr, $authorization_pending:expr, $signed:expr) => {
        $state == $authorization_pending && $event == $signed
    };
}

macro_rules! deposit_numeric_effects_body {
    (
        $state:expr, $event:expr, $gross:expr, $net:expr, $fee:expr, $reserved:expr,
        $amount_zero:expr, $zero:expr, $one:expr, $two:expr, $three:expr, $four:expr, $five:expr,
        $six:expr, $seven:expr, $eight:expr, $nine:expr, $ten:expr, $eleven:expr
    ) => {{
        let next = deposit_transition_body!(
            $state, $event, $zero, $one, $two, $three, $four, $five, $six, $seven, $eight, $nine,
            $ten, $eleven
        );
        let was_reserved = deposit_reservation_active_body!($state, $two, $three);
        let after_reserved = match next {
            Some(next_state) => deposit_reservation_active_body!(next_state, $two, $three),
            None => false,
        };
        let authorization_signed = deposit_charge_fee_body!($state, $event, $two, $five);
        let mint_completed = $state == $four && $event == $six;
        let refund_completed = $state == $six && $event == $eight;
        let terminal_liability = if $net == $amount_zero { $gross } else { $net };
        (
            if after_reserved {
                if was_reserved {
                    $reserved
                } else {
                    $net
                }
            } else {
                $amount_zero
            },
            if !was_reserved && after_reserved {
                $net
            } else {
                $amount_zero
            },
            if was_reserved && !after_reserved && next.is_some() {
                $reserved
            } else {
                $amount_zero
            },
            if authorization_signed {
                $fee
            } else {
                $amount_zero
            },
            if authorization_signed {
                $fee
            } else if mint_completed || refund_completed {
                terminal_liability
            } else {
                $amount_zero
            },
            if refund_completed {
                terminal_liability
            } else {
                $amount_zero
            },
            if mint_completed { $net } else { $amount_zero },
        )
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

macro_rules! withdrawal_transition_effects_body {
    (
        $state:expr, $event:expr, $amount_out:expr, $ledger_fee:expr, $service_fee:expr,
        $max:expr, $zero_state:expr, $release_pending:expr, $paid:expr, $hold:expr,
        $start:expr, $retry:expr, $succeeded:expr, $ambiguous:expr, $hold_succeeded:expr,
        $hold_absent:expr, $amount_zero:expr
    ) => {{
        let next = withdrawal_step_body!(
            $state,
            $event,
            $zero_state,
            $release_pending,
            $paid,
            $hold,
            $hold_succeeded,
            $hold_absent
        );
        if next == $state && !($state == $release_pending && $event == $retry) {
            None
        } else if ($state == $release_pending && $event == $succeeded)
            || ($state == $hold && $event == $hold_succeeded)
        {
            match outbound_settlement_body!($amount_out, $ledger_fee, $service_fee, $max) {
                None => None,
                Some((escrow_debit, reserve_credit, liability_debit)) => {
                    Some((next, escrow_debit, reserve_credit, liability_debit))
                }
            }
        } else if ($state == $zero_state && $event == $start)
            || ($state == $release_pending && $event == $ambiguous)
            || ($state == $hold && $event == $hold_absent)
            || ($state == $release_pending && $event == $retry)
        {
            Some((next, $amount_zero, $amount_zero, $amount_zero))
        } else {
            None
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

/// Applies only ledger-success evidence to the deposit block indexes. Event codes are
/// preserve=0, funding-success=1, and refund-success=2. `None` is a conflicting replay.
#[cfg(not(verus_keep_ghost))]
pub const fn deposit_ledger_block_transition(
    funding: Option<u128>,
    refund: Option<u128>,
    event: u8,
    block: u128,
) -> Option<(Option<u128>, Option<u128>)> {
    deposit_ledger_block_transition_body!(funding, refund, event, block, 0u8, 1u8, 2u8)
}

/// Applies only ledger-success evidence to the withdrawal release block index.
/// Event codes are preserve=0 and release-success=1. `None` is a conflicting replay.
#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_ledger_block_transition(
    release: Option<u128>,
    event: u8,
    block: u128,
) -> Option<Option<u128>> {
    withdrawal_ledger_block_transition_body!(release, event, block, 0u8, 1u8)
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
pub const fn lease_generation_next(current: u64) -> Option<u64> {
    next_attempt_body!(current, u64::MAX, 1u64)
}

#[cfg(not(verus_keep_ghost))]
pub const fn checked_requirement(floor: u128, unit: u128, count: u128) -> Option<u128> {
    checked_requirement_body!(floor, unit, count, u128::MAX, 0u128)
}

/// Returns the minimum liquid-cycle balance required before a paid threshold-signing call.
/// `None` rejects arithmetic overflow rather than weakening the reserve.
#[cfg(not(verus_keep_ghost))]
pub const fn signing_cycle_requirement(
    required_reserve: u128,
    signing_cost: u128,
    call_margin: u128,
) -> Option<u128> {
    signing_cycle_requirement_body!(required_reserve, signing_cost, call_margin, u128::MAX)
}

#[cfg(not(verus_keep_ghost))]
pub const fn transaction_liability_wei(
    gas_limit: u128,
    max_fee_per_gas: u128,
    l1_fee_upper_bound_wei: u128,
    transaction_value: u128,
) -> Option<u128> {
    transaction_liability_body!(
        gas_limit,
        max_fee_per_gas,
        l1_fee_upper_bound_wei,
        transaction_value,
        u128::MAX,
        0u128
    )
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

/// Accepts only Base withdrawal IDs at or above the immutable deployment boundary.
/// Both values use the contract's 32-byte big-endian uint256 representation.
#[cfg(not(verus_keep_ghost))]
pub fn withdrawal_id_is_admissible(observed: &[u8; 32], minimum: &[u8]) -> bool {
    withdrawal_id_is_admissible_body!(
        minimum.len() == 32,
        minimum.iter().any(|byte| *byte != 0),
        observed.as_slice() >= minimum
    )
}

#[cfg(not(verus_keep_ghost))]
pub fn activation_base_preflight_matches(
    signer_matches: bool,
    deposits_paused: bool,
    withdrawals_paused: bool,
) -> bool {
    activation_base_preflight_matches_body!(signer_matches, deposits_paused, withdrawals_paused)
}

#[cfg(not(verus_keep_ghost))]
pub fn activation_postcondition_matches(deposits_paused: bool, withdrawals_paused: bool) -> bool {
    activation_postcondition_matches_body!(deposits_paused, withdrawals_paused)
}

/// Accepts only an exact finalized block identity attested by at least two of three providers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WithdrawalFinalizedIdentity {
    pub block_number: u64,
    pub block_hash: [u8; 32],
}

#[cfg(not(verus_keep_ghost))]
pub fn withdrawal_finalized_identity_quorum(
    first: Option<WithdrawalFinalizedIdentity>,
    second: Option<WithdrawalFinalizedIdentity>,
    third: Option<WithdrawalFinalizedIdentity>,
) -> Option<WithdrawalFinalizedIdentity> {
    withdrawal_finalized_identity_quorum_body!(first, second, third)
}

/// Accepts only checkpoint identities returned by providers whose own finalized
/// head has reached that checkpoint, then requires an exact two-of-three match.
#[cfg(not(verus_keep_ghost))]
pub fn withdrawal_finalized_checkpoint_quorum(
    finalized_heads: [Option<WithdrawalFinalizedIdentity>; 3],
    checkpoint_observations: [Option<WithdrawalFinalizedIdentity>; 3],
    checkpoint: u64,
) -> Option<WithdrawalFinalizedIdentity> {
    let eligible = [
        withdrawal_finalized_checkpoint_attestation_body!(
            finalized_heads[0],
            checkpoint_observations[0],
            checkpoint
        ),
        withdrawal_finalized_checkpoint_attestation_body!(
            finalized_heads[1],
            checkpoint_observations[1],
            checkpoint
        ),
        withdrawal_finalized_checkpoint_attestation_body!(
            finalized_heads[2],
            checkpoint_observations[2],
            checkpoint
        ),
    ];
    self::withdrawal_finalized_identity_quorum(eligible[0], eligible[1], eligible[2])
}

/// Selects the newest checkpoint that every available honest pair can safely
/// have reached. Exact hash quorum is established by a second query at this
/// height; head hashes are deliberately not trusted here.
#[cfg(not(verus_keep_ghost))]
pub fn withdrawal_common_checkpoint(
    first: Option<WithdrawalFinalizedIdentity>,
    second: Option<WithdrawalFinalizedIdentity>,
    third: Option<WithdrawalFinalizedIdentity>,
) -> Option<u64> {
    let mut heights = [None, None, None];
    let mut len = 0usize;
    for identity in [first, second, third].into_iter().flatten() {
        heights[len] = Some(identity.block_number);
        len += 1;
    }
    if len < 2 {
        return None;
    }
    let heights = &mut heights[..len];
    heights.sort_unstable();
    Some(heights[if len == 3 { 1 } else { 0 }].expect("successful head has a height"))
}

/// Allows reuse of a persisted runtime attestation only when every immutable-config
/// binding is present and matches exactly.
#[cfg(not(verus_keep_ghost))]
pub const fn runtime_attestation_matches(
    observation_present: bool,
    chain_id_matches: bool,
    runtime_hash_matches: bool,
) -> bool {
    runtime_attestation_matches_body!(observation_present, chain_id_matches, runtime_hash_matches)
}

#[cfg(not(verus_keep_ghost))]
pub const fn withdrawal_liability_indexed(state: u8) -> bool {
    withdrawal_liability_indexed_body!(state, 0u8, 1u8, 3u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn reconciliation_hold_indexed(state: u8) -> bool {
    reconciliation_hold_indexed_body!(state, 0u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn deposit_nonterminal_indexed(state: u8) -> bool {
    deposit_nonterminal_indexed_body!(state, 8u8, 9u8, 10u8)
}

#[cfg(not(verus_keep_ghost))]
pub const fn mint_admission_total(consumed: u128, reserved: u128, candidate: u128) -> Option<u128> {
    mint_admission_total_body!(consumed, reserved, candidate, u128::MAX)
}

#[cfg(not(verus_keep_ghost))]
pub const fn notification_failure_cooldown_active(
    hash_matches: bool,
    now_ns: u64,
    retry_after_ns: u64,
) -> bool {
    notification_failure_cooldown_active_body!(hash_matches, now_ns, retry_after_ns)
}

verus! {
    pub fn deposit_identity_decision(processed: bool) -> (result: DepositIdentityDecision)
        ensures
            result == if processed {
                DepositIdentityDecision::Conflict
            } else {
                DepositIdentityDecision::Allow
            },
    {
        if processed {
            DepositIdentityDecision::Conflict
        } else {
            DepositIdentityDecision::Allow
        }
    }
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
pub const fn service_fee_change_allowed(
    service_fee: u128,
    minimum_service_fee: u128,
    maximum_service_fee: u128,
) -> bool {
    service_fee_change_allowed_body!(service_fee, minimum_service_fee, maximum_service_fee)
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
    scheduled: bool,
    active: bool,
    stopped: bool,
    overdue: bool,
    expired: bool,
) -> bool {
    manual_claim_allowed_body!(scheduled, active, stopped, overdue, expired)
}

verus! {
    pub fn refund_request_identity_decision(
        authenticated: bool,
    ) -> (result: RefundRequestIdentityDecision)
        ensures
            match result {
                RefundRequestIdentityDecision::Allow =>
                    authenticated,
                RefundRequestIdentityDecision::AnonymousCaller =>
                    !authenticated,
            },
    {
        refund_request_identity_decision_body!(
            authenticated,
            RefundRequestIdentityDecision::Allow,
            RefundRequestIdentityDecision::AnonymousCaller
        )
    }
}

verus! {
    pub fn confirmation_caller_authorized(
        non_anonymous: bool,
        relayer: bool,
        governance: bool,
        pause: bool,
    ) -> (result: bool)
        ensures
            result == (non_anonymous && (relayer || governance || pause)),
    {
        confirmation_caller_authorized_body!(non_anonymous, relayer, governance, pause)
    }
}

verus! {
    pub fn confirmation_roles_distinct(
        relayer_is_governance: bool,
        relayer_is_pause: bool,
        governance_is_pause: bool,
        allow_staging_relayer_governance: bool,
    ) -> (result: bool)
        ensures
            result == (!governance_is_pause
                && !relayer_is_pause
                && (allow_staging_relayer_governance || !relayer_is_governance)),
    {
        confirmation_roles_distinct_body!(
            relayer_is_governance,
            relayer_is_pause,
            governance_is_pause,
            allow_staging_relayer_governance
        )
    }
}

verus! {
    pub fn deposit_continuation_decision(
        authenticated: bool,
        authorization_phase: bool,
        retryable_stop: bool,
    ) -> (result: DepositContinuationDecision)
        ensures
            match result {
                DepositContinuationDecision::Allow =>
                    authenticated && authorization_phase && retryable_stop,
                DepositContinuationDecision::AnonymousCaller => !authenticated,
                DepositContinuationDecision::WrongState =>
                    authenticated && (!authorization_phase || !retryable_stop),
            },
    {
        deposit_continuation_decision_body!(
            authenticated,
            authorization_phase,
            retryable_stop,
            DepositContinuationDecision::Allow,
            DepositContinuationDecision::AnonymousCaller,
            DepositContinuationDecision::WrongState
        )
    }
}

verus! {
    pub fn notification_admission_allowed(
        global_count: u16,
        caller_count: u16,
        global_limit: u16,
        caller_limit: u16,
    ) -> (result: bool)
        ensures
            result == (global_count < global_limit && caller_count < caller_limit),
    {
        notification_admission_body!(global_count, caller_count, global_limit, caller_limit)
    }
}

verus! {
    pub fn notification_ingestion_allowed(
        ingestion_count: u16,
        ingestion_limit: u16,
    ) -> (result: bool)
        ensures result == (ingestion_count < ingestion_limit),
    {
        ingestion_count < ingestion_limit
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
    pub fn funding_reconciliation_decision(
        complete_absence: bool,
        final_scan: bool,
        dedup_expired: bool,
    ) -> (result: FundingReconciliationDecision)
        ensures
            match result {
                FundingReconciliationDecision::Wait =>
                    !complete_absence || (final_scan && !dedup_expired),
                FundingReconciliationDecision::RestartFresh =>
                    complete_absence && !final_scan,
                FundingReconciliationDecision::Release =>
                    complete_absence && final_scan && dedup_expired,
            },
    {
        funding_reconciliation_decision_body!(
            complete_absence,
            final_scan,
            dedup_expired,
            FundingReconciliationDecision::Wait,
            FundingReconciliationDecision::RestartFresh,
            FundingReconciliationDecision::Release
        )
    }
}

verus! {
    pub fn manual_claim_decision(
        scheduled: bool,
        active: bool,
        stopped: bool,
        overdue: bool,
        expired: bool,
    ) -> (result: ManualClaimDecision)
        ensures
            match result {
                ManualClaimDecision::Allow =>
                    (!active || expired)
                        && (!scheduled || stopped || overdue || expired),
                ManualClaimDecision::AutomaticProgressPending =>
                    !(!active || expired)
                        || !(!scheduled || stopped || overdue || expired),
            },
    {
        manual_claim_decision_body!(
            manual_claim_allowed_body!(
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
pub const fn deposit_refund_amount(
    gross: u128,
    service_fee: u128,
    ledger_fee: u128,
) -> Option<u128> {
    deposit_refund_body!(gross, service_fee, ledger_fee)
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

pub const fn deposit_transition(state: u8, event: u8) -> Option<u8> {
    deposit_transition_body!(
        state, event, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8
    )
}

pub const fn authorization_commit_allowed(
    identity_matches: bool,
    amounts_match: bool,
    domain_valid: bool,
    origin_valid: bool,
    record_pristine: bool,
    deadline_matches: bool,
) -> bool {
    authorization_commit_allowed_body!(
        identity_matches,
        amounts_match,
        domain_valid,
        origin_valid,
        record_pristine,
        deadline_matches
    )
}

#[cfg(not(verus_keep_ghost))]
pub const fn expiry_refund_allowed(
    binding_matches: bool,
    deposit_processed: bool,
    finalized_timestamp: u64,
    deadline: u64,
) -> bool {
    expiry_refund_allowed_body!(
        binding_matches,
        deposit_processed,
        finalized_timestamp,
        deadline
    )
}

pub const fn mint_finalization_allowed(
    binding_matches: bool,
    receipt_succeeded: bool,
    receipt_block: u64,
    finalized_block: u64,
) -> bool {
    mint_finalization_allowed_body!(
        binding_matches,
        receipt_succeeded,
        receipt_block,
        finalized_block
    )
}

pub const fn mint_authorization_has_minimum_remaining_time(
    observed_timestamp: u64,
    deadline: u64,
) -> bool {
    mint_authorization_has_minimum_remaining_time_body!(
        observed_timestamp,
        deadline,
        MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
        u64::MAX
    )
}

pub const fn signature_install_allowed(
    signature_dispatched: bool,
    signature_absent: bool,
    signature_length_valid: bool,
    observed_timestamp: u64,
    deadline: u64,
) -> bool {
    signature_install_allowed_body!(
        signature_dispatched,
        signature_absent,
        signature_length_valid,
        observed_timestamp,
        deadline,
        MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
        u64::MAX
    )
}

pub const fn refund_start_allowed(attempt_matches: bool, policy_matches: bool) -> bool {
    refund_start_allowed_body!(attempt_matches, policy_matches)
}

pub const fn deposit_reservation_active(state: u8) -> bool {
    deposit_reservation_active_body!(state, 2u8, 3u8)
}

pub const fn deposit_charge_service_fee(state: u8, event: u8) -> bool {
    deposit_charge_fee_body!(state, event, 2u8, 5u8)
}

pub const fn deposit_releases_reservation(state: u8, event: u8) -> bool {
    match self::deposit_transition(state, event) {
        None => false,
        Some(next) => deposit_reservation_active(state) && !deposit_reservation_active(next),
    }
}

#[cfg(not(verus_keep_ghost))]
pub const fn deposit_numeric_effects(
    state: u8,
    event: u8,
    gross_amount: u128,
    net_amount: u128,
    service_fee: u128,
    reserved_amount: u128,
) -> (u128, u128, u128, u128, u128, u128, u128) {
    deposit_numeric_effects_body!(
        state,
        event,
        gross_amount,
        net_amount,
        service_fee,
        reserved_amount,
        0u128,
        0u8,
        1u8,
        2u8,
        3u8,
        4u8,
        5u8,
        6u8,
        7u8,
        8u8,
        9u8,
        10u8,
        11u8
    )
}

verus! {
    pub fn deposit_transition_effects(
        state: u8,
        event: u8,
        gross_amount: u128,
        net_amount: u128,
        service_fee: u128,
        reserved_amount: u128,
    ) -> (result: (u128, u128, u128, u128, u128, u128, u128))
        ensures result == deposit_numeric_effects_body!(
            state, event, gross_amount, net_amount, service_fee, reserved_amount,
            0u128, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8, 11u8),
    {
        deposit_numeric_effects_body!(
            state,
            event,
            gross_amount,
            net_amount,
            service_fee,
            reserved_amount,
            0u128,
            0u8,
            1u8,
            2u8,
            3u8,
            4u8,
            5u8,
            6u8,
            7u8,
            8u8,
            9u8,
            10u8,
            11u8
        )
    }
}

verus! {
#[derive(Clone, Copy)]
#[cfg_attr(not(verus_keep_ghost), derive(Debug, PartialEq, Eq))]
pub enum DepositEventGuard {
    Funding,
    CommitAuthorization {
        quote_valid: bool,
        fixed_fields_match: bool,
        canonical_domain_strings: bool,
        deadline_valid: bool,
        pristine: bool,
    },
    InstallSignature {
        dispatched: bool,
        signature_absent: bool,
        signature_length_valid: bool,
        observed_timestamp: u64,
        deadline: u64,
    },
    MarkRefundAvailable {
        policy_allowed: bool,
    },
    MintFinalization {
        fixed_fields_match: bool,
        receipt_succeeded: bool,
        receipt_block: u64,
        finalized_block: u64,
        audit_complete: bool,
    },
    StartRefund {
        attempt_matches: bool,
        policy_matches: bool,
    },
    RefundResult,
}

impl DepositEventGuard {
    pub const fn matches_event(self, event: u8) -> bool {
        match self {
            Self::Funding => event <= 2,
            Self::CommitAuthorization { .. } => event == 3,
            Self::StartRefund { .. } => event == 7,
            Self::InstallSignature { .. } => event == 5,
            Self::MarkRefundAvailable { .. } => event == 4,
            Self::MintFinalization { .. } => event == 6,
            Self::RefundResult => event == 8 || event == 9,
        }
    }

    pub const fn accepts(self) -> bool {
        match self {
            Self::Funding | Self::RefundResult => true,
            Self::MarkRefundAvailable { policy_allowed } => policy_allowed,
            Self::CommitAuthorization {
                quote_valid,
                fixed_fields_match,
                canonical_domain_strings,
                deadline_valid,
                pristine,
            } => authorization_commit_allowed_body!(
                fixed_fields_match,
                quote_valid,
                canonical_domain_strings,
                fixed_fields_match,
                pristine,
                deadline_valid
            ),
            Self::InstallSignature {
                dispatched,
                signature_absent,
                signature_length_valid,
                observed_timestamp,
                deadline,
            } => signature_install_allowed_body!(
                dispatched,
                signature_absent,
                signature_length_valid,
                observed_timestamp,
                deadline,
                MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
                u64::MAX
            ),
            Self::MintFinalization {
                fixed_fields_match,
                receipt_succeeded,
                receipt_block,
                finalized_block,
                audit_complete,
            } => mint_finalization_allowed_body!(
                (fixed_fields_match && audit_complete),
                receipt_succeeded,
                receipt_block,
                finalized_block
            ),
            Self::StartRefund {
                attempt_matches,
                policy_matches,
            } => refund_start_allowed_body!(attempt_matches, policy_matches),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(verus_keep_ghost), derive(Debug, PartialEq, Eq))]
pub struct DepositTransitionInput {
    pub state: u8,
    pub event: u8,
    pub guard: DepositEventGuard,
    pub same_payload: bool,
    pub gross_amount: u128,
    pub net_amount: u128,
    pub service_fee: u128,
    pub reserved_amount: u128,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(verus_keep_ghost), derive(Debug, PartialEq, Eq))]
pub struct DepositEffects {
    pub next_state: u8,
    pub reservation_active: bool,
    pub release_reservation: bool,
    pub charge_service_fee: bool,
    pub reservation_after: u128,
    pub reservation_add: u128,
    pub reservation_release: u128,
    pub fee_credit: u128,
    pub pending_liability_debit: u128,
    pub escrow_debit: u128,
    pub mint_supply_increase: u128,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(verus_keep_ghost), derive(Debug, PartialEq, Eq))]
pub enum DepositTransitionDecision {
    Idempotent,
    Apply(DepositEffects),
    Reject,
}
}

verus! {
    pub fn deposit_transition_decision(
        input: DepositTransitionInput,
    ) -> (result: DepositTransitionDecision)
        ensures ((match result {
                DepositTransitionDecision::Idempotent => true,
                _ => false,
            }) == input.same_payload)
            && (match result {
                DepositTransitionDecision::Apply(effects) =>
                    deposit_transition_body!(
                        input.state, input.event,
                        0u8, 1u8, 2u8, 3u8, 4u8, 5u8,
                        6u8, 7u8, 8u8, 9u8, 10u8, 11u8)
                        == Some(effects.next_state)
                    && effects.reservation_active
                        == deposit_reservation_active_body!(effects.next_state, 2u8, 3u8)
                    && effects.release_reservation
                        == (deposit_reservation_active_body!(input.state, 2u8, 3u8)
                            && !deposit_reservation_active_body!(effects.next_state, 2u8, 3u8))
                    && effects.charge_service_fee
                        == deposit_charge_fee_body!(input.state, input.event, 2u8, 5u8)
                    && (
                        effects.reservation_after,
                        effects.reservation_add,
                        effects.reservation_release,
                        effects.fee_credit,
                        effects.pending_liability_debit,
                        effects.escrow_debit,
                        effects.mint_supply_increase,
                    ) == deposit_numeric_effects_body!(
                        input.state, input.event, input.gross_amount, input.net_amount,
                        input.service_fee, input.reserved_amount,
                        0u128, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8,
                        6u8, 7u8, 8u8, 9u8, 10u8, 11u8),
                _ => true,
            }),
    {
        if input.same_payload {
            return DepositTransitionDecision::Idempotent;
        }
        if !input.guard.matches_event(input.event) || !input.guard.accepts() {
            return DepositTransitionDecision::Reject;
        }
        match deposit_transition_body!(
            input.state, input.event,
            0u8, 1u8, 2u8, 3u8, 4u8, 5u8,
            6u8, 7u8, 8u8, 9u8, 10u8, 11u8) {
            None => DepositTransitionDecision::Reject,
            Some(next_state) => {
                let after_reserved = deposit_reservation_active_body!(next_state, 2u8, 3u8);
                let release_reservation =
                    deposit_reservation_active_body!(input.state, 2u8, 3u8)
                    && !after_reserved;
                let charge_service_fee =
                    deposit_charge_fee_body!(input.state, input.event, 2u8, 5u8);
                let (
                    reservation_after,
                    reservation_add,
                    reservation_release,
                    fee_credit,
                    pending_liability_debit,
                    escrow_debit,
                    mint_supply_increase,
                ) = self::deposit_transition_effects(
                    input.state,
                    input.event,
                    input.gross_amount,
                    input.net_amount,
                    input.service_fee,
                    input.reserved_amount,
                );
                DepositTransitionDecision::Apply(DepositEffects {
                    next_state,
                    reservation_active: after_reserved,
                    release_reservation,
                    charge_service_fee,
                    reservation_after,
                    reservation_add,
                    reservation_release,
                    fee_credit,
                    pending_liability_debit,
                    escrow_debit,
                    mint_supply_increase,
                })
            }
        }
    }
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

verus! {
    pub fn withdrawal_transition_effects(
        state: u8,
        event: u8,
        amount_out: u128,
        ledger_fee: u128,
        service_fee: u128,
    ) -> (result: Option<(u8, u128, u128, u128)>)
        ensures result == withdrawal_transition_effects_body!(
            state, event, amount_out, ledger_fee, service_fee, u128::MAX,
            0u8, 1u8, 2u8, 3u8, 0u8, 1u8, 2u8, 3u8, 4u8, 5u8, 0u128),
    {
        withdrawal_transition_effects_body!(
            state,
            event,
            amount_out,
            ledger_fee,
            service_fee,
            u128::MAX,
            0u8,
            1u8,
            2u8,
            3u8,
            0u8,
            1u8,
            2u8,
            3u8,
            4u8,
            5u8,
            0u128
        )
    }
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

    pub open spec fn deposit_ledger_block_transition_spec(
        funding: Option<int>, refund: Option<int>, event: int, block: int,
    ) -> Option<(Option<int>, Option<int>)> {
        let preserve: int = 0; let funded: int = 1; let refunded: int = 2;
        deposit_ledger_block_transition_body!(
            funding, refund, event, block, preserve, funded, refunded)
    }

    pub open spec fn withdrawal_ledger_block_transition_spec(
        release: Option<int>, event: int, block: int,
    ) -> Option<Option<int>> {
        let preserve: int = 0; let released: int = 1;
        withdrawal_ledger_block_transition_body!(release, event, block, preserve, released)
    }


    pub open spec fn refresh_owner_matches_spec(current: Option<int>, claimant: int) -> bool {
        refresh_owner_matches_body!(current, claimant)
    }

    pub open spec fn refresh_generation_next_spec(current: int) -> Option<int> {
        let max: int = 18446744073709551615;
        let one: int = 1;
        next_attempt_body!(current, max, one)
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

    pub open spec fn signing_cycle_requirement_spec(
        required_reserve: int,
        signing_cost: int,
        call_margin: int,
    ) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        signing_cycle_requirement_body!(required_reserve, signing_cost, call_margin, max)
    }

    pub open spec fn transaction_liability_wei_spec(
        gas_limit: int,
        max_fee_per_gas: int,
        l1_fee_upper_bound_wei: int,
        transaction_value: int,
    ) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        let zero: int = 0;
        transaction_liability_body!(gas_limit, max_fee_per_gas, l1_fee_upper_bound_wei, transaction_value, max, zero)
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

    pub open spec fn withdrawal_id_is_admissible_spec(
        well_formed_len: bool,
        minimum_nonzero: bool,
        observed_ge_minimum: bool,
    ) -> bool {
        withdrawal_id_is_admissible_body!(
            well_formed_len, minimum_nonzero, observed_ge_minimum)
    }

    pub open spec fn activation_base_preflight_matches_spec(
        signer_matches: bool,
        deposits_paused: bool,
        withdrawals_paused: bool,
    ) -> bool {
        activation_base_preflight_matches_body!(
            signer_matches, deposits_paused, withdrawals_paused)
    }

    pub open spec fn activation_postcondition_matches_spec(
        deposits_paused: bool,
        withdrawals_paused: bool,
    ) -> bool {
        activation_postcondition_matches_body!(deposits_paused, withdrawals_paused)
    }

    pub open spec fn withdrawal_finalized_identity_quorum_spec(
        first: Option<(int, int)>,
        second: Option<(int, int)>,
        third: Option<(int, int)>,
    ) -> Option<(int, int)> {
        match (first, second, third) {
            (Some(a), Some(b), Some(c)) => {
                if (a.0 == b.0 && a.1 == b.1) || (a.0 == c.0 && a.1 == c.1) {
                    Some(a)
                } else if b.0 == c.0 && b.1 == c.1 {
                    Some(b)
                } else {
                    None
                }
            },
            (Some(a), Some(b), None)
            | (Some(a), None, Some(b))
            | (None, Some(a), Some(b)) => {
                if a.0 == b.0 && a.1 == b.1 { Some(a) } else { None }
            },
            _ => None,
        }
    }

    pub open spec fn withdrawal_finalized_checkpoint_quorum_spec(
        first_head: Option<(int, int)>,
        second_head: Option<(int, int)>,
        third_head: Option<(int, int)>,
        first_observation: Option<(int, int)>,
        second_observation: Option<(int, int)>,
        third_observation: Option<(int, int)>,
        checkpoint: int,
    ) -> Option<(int, int)> {
        let first = match (first_head, first_observation) {
            (Some(head), Some(observation)) => {
                if head.0 >= checkpoint && observation.0 == checkpoint {
                    Some(observation)
                } else {
                    None
                }
            },
            _ => None,
        };
        let second = match (second_head, second_observation) {
            (Some(head), Some(observation)) => {
                if head.0 >= checkpoint && observation.0 == checkpoint {
                    Some(observation)
                } else {
                    None
                }
            },
            _ => None,
        };
        let third = match (third_head, third_observation) {
            (Some(head), Some(observation)) => {
                if head.0 >= checkpoint && observation.0 == checkpoint {
                    Some(observation)
                } else {
                    None
                }
            },
            _ => None,
        };
        withdrawal_finalized_identity_quorum_spec(
            first,
            second,
            third,
        )
    }

    pub open spec fn runtime_attestation_matches_spec(
        observation_present: bool,
        chain_id_matches: bool,
        runtime_hash_matches: bool,
    ) -> bool {
        runtime_attestation_matches_body!(
            observation_present, chain_id_matches, runtime_hash_matches)
    }

    pub open spec fn withdrawal_liability_indexed_spec(state: int) -> bool {
        let observed: int = 0;
        let release_pending: int = 1;
        let reconciliation_hold: int = 3;
        withdrawal_liability_indexed_body!(state, observed, release_pending, reconciliation_hold)
    }

    pub open spec fn reconciliation_hold_indexed_spec(state: int) -> bool {
        let open: int = 0;
        reconciliation_hold_indexed_body!(state, open)
    }

    pub open spec fn deposit_nonterminal_indexed_spec(state: int) -> bool {
        let refunded: int = 8;
        let cancelled: int = 9;
        let minted: int = 10;
        deposit_nonterminal_indexed_body!(state, refunded, cancelled, minted)
    }

    pub open spec fn mint_admission_total_spec(consumed: int, reserved: int, candidate: int) -> Option<int> {
        let max: int = 340282366920938463463374607431768211455;
        mint_admission_total_body!(consumed, reserved, candidate, max)
    }

    pub open spec fn payout_allowed_spec(reserve: int, pending: int, amount: int, fee: int) -> bool {
        let max: int = 340282366920938463463374607431768211455;
        payout_allowed_body!(reserve, pending, amount, fee, max)
    }

    pub open spec fn service_fee_change_allowed_spec(
        service_fee: int, minimum_service_fee: int, maximum_service_fee: int,
    ) -> bool {
        service_fee_change_allowed_body!(service_fee, minimum_service_fee, maximum_service_fee)
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

    pub open spec fn deposit_refund_amount_spec(
        gross: int, service_fee: int, ledger_fee: int,
    ) -> Option<int> {
        deposit_refund_body!(gross, service_fee, ledger_fee)
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

    pub open spec fn deposit_transition_spec(state: int, event: int) -> Option<int> {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5; let six: int = 6; let seven: int = 7;
        let eight: int = 8; let nine: int = 9; let ten: int = 10; let eleven: int = 11;
        deposit_transition_body!(
            state, event, zero, one, two, three, four, five, six, seven, eight, nine, ten, eleven)
    }

    pub open spec fn authorization_commit_allowed_spec(
        identity: bool, amounts: bool, domain: bool, origin: bool, pristine: bool, deadline: bool
    ) -> bool {
        authorization_commit_allowed_body!(identity, amounts, domain, origin, pristine, deadline)
    }

    pub open spec fn expiry_refund_allowed_spec(
        binding: bool, processed: bool, timestamp: int, deadline: int
    ) -> bool {
        expiry_refund_allowed_body!(binding, processed, timestamp, deadline)
    }

    pub open spec fn mint_finalization_allowed_spec(
        binding: bool, receipt_succeeded: bool, receipt_block: int, finalized_block: int
    ) -> bool {
        mint_finalization_allowed_body!(
            binding, receipt_succeeded, receipt_block, finalized_block)
    }

    pub open spec fn mint_authorization_has_minimum_remaining_time_spec(
        observed_timestamp: int, deadline: int,
    ) -> bool {
        let minimum: int = 300;
        let maximum: int = 18446744073709551615;
        mint_authorization_has_minimum_remaining_time_body!(
            observed_timestamp, deadline, minimum, maximum)
    }

    pub open spec fn signature_install_allowed_spec(
        dispatched: bool, absent: bool, length: bool, observed_timestamp: int, deadline: int,
    ) -> bool {
        let minimum: int = 300;
        let maximum: int = 18446744073709551615;
        signature_install_allowed_body!(
            dispatched,
            absent,
            length,
            observed_timestamp,
            deadline,
            minimum,
            maximum
        )
    }

    pub open spec fn notification_failure_cooldown_active_spec(
        hash_matches: bool, now_ns: int, retry_after_ns: int,
    ) -> bool {
        notification_failure_cooldown_active_body!(hash_matches, now_ns, retry_after_ns)
    }

    pub open spec fn refund_start_allowed_spec(attempt: bool, policy: bool) -> bool {
        refund_start_allowed_body!(attempt, policy)
    }

    pub open spec fn deposit_reservation_active_spec(state: int) -> bool {
        let authorization_pending: int = 2;
        let authorization_available: int = 3;
        deposit_reservation_active_body!(state, authorization_pending, authorization_available)
    }

    pub open spec fn deposit_charge_service_fee_spec(state: int, event: int) -> bool {
        let authorization_pending: int = 2;
        let signed: int = 5;
        deposit_charge_fee_body!(state, event, authorization_pending, signed)
    }

    pub open spec fn deposit_releases_reservation_spec(state: int, event: int) -> bool {
        match deposit_transition_spec(state, event) {
            | None => false,
            | Some(next) =>
                deposit_reservation_active_spec(state)
                    && !deposit_reservation_active_spec(next),
        }
    }

    pub open spec fn deposit_numeric_effects_spec(
        state: int,
        event: int,
        gross_amount: int,
        net_amount: int,
        service_fee: int,
        reserved_amount: int,
    ) -> (int, int, int, int, int, int, int) {
        let zero: int = 0; let one: int = 1; let two: int = 2; let three: int = 3;
        let four: int = 4; let five: int = 5; let six: int = 6; let seven: int = 7;
        let eight: int = 8; let nine: int = 9; let ten: int = 10; let eleven: int = 11;
        deposit_numeric_effects_body!(
            state, event, gross_amount, net_amount, service_fee, reserved_amount,
            zero, zero, one, two, three, four, five, six, seven, eight, nine, ten, eleven)
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

    pub open spec fn withdrawal_phase_run_spec(state: int, events: Seq<int>) -> int
        decreases events.len()
    {
        if events.len() == 0 { state }
        else { withdrawal_phase_run_spec(withdrawal_phase_step_spec(state, events[0]), events.drop_first()) }
    }

}
