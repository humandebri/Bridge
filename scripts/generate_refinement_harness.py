#!/usr/bin/env python3
"""Generate fixed Rust, Foundry, and Vitest refinement consumers."""

from __future__ import annotations

import argparse
import json
import subprocess
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "verification" / "generated" / "refinement-harnesses.json"
RUST_OUTPUT = ROOT / "canister" / "bridge-core" / "tests" / "generated_refinement.rs"
FOUNDRY_OUTPUT = ROOT / "contracts" / "test" / "GeneratedRefinement.t.sol"
VITEST_OUTPUT = ROOT / "ui" / "src" / "lib" / "generated-refinement.test.ts"
RUST_TARGET = "canister/bridge-core/tests/generated_refinement.rs"
FOUNDRY_TARGET = "contracts/test/GeneratedRefinement.t.sol"
VITEST_TARGET = "ui/src/lib/generated-refinement.test.ts"


@dataclass(frozen=True)
class Renderer:
    target: str
    selector: str
    body: str


def rust_test(selector: str, section: str, body: str) -> str:
    return f'''#[test]
fn {selector}() {{
    for case in cases("{section}") {{
{body.rstrip()}
    }}
}}
'''


def foundry_test(selector: str, body: str, mutability: str = "") -> str:
    suffix = f" {mutability}" if mutability else ""
    return f'''    function {selector}() public{suffix} {{
{body.rstrip()}
    }}
'''


def vitest_test(selector: str, body: str) -> str:
    return f'''  it("{selector}", () => {{
{body.rstrip()}
  }})
'''


