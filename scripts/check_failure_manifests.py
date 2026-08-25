#!/usr/bin/env python3
"""Validate deliberate proof failures are registered one-to-one with claims."""

from __future__ import annotations

import re
import hashlib
from pathlib import Path

from smt_obligations import parse_smt_obligations
from halmos_obligations import parse_halmos_obligations
from check_claim_manifest import checked_link

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_LEAN_FAILURE_SHA256 = {
    "AccountingDeltaViolation.lean": "42c3cd975297018b3f37963910957eae30d8a5910b18aba483ee7e24a27151dd",
    "AnonymousRefundRequest.lean": "beae263600cfc9307feadae225b64983f34e11bfdfa04adae6316ec7a1869552",
    "AuthorizationReissue.lean": "19df2aa3867a25d5bc470ff8b064d4f7c5d943121c572b84c7f4f0b9b0fff6be",
    "BackingViolation.lean": "0ad4d6a1424518a8b8f9968310d0a88619133f86d4ce877fa7134a0f22b206d2",
    "ConflictingFundingReplay.lean": "fd5f6abf7f8812e64bf2cbf457991ca46fcea6cdcc71acd074a6b162317c7b96",
    "DeadlineOverflow.lean": "44a1f3f1e9c9546b3b35cc8ad5e76e652f6425d7aedaf5b0647f303c80ae9ec7",
    "DestinationMutation.lean": "a3992470b4340e01eb81291efe4172fb9b3269141e987beea8238f02fded0cae",
    "DoubleDepositFeeTrace.lean": "daa0327e85847d2435b85c11f9bfe1031a2f19e0a53249d66e24effa7fe43434",
    "DoubleFee.lean": "076049e9a5e13cd6c1e4ebab231ed452dffc9fcac994b7aa15bdd8645e304851",
    "EvidencelessMint.lean": "7d248eb1e8b57cfd2e9f2c3d3892e03b495934fbc920278146a056cca36b2c96",
    "IncompleteAbsence.lean": "22ed9af027b854b148ed57fd103aa4cf9707a4366943ce43b4276e3ca4cbad62",
    "IncompleteExpiryAudit.lean": "b4010319f19cbc7192adc8f92a6e88278592a24cf2d023de93b037c27f72174b",
    "IncompleteMintAudit.lean": "db49a793aa755310d12569bc9066ba997ed30bd0fc92e53615ff5fb79394bda1",
    "InvalidExecutionStep.lean": "a6b48a8e639af5578312a150535c1b778d45877f353b449eca9559558c156213",
    "ManualActiveLeaseBypass.lean": "05b63de3fa0532e200bdd0c2f8e75495f96b40683b11177827e16335ee5fa7a7",
    "ManualClaimEconomicMutation.lean": "9b60fdf900a577eea35aa64585347ed44b629cceae79b2dbbc0ede270ad7722d",
    "MintFeeAlreadyCounted.lean": "723521e3697cbe526ea02cbfab85c8765729b62b241f862d964f81009394833e",
    "MissingUserLiveness.lean": "285ee5520011c8603f7db684c1a6365bef69da98d9e38c6ed60a4f0b69bbe07e",
    "ProcessedExpiryRefund.lean": "a2412ad467b65611d640d698c109cb6305463b31d138c29fedab02940f927d18",
    "RefundBeforeFunding.lean": "47ac92b520aef0fc257bf1a3fa956af2ce6130b9bf676b56d8936e62c2bd56f0",
    "StaleCrossRecordCallback.lean": "22dc013ef440ee7341088666b13309141a4fd135931b167481741248a4720989",
    "StaleLeaseCallback.lean": "060d4a6c2845d57c76dc64427eca5be79f8444c0472b7191a2ffffe4f34c2dae",
    "TerminalAuthorizationReopen.lean": "c7ba272a6f81dfcd0c4333bd325bcea1f41c1ce618cd1a645cd3dddea5daa29f",
    "TerminalDepositIndexed.lean": "5a740f2449b4ebd506b9d3095378f2226f253b9fd4ec96442422a0adbcbd91d7",
    "UnauthorizedConfirmationCaller.lean": "ebb7016ca30fe6b8c28c6dc6533da01331be76d3cc479a886866213d52ce80d0",
    "UnfairLiveness.lean": "e346efb2083754bc8793d27a176243cc79c29f55a54eed0af4778e85faff6eca",
}


def rows(path: Path, width: int) -> list[list[str]]:
    parsed = [line.split("\t") for line in path.read_text(encoding="utf-8").splitlines() if line]
    if any(len(row) != width or not all(row) for row in parsed):
        raise ValueError(f"invalid failure manifest row: {path}")
    return parsed


def relative_fixture_paths(directory: Path, suffix: str) -> list[str]:
    return sorted(
        path.relative_to(directory).as_posix()
        for path in directory.rglob(f"*{suffix}")
        if path.is_file()
    )


