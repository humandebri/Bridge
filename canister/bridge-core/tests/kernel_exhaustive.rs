use bridge_core::{
    administrator_authorized, audit_next, authorization_commit_allowed, checked_counter_transition,
    checked_requirement, counter_delta, deposit_admission_decision, deposit_reservation_active,
    deposit_transition, deposit_transition_decision, evidence_matches, expiry_refund_allowed,
    fee_delta_once, fee_recipient_rotation_allowed, fee_recipient_rotation_decision,
    funding_reconciliation_decision, hold_resolution_decision, lease_generation_next,
    lease_outcome_is_current, manual_claim_decision, mint_admission_total,
    mint_finalization_allowed, next_attempt, outbound_settlement, payout_allowed, payout_debit,
    refresh_generation_next, refresh_owner_matches, release_transfer_matches, replay_matches,
    reservation_decision, reserve_admission_preserves_requirement, resources_sufficient,
    scan_complete, service_fee_change_allowed, settlement_decision, withdrawal_phase_allows,
    withdrawal_phase_step, DepositEventGuard, DepositTransitionDecision, DepositTransitionInput,
    FeeRecipientRotationDecision, FundingReconciliationDecision, HoldResolutionDecision,
    ManualClaimDecision,
};

#[test]
fn boolean_decisions_are_exhaustive() {
    for old in [false, true] {
        for new in [false, true] {
            let expected = match (old, new) {
                (false, true) => 1,
                (true, false) => -1,
                _ => 0,
            };
            assert_eq!(counter_delta(old, new), expected);
        }
    }
    for request in [false, true] {
        for hold in [false, true] {
            for transfer in [false, true] {
                for open in [false, true] {
                    for evidence in [false, true] {
                        assert_eq!(
                            evidence_matches(request, hold, transfer, open, evidence),
                            request && hold && transfer && open && evidence
                        );
                    }
                }
            }
        }
    }
    assert!(replay_matches(true));
    assert!(!replay_matches(false));
}

#[test]
fn reserve_boundaries_and_overflow_are_checked() {
    assert_eq!(checked_requirement(0, 0, 0), Some(0));
    assert_eq!(checked_requirement(7, 3, 4), Some(19));
    assert_eq!(
        checked_requirement(u128::MAX, 0, u128::MAX),
        Some(u128::MAX)
    );
    assert_eq!(checked_requirement(u128::MAX, 1, 1), None);
    assert_eq!(checked_requirement(0, u128::MAX, 2), None);
    for eth_ok in [false, true] {
        for cycles_ok in [false, true] {
            let eth = if eth_ok { 10 } else { 9 };
            let cycles = if cycles_ok { 20 } else { 19 };
            assert_eq!(
                resources_sufficient(eth, 10, cycles, 20),
                eth_ok && cycles_ok
            );
        }
    }
}

#[test]
fn fee_rotation_reserve_admission_and_lease_fences_are_fail_closed() {
    assert!(fee_recipient_rotation_allowed(0));
    assert!(!fee_recipient_rotation_allowed(1));
    assert!(service_fee_change_allowed(10, 10));
    assert!(!service_fee_change_allowed(11, 10));
    assert!(reserve_admission_preserves_requirement(7, 1, 8, 0));
    assert!(!reserve_admission_preserves_requirement(7, 1, 7, 0));
    assert!(!reserve_admission_preserves_requirement(
        u128::MAX,
        1,
        u128::MAX,
        1
    ));
    assert!(lease_outcome_is_current(4, 4, true));
    assert!(!lease_outcome_is_current(4, 3, true));
    assert!(!lease_outcome_is_current(4, 4, false));
}