RUST_RENDERERS: dict[str, tuple[str, str]] = {
    "quote_cases": (
        "protocol_quote_cases_matches_production",
        '''        let amount_value = amount(text(&case, "amount"));
        let service_fee = amount(text(&case, "service_fee"));
        let actual = optional_text(&case, "amount_out")
            .map(amount)
            .is_some_and(|amount_out| committed_quote_matches(amount_value, amount_out, service_fee));
        assert_eq!(actual, boolean(&case, "accepted"));''',
    ),
    "settlement_cases": (
        "protocol_settlement_cases_matches_production",
        '''        let amount_out = amount(text(&case, "amount_out"));
        let ledger_fee = amount(text(&case, "ledger_fee"));
        let service_fee = amount(text(&case, "service_fee"));
        let arithmetic = outbound_settlement(amount_out, ledger_fee, service_fee);
        assert_eq!(arithmetic.unwrap_or((0, 0, 0)), (
            amount(text(&case, "escrow_debit")),
            amount(text(&case, "reserve_credit")),
            amount(text(&case, "liability_debit")),
        ));
        let before = (
            amount(text(&case, "before_escrow")),
            amount(text(&case, "before_base_supply")),
            amount(text(&case, "before_fee_reserve")),
            amount(text(&case, "before_unpaid_liability")),
        );
        assert_eq!(backed(before), boolean(&case, "before_backed"));
        let checked_after = arithmetic.and_then(|_| Some((
            before.0.checked_sub(amount_out.checked_add(ledger_fee)?)?,
            before.1,
            before.2.checked_add(service_fee)?.checked_sub(ledger_fee)?,
            before.3.checked_sub(amount_out.checked_add(service_fee)?)?,
        )));
        let accepted = boolean(&case, "before_backed") && checked_after.is_some();
        assert_eq!(accepted, boolean(&case, "accepted"));
        let after = checked_after.filter(|_| boolean(&case, "before_backed")).unwrap_or(before);
        assert_eq!(after, (
            amount(text(&case, "after_escrow")),
            amount(text(&case, "after_base_supply")),
            amount(text(&case, "after_fee_reserve")),
            amount(text(&case, "after_unpaid_liability")),
        ));
        assert_eq!(backed(after), boolean(&case, "after_backed"));''',
    ),
    "payment_cases": (
        "protocol_payment_cases_matches_production",
        '''        let accepted = !boolean(&case, "already_paid")
            && boolean(&case, "destination_matches")
            && amount(text(&case, "transfer_fee")) <= amount(text(&case, "charged_fee"))
            && release_transfer_matches(
                amount(text(&case, "transfer_amount")),
                amount(text(&case, "transfer_fee")),
                amount(text(&case, "amount_out")),
                amount(text(&case, "transfer_fee")),
            );
        assert_eq!(accepted, boolean(&case, "accepted"));''',
    ),
    "deposit_admission_cases": (
        "protocol_deposit_admission_cases_matches_production",
        '''        let maximum = amount(text(&case, "maximum_service_fee"));
        let snapshot = BaseMintSnapshot {
            finalized_head_block_number: 1,
            confirmed_block_timestamp: 0,
            service_fee: Amount::new(amount(text(&case, "service_fee"))),
            max_service_fee: Amount::new(maximum),
            per_deposit_limit: Amount::new(amount(text(&case, "per_deposit_limit"))),
            mint_window_limit: Amount::new(amount(text(&case, "mint_window_limit"))),
            mint_window_started_at: 0,
            mint_window_duration: u64::MAX,
            minted_in_window: Amount::new(amount(text(&case, "minted_in_window"))),
        };
        let actual = snapshot.quote(Amount::new(amount(text(&case, "gross"))), Amount::new(maximum))
            .ok().map(|value| value.get());
        assert_eq!(actual.is_some(), boolean(&case, "accepted"));
        assert_eq!(actual, optional_text(&case, "net").map(amount));''',
    ),
    "deposit_identity_cases": (
        "protocol_deposit_identity_cases_matches_production",
        '''        let actual = match deposit_identity_decision(boolean(&case, "processed")) {
            DepositIdentityDecision::Allow => "Allow",
            DepositIdentityDecision::Conflict => "Conflict",
        };
        assert_eq!(actual, text(&case, "decision"));''',
    ),
    "reservation_cases": (
        "protocol_reservation_cases_matches_production",
        '''        let before_reserved = amount(text(&case, "before_reserved"));
        let before_candidate = amount(text(&case, "before_candidate"));
        let after_reserved = amount(text(&case, "after_reserved"));
        let after_candidate = amount(text(&case, "after_candidate"));
        let exact_commit = before_reserved.checked_add(before_candidate)
            .is_some_and(|committed| committed == after_reserved && after_candidate == 0);
        assert_eq!(exact_commit && reserve_admission_preserves_requirement(
            before_reserved, before_candidate, after_reserved, after_candidate,
        ), boolean(&case, "accepted"));''',
    ),
    "service_fee_cases": (
        "protocol_service_fee_cases_matches_production",
        '''        assert_eq!(service_fee_change_allowed(
            amount(text(&case, "service_fee")), amount(text(&case, "maximum")),
        ), boolean(&case, "accepted"));''',
    ),
    "fee_rotation_cases": (
        "protocol_fee_rotation_cases_matches_production",
        '''        let accepted = fee_recipient_rotation_allowed(amount(text(&case, "pending")));
        assert_eq!(accepted, boolean(&case, "accepted"));
        if accepted {
            assert_eq!(text(&case, "before_reserve"), text(&case, "after_reserve"));
            assert_eq!(text(&case, "before_deposit_fees"), text(&case, "after_deposit_fees"));
            assert_eq!(text(&case, "before_withdrawal_fees"), text(&case, "after_withdrawal_fees"));
            assert_eq!(text(&case, "next_recipient"), text(&case, "after_recipient"));
        } else {
            assert_eq!(text(&case, "before_recipient"), text(&case, "after_recipient"));
        }''',
    ),
    "fee_payout_cases": (
        "protocol_fee_payout_cases_matches_production",
        '''        let reserve = amount(text(&case, "reserve"));
        let pending = amount(text(&case, "pending"));
        let payout_amount = amount(text(&case, "amount"));
        let fee = amount(text(&case, "fee"));
        assert_eq!(payout_allowed(reserve, pending, payout_amount, fee), boolean(&case, "allowed"));
        assert_eq!(payout_debit(true, payout_amount, fee), Some(amount(text(&case, "first_debit"))));
        assert_eq!(payout_debit(false, payout_amount, fee), Some(amount(text(&case, "replay_debit"))));''',
    ),
    "hold_cases": (
        "protocol_hold_cases_matches_production",
        '''        assert_eq!(hold_retry_allowed(
            boolean(&case, "success"), boolean(&case, "absence"),
        ), boolean(&case, "allowed"));''',
    ),
    "lease_cases": (
        "protocol_lease_cases_matches_production",
        '''        assert_eq!(lease_outcome_is_current(
            block(text(&case, "current")), block(text(&case, "outcome")), boolean(&case, "active"),
        ), boolean(&case, "accepted"));''',
    ),
    "manual_claim_cases": (
        "protocol_manual_claim_cases_matches_production",
        '''        assert_eq!(manual_claim_allowed(
            boolean(&case, "scheduled"), boolean(&case, "active"), boolean(&case, "stopped"),
            boolean(&case, "overdue"), boolean(&case, "expired"),
        ), boolean(&case, "allowed"));''',
    ),
    "refund_request_identity_cases": (
        "protocol_refund_request_identity_cases_matches_production",
        '''        let actual = refund_request_identity_decision(boolean(&case, "authenticated"));
        let expected = match text(&case, "decision") {
            "allow" => RefundRequestIdentityDecision::Allow,
            "anonymous-caller" => RefundRequestIdentityDecision::AnonymousCaller,
            value => panic!("unknown refund request identity decision: {value}"),
        };
        assert_eq!(actual, expected);''',
    ),
    "deposit_nonterminal_index_cases": (
        "protocol_deposit_nonterminal_index_cases_match_production",
        '''        assert_eq!(deposit_nonterminal_indexed(
            short(text(&case, "state")) as u8,
        ), boolean(&case, "indexed"));''',
    ),
    "notification_admission_cases": (
        "protocol_notification_admission_cases_matches_production",
        '''        assert_eq!(notification_admission_allowed(
            short(text(&case, "global_count")), short(text(&case, "caller_count")),
            short(text(&case, "global_limit")), short(text(&case, "caller_limit")),
        ), boolean(&case, "allowed"));
        assert_eq!(notification_ingestion_allowed(
            short(text(&case, "ingestion_count")), short(text(&case, "ingestion_limit")),
        ), boolean(&case, "ingestion_allowed"));
        assert_eq!(notification_failure_cooldown_active(
            boolean(&case, "hash_matches"), block(text(&case, "now_ns")),
            block(text(&case, "retry_after_ns")),
        ), boolean(&case, "cooldown_active"));''',
    ),
    "lease_lane_cases": (
        "protocol_lease_lane_cases_matches_production",
        '''        let actual = lease_lane_claim_decision(
            boolean(&case, "target_active"), boolean(&case, "target_automatic"),
            block(text(&case, "active_in_lane")), block(text(&case, "capacity")),
        );
        let expected = match text(&case, "decision") {
            "allow" => LeaseLaneClaimDecision::Allow,
            "automatic-progress-pending" => LeaseLaneClaimDecision::AutomaticProgressPending,
            "busy" => LeaseLaneClaimDecision::Busy,
            value => panic!("unknown lease lane decision: {value}"),
        };
        assert_eq!(actual, expected);''',
    ),
    "funding_attempt_cases": (
        "protocol_funding_attempt_cases_matches_production",
        '''        let actual = funding_attempt_decision(
            block(text(&case, "outcome_kind")).try_into().expect("funding outcome kind fits u8"),
        );
        let expected = match text(&case, "decision") {
            "promote-success" => FundingAttemptDecision::PromoteSuccess,
            "promote-ambiguous" => FundingAttemptDecision::PromoteAmbiguous,
            "release" => FundingAttemptDecision::Release,
            "retain" => FundingAttemptDecision::Retain,
            value => panic!("unknown funding attempt decision: {value}"),
        };
        assert_eq!(actual, expected);''',
    ),
    "funding_reconciliation_cases": (
        "protocol_funding_reconciliation_cases_matches_production",
        '''        let actual = funding_reconciliation_decision(
            boolean(&case, "complete_absence"), boolean(&case, "final_scan"),
            boolean(&case, "dedup_expired"),
        );
        let expected = match text(&case, "decision") {
            "wait" => FundingReconciliationDecision::Wait,
            "restart-fresh" => FundingReconciliationDecision::RestartFresh,
            "release" => FundingReconciliationDecision::Release,
            value => panic!("unknown funding reconciliation decision: {value}"),
        };
        assert_eq!(actual, expected);''',
    ),
    "canonical_probe_cases": (
        "protocol_canonical_probe_cases_matches_production",
        '''        assert_eq!(canonical_probe_matches(
            block(text(&case, "receipt_block")), block(text(&case, "snapshot_block")),
        ), boolean(&case, "accepted"));''',
    ),
    "ledger_block_provenance_cases": (
        "protocol_ledger_block_provenance_cases_match_production",
        '''        let funding = optional_text(&case, "funding").map(amount);
        let refund = optional_text(&case, "refund").map(amount);
        let release = optional_text(&case, "release").map(amount);
        let candidate = amount(text(&case, "block"));
        let accepted = boolean(&case, "accepted");
        match text(&case, "event") {
            "preserve" => {
                let deposit = deposit_ledger_block_transition(funding, refund, 0, candidate);
                assert_eq!(deposit.is_some(), accepted);
                assert_eq!(deposit.and_then(|blocks| blocks.0),
                    optional_text(&case, "next_funding").map(amount));
                assert_eq!(deposit.and_then(|blocks| blocks.1),
                    optional_text(&case, "next_refund").map(amount));
                let withdrawal = withdrawal_ledger_block_transition(release, 0, candidate);
                assert_eq!(withdrawal.is_some(), accepted);
                assert_eq!(withdrawal.flatten(),
                    optional_text(&case, "next_release").map(amount));
            }
            "funding" | "refund" => {
                let event = if text(&case, "event") == "funding" { 1 } else { 2 };
                let deposit = deposit_ledger_block_transition(funding, refund, event, candidate);
                assert_eq!(deposit.is_some(), accepted);
                assert_eq!(deposit.and_then(|blocks| blocks.0),
                    optional_text(&case, "next_funding").map(amount));
                assert_eq!(deposit.and_then(|blocks| blocks.1),
                    optional_text(&case, "next_refund").map(amount));
            }
            "release" => {
                let withdrawal = withdrawal_ledger_block_transition(release, 1, candidate);
                assert_eq!(withdrawal.is_some(), accepted);
                assert_eq!(withdrawal.flatten(),
                    optional_text(&case, "next_release").map(amount));
            }
            event => panic!("unknown ledger block event: {event}"),
        }''',
    ),
}


