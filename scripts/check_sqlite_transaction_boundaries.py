#!/usr/bin/env python3
"""Reject async or inter-canister work inside SQLite update closures."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "canister/bridge-canister/src/storage.rs"
CALLER_SOURCES = (
    ROOT / "canister/bridge-canister/src/api.rs",
    ROOT / "canister/bridge-canister/src/tasks.rs",
    ROOT / "canister/bridge-canister/src/admin.rs",
)
FORBIDDEN_EVM_SEQUENCE_WRITES = (
    ".put_evm_call_intent(",
    ".allocate_evm_operation_id()",
    ".allocate_hold_id()",
    ".put_open_reconciliation_hold(",
)
START = ".update(|connection|"
FORBIDDEN = (".await", "call_perform", "ic_cdk::call", "call::bounded_wait")


def closure(source: str, start: int) -> str:
    brace = source.find("{", start)
    if brace < 0:
        raise ValueError("SQLite update closure has no body")
    depth = 0
    for index in range(brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[brace : index + 1]
    raise ValueError("SQLite update closure is not balanced")


def main() -> int:
    source = SOURCE.read_text(encoding="utf-8")
    offset = 0
    while (start := source.find(START, offset)) >= 0:
        body = closure(source, start)
        for token in FORBIDDEN:
            if token in body:
                line = source.count("\n", 0, start) + 1
                print(f"{SOURCE}:{line}: forbidden {token!r} in SQLite transaction", file=sys.stderr)
                return 1
        offset = start + len(START)
    for caller_source in CALLER_SOURCES:
        caller = caller_source.read_text(encoding="utf-8")
        for token in FORBIDDEN_EVM_SEQUENCE_WRITES:
            if token in caller:
                line = caller.count("\n", 0, caller.index(token)) + 1
                print(
                    f"{caller_source}:{line}: EVM operation creation must use an atomic candidate bundle ({token})",
                    file=sys.stderr,
                )
                return 1
    tasks = CALLER_SOURCES[1].read_text(encoding="utf-8")
    for function_name in ("confirm_evm_member", "mark_evm_reverted"):
        start = tasks.index(f"fn {function_name}(")
        body = closure(tasks, start)
        if "commit_evm_terminal_bundle(" not in body:
            print(
                f"{CALLER_SOURCES[1]}: {function_name} must use commit_evm_terminal_bundle",
                file=sys.stderr,
            )
            return 1
        for token in (".put_evm_operation(", ".put_deposit(", ".put_withdrawal(",
                      ".set_accounting(", ".set_admin_state(", ".append_audit_event(",
                      ".set_external_progress("):
            if token in body:
                print(
                    f"{CALLER_SOURCES[1]}: forbidden sequential terminal write {token} in {function_name}",
                    file=sys.stderr,
                )
                return 1
    for required in ("commit_deposit_hold_bundle(", "commit_withdrawal_hold_bundle("):
        if required not in tasks:
            print(f"{CALLER_SOURCES[1]}: ambiguous Ledger holds must use {required}", file=sys.stderr)
            return 1
    for function_name in ("resolve_reconciliation_success", "resolve_reconciliation_absence"):
        start = tasks.index(f"fn {function_name}(")
        body = closure(tasks, start)
        if "remove_reconciliation_scan(" in body:
            print(
                f"{CALLER_SOURCES[1]}: {function_name} must remove scans inside its atomic transition bundle",
                file=sys.stderr,
            )
            return 1
        if "Some(&scan_target)" not in body:
            print(
                f"{CALLER_SOURCES[1]}: {function_name} must pass the scan into the hold bundle",
                file=sys.stderr,
            )
            return 1
    for forbidden in (".put_fee_payout(", ".allocate_fee_payout_id("):
        if forbidden in tasks or forbidden in CALLER_SOURCES[2].read_text(encoding="utf-8"):
            print(f"fee payout caller uses forbidden sequential write {forbidden}", file=sys.stderr)
            return 1
    for required in (
        "commit_fee_payout_request(",
        "hold_fee_payout(",
        "complete_fee_payout_success_and_scan(",
        "complete_fee_payout_failure_and_scan(",
        "commit_fee_payout_scan(",
    ):
        if required not in tasks and required not in CALLER_SOURCES[2].read_text(encoding="utf-8"):
            print(f"fee payout flow must use {required}", file=sys.stderr)
            return 1
    fee_payout_start = tasks.index("pub(crate) async fn advance_fee_payout(")
    fee_payout_body = closure(tasks, fee_payout_start)
    if ".put_reconciliation_scan(" in fee_payout_body:
        print("fee payout progress must use a state-bound CAS scan update", file=sys.stderr)
        return 1
    if ".update_fee_payout_scan(" not in fee_payout_body:
        print("fee payout progress must use update_fee_payout_scan", file=sys.stderr)
        return 1
    storage = SOURCE.read_text(encoding="utf-8")
    for function_name in ("put_deposit", "put_withdrawal", "put_reconciliation_hold"):
        start = storage.index(f"fn {function_name}(")
        body = closure(storage, start)
        if "self.handle.update(|connection|" not in body:
            print(
                f"{SOURCE}: {function_name} must commit record, indexes, and counters in one SQLite transaction",
                file=sys.stderr,
            )
            return 1
        for token in (
            "self.deposits.insert(",
            "self.withdrawals.insert(",
            "self.reconciliation_holds.insert(",
            "self.pull_pending_deposit_index.insert(",
            "self.release_pending_withdrawal_index.insert(",
            "self.open_hold_index.insert(",
            "self.operation_owner_index.insert(",
            "self.counters.set(",
        ):
            if token in body:
                print(
                    f"{SOURCE}: {function_name} uses forbidden sequential write {token}",
                    file=sys.stderr,
                )
                return 1
    for function_name in ("persist_resolved_deposit_and_hold", "persist_resolved_withdrawal_and_hold"):
        start = storage.index(f"fn {function_name}(")
        body = closure(storage, start)
        if "persist_resolved_hold_bundle(" not in body:
            print(f"{SOURCE}: {function_name} must use the atomic resolved-hold bundle", file=sys.stderr)
            return 1
    for required in (
        "commit_deposit_mint_bundle_and_scan",
        "commit_acknowledgement_bundle_and_scan",
        "resolve_deposit_hold_and_scan",
        "resolve_withdrawal_hold_and_scan",
    ):
        if required not in storage:
            print(f"{SOURCE}: missing scan-aware atomic bundle {required}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