#[test]
fn typed_decisions_preserve_every_shared_guard() {
    assert_eq!(
        fee_recipient_rotation_decision(true, false, false, 32, 0),
        FeeRecipientRotationDecision::Allow
    );
    assert_eq!(
        fee_recipient_rotation_decision(false, false, false, 32, 0),
        FeeRecipientRotationDecision::Unauthorized
    );
    assert_eq!(
        fee_recipient_rotation_decision(true, false, true, 32, 0),
        FeeRecipientRotationDecision::InvalidInput
    );
    assert_eq!(
        fee_recipient_rotation_decision(true, false, false, 32, 1),
        FeeRecipientRotationDecision::Busy
    );
    assert_eq!(
        hold_resolution_decision(false, false),
        HoldResolutionDecision::Wait
    );
    assert_eq!(
        hold_resolution_decision(true, false),
        HoldResolutionDecision::ResolveSucceeded
    );
    assert_eq!(
        hold_resolution_decision(false, true),
        HoldResolutionDecision::ResolveAbsent
    );
    for complete_absence in [false, true] {
        for final_scan in [false, true] {
            for dedup_expired in [false, true] {
                let expected = if !complete_absence {
                    FundingReconciliationDecision::Wait
                } else if !final_scan {
                    FundingReconciliationDecision::RestartFresh
                } else if dedup_expired {
                    FundingReconciliationDecision::Release
                } else {
                    FundingReconciliationDecision::Wait
                };
                assert_eq!(
                    funding_reconciliation_decision(complete_absence, final_scan, dedup_expired,),
                    expected
                );
            }
        }
    }
    assert_eq!(
        manual_claim_decision(false, false, false, false, false),
        ManualClaimDecision::Allow
    );
    assert_eq!(
        manual_claim_decision(false, false, true, true, true),
        ManualClaimDecision::Allow
    );
    assert_eq!(
        manual_claim_decision(true, true, false, true, false),
        ManualClaimDecision::AutomaticProgressPending,
        "an overdue job must not bypass a still-active lease"
    );
    assert_eq!(
        settlement_decision(100, 3, 5).map(|decision| (
            decision.escrow_debit,
            decision.reserve_credit,
            decision.liability_debit,
        )),
        outbound_settlement(100, 3, 5)
    );
    assert_eq!(
        reservation_decision(7, 3).map(|decision| (decision.reserved, decision.candidate)),
        Some((10, 0))
    );
    assert_eq!(reservation_decision(u128::MAX, 1), None);
    let admission = deposit_admission_decision(105, 5, 5, 100, 0, 0, 100)
        .expect("exact boundary must be admitted");
    assert_eq!(admission.net_amount, 100);
    assert_eq!(admission.next_window_total, 100);
    assert!(deposit_admission_decision(105, 6, 5, 100, 0, 0, 100).is_none());
    assert!(deposit_admission_decision(105, 5, 5, 100, 0, 1, 100).is_none());
}

#[test]
fn mint_admission_includes_existing_reservations_and_checks_overflow() {
    assert_eq!(mint_admission_total(90, 9, 1), Some(100));
    assert_eq!(mint_admission_total(90, 10, 1), Some(101));
    assert_eq!(mint_admission_total(u128::MAX, 0, 0), Some(u128::MAX));
    assert_eq!(mint_admission_total(u128::MAX, 1, 0), None);
    assert_eq!(mint_admission_total(0, u128::MAX, 1), None);
}

#[test]
fn audit_boundaries_are_deterministic() {
    assert_eq!(audit_next(0), Some(1));
    assert_eq!(audit_next(u64::MAX), None);
}

#[test]
fn refresh_reserve_and_lease_tokens_fail_closed() {
    assert!(refresh_owner_matches(Some(7), 7));
    assert!(!refresh_owner_matches(Some(7), 8));
    assert!(!refresh_owner_matches(None, 7));
    assert_eq!(refresh_generation_next(0), Some(1));
    assert_eq!(refresh_generation_next(u64::MAX), None);
    assert_eq!(lease_generation_next(0), Some(1));
    assert_eq!(lease_generation_next(u64::MAX), None);
}

#[test]
fn payout_and_authorization_tables_are_exhaustive() {
    assert!(payout_allowed(12, 2, 7, 3));
    assert!(!payout_allowed(11, 2, 7, 3));
    assert!(!payout_allowed(u128::MAX, 0, u128::MAX, 1));
    assert_eq!(payout_debit(true, 7, 3), Some(10));
    assert_eq!(payout_debit(false, 7, 3), Some(0));
    assert_eq!(payout_debit(true, u128::MAX, 1), None);
    for action in 0..=u8::MAX {
        for pause in [false, true] {
            for governance in [false, true] {
                let expected = (action == 0 && pause)
                    || ((action == 1 || action == 2 || action == 3) && governance);
                assert_eq!(
                    administrator_authorized(action, pause, governance),
                    expected
                );
            }
        }
    }
}