FOUNDRY_RENDERERS = {
    "quote_cases": (
        "test_protocol_quote_cases_matches_production",
        '''        string memory json = vm.readFile(VECTORS);
        uint256 count = vm.parseJsonUint(json, ".quote_count");
        assert(count > 0);
        for (uint256 index = 0; index < count; ++index) {
            string memory base = string.concat(".quote_cases[", vm.toString(index), "]");
            uint256 amount = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".amount")));
            uint256 serviceFee = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".service_fee")));
            bool accepted = vm.parseJsonBool(json, string.concat(base, ".accepted"));
            (Bridge bridge, IBSNS token) = _deployBridge(serviceFee);
            vm.prank(USER);
            token.approve(address(bridge), amount);
            if (!accepted) {
                vm.prank(USER);
                vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, amount));
                bridge.createWithdrawal(amount, serviceFee, hex"01", bytes32(0));
                continue;
            }
            uint256 expectedAmountOut = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".amount_out")));
            vm.prank(USER);
            uint256 withdrawalId = bridge.createWithdrawal(amount, serviceFee, hex"01", bytes32(0));
            IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
            assert(withdrawal.amount == amount);
            assert(withdrawal.chargedServiceFee == serviceFee);
            assert(withdrawal.amountOut == expectedAmountOut);
            assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
        }''',
        "",
    ),
    "deposit_admission_cases": (
        "test_mint_transition_matches_lean_deposit_admission_cases",
        '''        string memory json = vm.readFile(VECTORS);
        uint256 count = vm.parseJsonUint(json, ".deposit_admission_count");
        assert(count > 0);
        for (uint256 index = 0; index < count; ++index) {
            string memory base = string.concat(".deposit_admission_cases[", vm.toString(index), "]");
            MintAuthorizationPolicy.MintTransitionInput memory input = MintAuthorizationPolicy.MintTransitionInput({
                timestamp: 0,
                deadline: 0,
                authorizationEpoch: 1,
                currentEpoch: 1,
                recipient: USER,
                bridge: address(2),
                token: address(3),
                grossAmount: _uint(json, base, ".gross"),
                maximumFee: _uint(json, base, ".maximum_service_fee"),
                chargedFee: _uint(json, base, ".service_fee"),
                protocolMaximumFee: _uint(json, base, ".maximum_service_fee"),
                perDepositLimit: _uint(json, base, ".per_deposit_limit"),
                consumedInWindow: _uint(json, base, ".minted_in_window"),
                windowLimit: _uint(json, base, ".mint_window_limit"),
                windowStartedAt: 0,
                windowDuration: 1,
                paused: false,
                processed: false
            });
            (MintAuthorizationPolicy.RejectReason reason, MintAuthorizationPolicy.MintEffects memory effects,) =
                MintAuthorizationPolicy.evaluateMint(input);
            bool accepted = vm.parseJsonBool(json, string.concat(base, ".accepted"));
            assert((reason == MintAuthorizationPolicy.RejectReason.None) == accepted);
            if (accepted) {
                uint256 expectedNet = _uint(json, base, ".net");
                assert(effects.mintAmount == expectedNet);
                assert(effects.supplyIncrease == expectedNet);
                assert(effects.eventMintedAmount == expectedNet);
            }
        }''',
        "view",
    ),
}