def main() -> int:
    smt_dir = ROOT / "verification" / "smt" / "fail"
    smt_rows = rows(ROOT / "verification" / "smt" / "failure-manifest.tsv", 2)
    smt_fixtures = [row[1] for row in smt_rows]
    actual_smt = relative_fixture_paths(smt_dir, ".sol")
    if len(set(smt_fixtures)) != len(smt_fixtures) or sorted(smt_fixtures) != actual_smt:
        raise ValueError("SMT failure manifest does not exactly cover deliberate fixtures")
    smt_obligations = parse_smt_obligations(
        (ROOT / "verification" / "smt" / "obligations.tsv").read_text(encoding="utf-8")
    )
    registered_failure_ids = {
        failure_id
        for obligation in smt_obligations.values()
        for failure_id in obligation.failure_ids
    }
    actual_failure_ids = {row[0] for row in smt_rows}
    if registered_failure_ids != actual_failure_ids:
        raise ValueError(
            "SMT obligations do not exactly cover negative IDs: "
            f"missing={sorted(actual_failure_ids - registered_failure_ids)} "
            f"extra={sorted(registered_failure_ids - actual_failure_ids)}"
        )

    halmos_dir = ROOT / "contracts" / "test" / "halmos" / "fail"
    halmos_rows = rows(ROOT / "verification" / "halmos" / "failure-manifest.tsv", 2)
    halmos_fixture_paths: list[str] = []
    for failure_id, link in halmos_rows:
        path, _ = checked_link(link)
        if halmos_dir.resolve() not in path.parents:
            raise ValueError(f"Halmos failure fixture is outside the fail directory: {link}")
        halmos_fixture_paths.append(path.relative_to(halmos_dir).as_posix())
    actual_halmos = sorted(
        path.relative_to(halmos_dir).as_posix()
        for path in halmos_dir.rglob("*.sol")
        if path.is_file()
    )
    if (
        len({row[0] for row in halmos_rows}) != len(halmos_rows)
        or len(set(halmos_fixture_paths)) != len(halmos_fixture_paths)
        or sorted(halmos_fixture_paths) != actual_halmos
    ):
        raise ValueError("Halmos failure manifest does not exactly cover deliberate fixtures")
    halmos_obligations = parse_halmos_obligations(
        (ROOT / "verification" / "halmos" / "obligations.tsv").read_text(encoding="utf-8")
    )
    registered_halmos_failure_ids = {
        failure_id
        for obligation in halmos_obligations.values()
        for failure_id in obligation.failure_ids
    }
    actual_halmos_failure_ids = {row[0] for row in halmos_rows}
    if registered_halmos_failure_ids != actual_halmos_failure_ids:
        raise ValueError(
            "Halmos obligations do not exactly cover negative IDs: "
            f"missing={sorted(actual_halmos_failure_ids - registered_halmos_failure_ids)} "
            f"extra={sorted(registered_halmos_failure_ids - actual_halmos_failure_ids)}"
        )

    lean_dir = ROOT / "verification" / "lean" / "fail"
    lean_source = "\n".join(
        (ROOT / "verification" / "lean" / "BridgeSpec" / name).read_text(encoding="utf-8")
        for name in (
            "DepositAuthorization.lean",
            "ClaimContracts.lean",
            "LedgerBlockProvenance.lean",
            "Protocol.lean",
        )
    )
    lean_rows = rows(ROOT / "verification" / "lean" / "deposit-failure-manifest.tsv", 3)
    fixture_names = set(relative_fixture_paths(lean_dir, ".lean"))
    manifest_names = {fixture for _, fixture, _ in lean_rows}
    required_names = set(REQUIRED_LEAN_FAILURE_SHA256)
    if fixture_names != required_names or manifest_names != required_names:
        raise ValueError(
            "Lean failure policy coverage differs: "
            f"files={sorted(fixture_names)} manifest={sorted(manifest_names)} "
            f"required={sorted(required_names)}"
        )
    seen_pairs: set[tuple[str, str]] = set()
    for theorem, fixture, missing_premise in lean_rows:
        pair = theorem, missing_premise
        if pair in seen_pairs or not re.fullmatch(r"[a-z0-9_]+", missing_premise):
            raise ValueError(f"duplicate or invalid Lean failure mapping: {pair}")
        seen_pairs.add(pair)
        if re.search(rf"^theorem {re.escape(theorem)}\b", lean_source, re.MULTILINE) is None:
            raise ValueError(f"unknown Lean theorem in failure manifest: {theorem}")
        if not (lean_dir / fixture).is_file():
            raise ValueError(f"missing Lean failure fixture: {fixture}")
        digest = hashlib.sha256((lean_dir / fixture).read_bytes()).hexdigest()
        if digest != REQUIRED_LEAN_FAILURE_SHA256[fixture]:
            raise ValueError(f"Lean failure fixture changed outside trusted policy: {fixture}")
    print("failure fixture manifests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