#[test]
fn finite_scan_domain_finds_inclusive_tip_counterexample() {
    for next in 0..=3 {
        for tip in 0..=3 {
            for watermark in 0..=3 {
                for archives in [false, true] {
                    for matched in [false, true] {
                        let accepted = scan_complete(next, tip, watermark, archives, matched);
                        if accepted {
                            assert!(next > tip);
                            assert!(watermark >= tip);
                            assert!(archives);
                            assert!(!matched);
                        }
                    }
                }
            }
        }
    }
    assert!(!scan_complete(3, 3, 3, true, false));
    assert!(scan_complete(4, 3, 3, true, false));
}

#[test]
fn attempt_and_fee_boundaries_are_checked() {
    assert_eq!(next_attempt(0), Some(1));
    assert_eq!(next_attempt(u64::MAX - 1), Some(u64::MAX));
    assert_eq!(next_attempt(u64::MAX), None);
    assert_eq!(fee_delta_once(false, true, 9), 9);
    assert_eq!(fee_delta_once(true, true, 9), 0);
    assert_eq!(fee_delta_once(false, false, 9), 0);
    assert!(release_transfer_matches(85, 5, 85, 5));
    assert!(!release_transfer_matches(84, 5, 85, 5));
    assert!(!release_transfer_matches(85, 6, 85, 5));
    assert_eq!(checked_counter_transition(7, false, true), Some(8));
    assert_eq!(checked_counter_transition(7, true, false), Some(6));
    assert_eq!(checked_counter_transition(0, true, false), None);
}

#[test]
fn outbound_settlement_matches_the_backing_equation_at_boundaries() {
    assert_eq!(outbound_settlement(90, 1, 10), Some((91, 9, 100)));
    assert_eq!(outbound_settlement(90, 10, 10), Some((100, 0, 100)));
    assert_eq!(outbound_settlement(90, 11, 10), None);
    assert_eq!(
        outbound_settlement(u128::MAX, 0, 0),
        Some((u128::MAX, 0, u128::MAX))
    );
    assert_eq!(outbound_settlement(u128::MAX, 1, 1), None);
    assert_eq!(outbound_settlement(u128::MAX - 1, 1, 2), None);

    for amount_out in 0u128..=8 {
        for service_fee in 0u128..=8 {
            for ledger_fee in 0u128..=8 {
                let result = outbound_settlement(amount_out, ledger_fee, service_fee);
                if ledger_fee <= service_fee {
                    let (escrow_debit, reserve_credit, liability_debit) =
                        result.expect("small valid settlement");
                    assert_eq!(escrow_debit, amount_out + ledger_fee);
                    assert_eq!(reserve_credit, service_fee - ledger_fee);
                    assert_eq!(liability_debit, amount_out + service_fee);
                    assert_eq!(escrow_debit + reserve_credit, liability_debit);
                } else {
                    assert_eq!(result, None);
                }
            }
        }
    }
}

#[test]
fn compact_phase_kernels_match_the_legal_transition_graphs() {
    let deposit_edges = [
        (0, 0, 1),
        (0, 1, 5),
        (0, 2, 9),
        (1, 3, 2),
        (1, 4, 6),
        (2, 5, 3),
        (2, 6, 4),
        (3, 6, 4),
        (3, 7, 10),
        (4, 7, 10),
        (4, 4, 6),
        (6, 9, 8),
        (6, 10, 7),
    ];
    for state in 0..=10 {
        for event in 0..=10 {
            let expected = deposit_edges
                .iter()
                .find(|(from, input, _)| *from == state && *input == event)
                .map(|(_, _, next)| *next);
            assert_eq!(deposit_transition(state, event), expected);
        }
    }
    for state in 0..=10 {
        assert_eq!(deposit_reservation_active(state), matches!(state, 2..=4));
    }
    assert!(authorization_commit_allowed(
        true, true, true, true, true, true
    ));
    assert!(!authorization_commit_allowed(
        true, true, true, true, true, false
    ));
    assert!(expiry_refund_allowed(true, false, 101, 100));
    assert!(!expiry_refund_allowed(true, true, 101, 100));
    assert!(!expiry_refund_allowed(true, false, 100, 100));
    assert!(mint_finalization_allowed(true, true, 7, 8));
    assert!(!mint_finalization_allowed(true, false, 7, 8));

    let withdrawal_edges = [
        (0, 0, 1),
        (1, 1, 1),
        (1, 2, 2),
        (1, 3, 3),
        (3, 4, 2),
        (3, 5, 1),
    ];
    for state in 0..=3 {
        for event in 0..=5 {
            let expected = withdrawal_edges
                .iter()
                .find(|(from, input, _)| *from == state && *input == event)
                .map_or(state, |(_, _, next)| *next);
            assert_eq!(
                withdrawal_phase_step(state, event),
                expected,
                "withdrawal state {state}, event {event}"
            );
            assert_eq!(
                withdrawal_phase_allows(state, event),
                expected != state || (state == 1 && event == 1)
            );
        }
    }
}