VITEST_RENDERERS = {
    "finalization_cases": (
        "protocol_finalization_cases_matches_production",
        '''    for (const testCase of vectors.finalization_cases) {
      expect(decideWithdrawalFinalization(
        testCase.receipt_succeeded ? "success" : "reverted",
        BigInt(testCase.receipt_block),
        testCase.finalized_block === null ? null : BigInt(testCase.finalized_block),
      )).toBe(testCase.decision)
    }''',
    ),
    "queue_cases": (
        "protocol_queue_cases_matches_production",
        '''    for (const testCase of vectors.queue_cases) {
      const other = withdrawal("2", testCase.other_blocked, "other")
      const incoming = withdrawal("1", testCase.incoming_blocked, "incoming")
      const existing = testCase.existing_blocked === null
        ? []
        : [withdrawal("1", testCase.existing_blocked, "existing")]
      const restored = upsertPendingConfirmation([...existing, other], incoming, true)
      const target = restored.find((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(incoming))
      const preservedOther = restored.find((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(other))
      expect(target?.blocked).toBe(testCase.expected_blocked)
      expect(preservedOther?.blocked).toBe(testCase.expected_other_blocked)
    }''',
    ),
}


RENDERERS: dict[tuple[str, str], Renderer] = {}
for section, (selector, body) in RUST_RENDERERS.items():
    RENDERERS[(section, "rust")] = Renderer(RUST_TARGET, selector, rust_test(selector, section, body))
for section, (selector, body, mutability) in FOUNDRY_RENDERERS.items():
    RENDERERS[(section, "foundry")] = Renderer(FOUNDRY_TARGET, selector, foundry_test(selector, body, mutability))
for section, (selector, body) in VITEST_RENDERERS.items():
    RENDERERS[(section, "vitest")] = Renderer(VITEST_TARGET, selector, vitest_test(selector, body))


RUST_PRELUDE = '''// @generated by scripts/generate_refinement_harness.py
use bridge_core::{
    canonical_probe_matches, committed_quote_matches, deposit_identity_decision,
    deposit_nonterminal_indexed,
    deposit_ledger_block_transition,
    fee_recipient_rotation_allowed, funding_attempt_decision, funding_reconciliation_decision,
    hold_retry_allowed, lease_lane_claim_decision, lease_outcome_is_current, manual_claim_allowed,
    notification_admission_allowed, notification_failure_cooldown_active,
    notification_ingestion_allowed, outbound_settlement,
    payout_allowed, payout_debit, refund_request_identity_decision, release_transfer_matches,
    reserve_admission_preserves_requirement, service_fee_change_allowed, Amount,
    BaseMintSnapshot, DepositIdentityDecision,
    FundingAttemptDecision, FundingReconciliationDecision, LeaseLaneClaimDecision,
    withdrawal_ledger_block_transition, RefundRequestIdentityDecision,
};
use serde_json::Value;

fn document() -> Value {
    let document: Value = serde_json::from_str(include_str!(
        "../../../verification/generated/protocol-vectors.json"
    )).expect("Lean protocol vectors must be valid JSON");
    assert_eq!(document["schema_version"].as_u64(), Some(3));
    document
}

fn cases(section: &str) -> Vec<Value> {
    document()[section].as_array().expect("registered vector section must be an array").clone()
}

fn text<'a>(case: &'a Value, field: &str) -> &'a str {
    case[field].as_str().expect("vector field must be a string")
}

fn optional_text<'a>(case: &'a Value, field: &str) -> Option<&'a str> {
    case.get(field).and_then(Value::as_str)
}

fn boolean(case: &Value, field: &str) -> bool {
    case[field].as_bool().expect("vector field must be a boolean")
}

fn amount(value: &str) -> u128 {
    value.parse().expect("vector amount must be canonical u128")
}

fn block(value: &str) -> u64 {
    value.parse().expect("vector block must be canonical u64")
}

fn short(value: &str) -> u16 {
    value.parse().expect("vector counter must be canonical u16")
}

fn backed(state: (u128, u128, u128, u128)) -> bool {
    state.1.checked_add(state.2).and_then(|value| value.checked_add(state.3)) == Some(state.0)
}

'''