#[test]
fn deposit_transition_decision_effects_cover_every_state_event_and_idempotency() {
    fn valid_guard(event: u8) -> DepositEventGuard {
        match event {
            0..=2 => DepositEventGuard::Funding,
            3 => DepositEventGuard::CommitAuthorization {
                quote_valid: true,
                fixed_fields_match: true,
                canonical_domain_strings: true,
                deadline_valid: true,
                pristine: true,
            },
            4 => DepositEventGuard::StartRefund {
                attempt_matches: true,
                policy_matches: true,
            },
            5 => DepositEventGuard::InstallSignature {
                dispatched: true,
                signature_absent: true,
                signature_length_valid: true,
            },
            6 => DepositEventGuard::BeginExpiry,
            7 => DepositEventGuard::MintFinalization {
                fixed_fields_match: true,
                receipt_succeeded: true,
                receipt_block: 0,
                finalized_block: 0,
                audit_complete: true,
            },
            _ => DepositEventGuard::RefundResult,
        }
    }

    for state in 0..=10 {
        for event in 0..=10 {
            assert_eq!(
                deposit_transition_decision(DepositTransitionInput {
                    state,
                    event,
                    guard: valid_guard(event),
                    same_payload: true,
                    gross_amount: 11,
                    net_amount: 10,
                    service_fee: 1,
                    reserved_amount: 10,
                }),
                DepositTransitionDecision::Idempotent
            );
            match (
                deposit_transition(state, event),
                deposit_transition_decision(DepositTransitionInput {
                    state,
                    event,
                    guard: valid_guard(event),
                    same_payload: false,
                    gross_amount: 11,
                    net_amount: 10,
                    service_fee: 1,
                    reserved_amount: 10,
                }),
            ) {
                (None, DepositTransitionDecision::Reject) => {}
                (Some(next_state), DepositTransitionDecision::Apply(effects)) => {
                    assert_eq!(effects.next_state, next_state);
                    assert_eq!(effects.reservation_active, matches!(next_state, 2..=4));
                    assert_eq!(
                        effects.release_reservation,
                        ((state == 3 || state == 4) && event == 7) || (state == 4 && event == 4)
                    );
                    assert_eq!(
                        effects.charge_service_fee,
                        (state == 3 || state == 4) && event == 7
                    );
                    assert_eq!(effects.fee_credit, u128::from(effects.charge_service_fee));
                    assert_eq!(
                        effects.reservation_add,
                        if !deposit_reservation_active(state)
                            && deposit_reservation_active(next_state)
                        {
                            10
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        effects.reservation_release,
                        if effects.release_reservation { 10 } else { 0 }
                    );
                    assert_eq!(
                        effects.pending_liability_debit,
                        if ((state == 3 || state == 4) && event == 7) || (state == 6 && event == 9)
                        {
                            11
                        } else {
                            0
                        }
                    );
                    assert_eq!(
                        effects.escrow_debit,
                        if state == 6 && event == 9 { 11 } else { 0 }
                    );
                    assert_eq!(
                        effects.mint_supply_increase,
                        if (state == 3 || state == 4) && event == 7 {
                            10
                        } else {
                            0
                        }
                    );
                }
                pair => panic!("transition decision mismatch: {pair:?}"),
            }
        }
    }
}