FOUNDRY_PRELUDE = '''// @generated by scripts/generate_refinement_harness.py
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {MintAuthorizationPolicy} from "../src/libraries/MintAuthorizationPolicy.sol";
import {TestBase} from "./TestBase.sol";

contract GeneratedRefinementTest is TestBase {
    string private constant VECTORS = "../verification/generated/protocol-vectors.json";
    uint256 private constant BRIDGE_SIGNER_KEY = 0xA11CE;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant USER = address(0x44);

    function _deployBridge(uint256 serviceFee) private returns (Bridge bridge, IBSNS token) {
        address bridgeSigner = vm.addr(BRIDGE_SIGNER_KEY);
        address timelock = _deployTestTimelock(address(0x33));
        bridge = new Bridge(
            bridgeSigner, RUNTIME_ADMINISTRATOR, timelock,
            _timelockCodeHash(timelock), 2_000, 2_000, 1 hours, 100, serviceFee
        );
        token = bridge.bsns();
        vm.prank(timelock);
        bridge.unpauseDepositMints();
        vm.prank(timelock);
        bridge.unpauseWithdrawals();
        IBridge.MintAuthorization memory authorization = IBridge.MintAuthorization({
            depositId: keccak256(abi.encode(serviceFee)),
            recipient: USER,
            grossAmount: 1_100,
            maxServiceFee: serviceFee,
            chargedServiceFee: serviceFee,
            deadline: block.timestamp + 30 minutes,
            authorizationEpoch: bridge.mintAuthorizationEpoch()
        });
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization, address(this));
    }

    function _uint(string memory json, string memory base, string memory field) private pure returns (uint256) {
        return vm.parseUint(vm.parseJsonString(json, string.concat(base, field)));
    }

'''


VITEST_PRELUDE = '''// @generated by scripts/generate_refinement_harness.py
import { describe, expect, it } from "vitest"
import vectors from "../../../verification/generated/protocol-vectors.json"
import { decideWithdrawalFinalization } from "./withdrawal-confirmation-state"
import { pendingConfirmationKey, upsertPendingConfirmation, type PendingConfirmation } from "./pending-confirmations"

const deployment = {
  bridgeCanisterId: "aaaaa-aa",
  chainId: 8453,
  bridgeAddress: "0x1111111111111111111111111111111111111111",
}

function withdrawal(byte: string, blocked: boolean, owner: string): PendingConfirmation {
  return {
    kind: "withdrawal",
    transactionHash: `0x${byte.repeat(64)}`,
    owner,
    blocked,
    notification: {
      status: "awaiting-notification",
      automaticAttemptUsed: false,
      shortRetryUsed: false,
      finalityReadvanceUsed: false,
    },
    ...deployment,
  }
}

describe("Generated Lean refinement consumers", () => {
'''


def registry() -> str:
    consumers = [
        {
            "section": section,
            "runner": runner,
            "target": renderer.target,
            "selector": renderer.selector,
        }
        for (section, runner), renderer in sorted(RENDERERS.items())
    ]
    return json.dumps({"schema": 3, "consumers": consumers}, indent=2) + "\n"


def format_source(command: list[str], source: str, language: str) -> str:
    result = subprocess.run(
        command,
        cwd=ROOT,
        input=source,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "unknown formatter error"
        raise RuntimeError(f"{language} formatter failed: {detail}")
    return result.stdout


def generated_sources(root: Path = ROOT) -> dict[Path, str]:
    rust_source = RUST_PRELUDE + "\n".join(
        renderer.body for key, renderer in sorted(RENDERERS.items()) if key[1] == "rust"
    )
    foundry_source = FOUNDRY_PRELUDE + "\n".join(
        renderer.body for key, renderer in sorted(RENDERERS.items()) if key[1] == "foundry"
    ) + "}\n"
    vitest = VITEST_PRELUDE + "\n".join(
        renderer.body for key, renderer in sorted(RENDERERS.items()) if key[1] == "vitest"
    ) + "})\n"
    rust = format_source(
        ["rustfmt", "--emit", "stdout", "--edition", "2021"], rust_source, "Rust"
    )
    foundry = format_source(
        ["forge", "fmt", "--root", str(ROOT / "contracts"), "--raw", "-"],
        foundry_source,
        "Solidity",
    )
    return {
        root / RUST_OUTPUT.relative_to(ROOT): rust,
        root / FOUNDRY_OUTPUT.relative_to(ROOT): foundry,
        root / VITEST_OUTPUT.relative_to(ROOT): vitest,
    }


def expected_outputs(root: Path = ROOT) -> dict[Path, str]:
    return {root / OUTPUT.relative_to(ROOT): registry(), **generated_sources(root)}


def stale_outputs(root: Path = ROOT) -> list[Path]:
    return [
        path
        for path, expected in expected_outputs(root).items()
        if not path.is_file() or path.read_text(encoding="utf-8") != expected
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        stale = [path.relative_to(ROOT).as_posix() for path in stale_outputs()]
        if stale:
            raise SystemExit(f"generated refinement harness is stale: {', '.join(stale)}")
    else:
        for path, expected in expected_outputs().items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
