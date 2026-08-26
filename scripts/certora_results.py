#!/usr/bin/env python3
"""Fetch and validate machine-readable results for one private Certora job."""

from __future__ import annotations

import argparse
import json
import re
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Iterator

from certora_fingerprint import validate_certora_fingerprint


RULE = re.compile(r"(?m)^\s*(?:rule|invariant)\s+([A-Za-z_][A-Za-z0-9_]*)\b")
SHA1 = re.compile(r"^[0-9a-fA-F]{40}$")
REVISION_KEYS = {
    "commitsha1",
    "commit_sha1",
    "githash",
    "git_hash",
    "provercommit",
    "prover_commit",
    "proverrevision",
    "prover_revision",
}
BAD_SANITY = re.compile(
    r"(?i)(?<![A-Za-z0-9])"
    r"(fail(?:ed|ure)?|warn(?:ing)?|vacuous|trivial|unknown|timeout)"
    r"(?![A-Za-z0-9])"
)
KNOWN_STATUSES = {"SUCCESS", "FAIL", "TIMEOUT", "UNKNOWN", "SANITY_FAIL"}


class CertoraResultError(ValueError):
    """Raised when cloud evidence is incomplete or not successful."""


def walk(value: Any) -> Iterator[Any]:
    yield value
    if isinstance(value, dict):
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def status_leaves(value: Any) -> list[str]:
    if isinstance(value, str):
        if value not in KNOWN_STATUSES:
            raise CertoraResultError(f"unexpected Certora terminal status: {value!r}")
        return [value]
    if isinstance(value, dict):
        leaves: list[str] = []
        for key, child in value.items():
            if key in KNOWN_STATUSES:
                if child:
                    leaves.append(key)
            else:
                leaves.extend(status_leaves(child))
        return leaves
    if isinstance(value, list):
        leaves: list[str] = []
        for child in value:
            leaves.extend(status_leaves(child))
        return leaves
    raise CertoraResultError(f"unexpected Certora rule result value: {value!r}")


def validate_rule_results(output: dict[str, Any], declared_rules: set[str]) -> dict[str, str]:
    rules = output.get("rules")
    if not isinstance(rules, dict) or not rules:
        raise CertoraResultError("Certora output has no rule results")
    actual_rules = set(rules)
    if actual_rules != declared_rules:
        missing = sorted(declared_rules - actual_rules)
        unexpected = sorted(actual_rules - declared_rules)
        raise CertoraResultError(
            f"Certora rule result set mismatch: missing={missing}, unexpected={unexpected}"
        )
    normalized: dict[str, str] = {}
    for rule, result in rules.items():
        leaves = status_leaves(result)
        if not leaves:
            raise CertoraResultError(f"Certora rule has no terminal status: {rule}")
        failures = sorted({status for status in leaves if status != "SUCCESS"})
        if failures:
            raise CertoraResultError(f"Certora rule did not pass: {rule}={failures}")
        normalized[rule] = "SUCCESS"
    return dict(sorted(normalized.items()))


def validate_sanity(treeview: Any) -> None:
    for node in walk(treeview):
        if not isinstance(node, dict):
            continue
        is_sanity = any(
            "sanity" in str(key).lower()
            or (isinstance(value, str) and "sanity" in value.lower())
            for key, value in node.items()
        )
        for key, value in node.items():
            status_field = key.lower() in {"severity", "status", "result", "outcome", "warning"}
            if ("sanity" in key.lower() or (is_sanity and status_field)) and (
                value is False or (isinstance(value, str) and BAD_SANITY.search(value))
            ):
                raise CertoraResultError(f"Certora sanity result is not clean: {key}={value!r}")


def prover_revision(job_data: Any) -> str:
    revisions: set[str] = set()
    for node in walk(job_data):
        if not isinstance(node, dict):
            continue
        for key, value in node.items():
            if key.lower() in REVISION_KEYS and isinstance(value, str) and SHA1.fullmatch(value):
                revisions.add(value.lower())
    if len(revisions) != 1:
        raise CertoraResultError(
            f"expected one exact Prover revision in job metadata, found {sorted(revisions)}"
        )
    return revisions.pop()


def validate_job_status(job_data: dict[str, Any]) -> None:
    status = job_data.get("jobStatus")
    if status != "SUCCEEDED":
        raise CertoraResultError(f"Certora job did not succeed: {status!r}")


def declared_rules(config_path: Path, root: Path) -> set[str]:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    verify = config.get("verify")
    if not isinstance(verify, str) or ":" not in verify:
        raise CertoraResultError("Certora config has no canonical verify entry")
    spec_path = root / verify.split(":", 1)[1]
    rules = set(RULE.findall(spec_path.read_text(encoding="utf-8")))
    if not rules:
        raise CertoraResultError("Certora spec declares no rules or invariants")
    return rules


def select_job(recent_jobs: Path, root: Path, started_at: int) -> dict[str, Any]:
    data = json.loads(recent_jobs.read_text(encoding="utf-8"))
    jobs = data.get(root.resolve().as_posix())
    if not isinstance(jobs, list):
        raise CertoraResultError("Certora recent-jobs file has no entry for this checkout")
    candidates = [job for job in jobs if isinstance(job, dict) and job.get("time", 0) >= started_at]
    if len(candidates) != 1:
        raise CertoraResultError(f"expected one new Certora job, found {len(candidates)}")
    required = {"anonymous_key", "domain", "job_id", "user_id"}
    if not required <= candidates[0].keys():
        raise CertoraResultError("Certora recent job is missing private result coordinates")
    return candidates[0]


def private_url(job: dict[str, Any], resource: str) -> str:
    domain = str(job["domain"]).rstrip("/")
    user = urllib.parse.quote(str(job["user_id"]), safe="")
    job_id = urllib.parse.quote(str(job["job_id"]), safe="")
    key = urllib.parse.quote(str(job["anonymous_key"]), safe="")
    return f"{domain}/{resource}/{user}/{job_id}?anonymousKey={key}"


def private_report_url(job: dict[str, Any]) -> str:
    return private_url(job, "output").split("?", 1)[0]


def fetch_json(url: str) -> Any:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.status != 200:
                raise CertoraResultError(f"Certora result endpoint returned HTTP {response.status}")
            return json.load(response)
    except CertoraResultError:
        raise
    except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
        raise CertoraResultError(
            f"Certora private result fetch failed: {type(error).__name__}"
        ) from None


def build_summary(
    *,
    target: str,
    config_path: Path,
    root: Path,
    recent_jobs: Path,
    started_at: int,
    duration_seconds: int,
    git_commit: str,
    fingerprint: dict[str, Any],
    cli_version: str,
    solc_version: str,
) -> dict[str, Any]:
    validate_certora_fingerprint(fingerprint)
    job = select_job(recent_jobs, root, started_at)
    output = fetch_json(private_url(job, "jsonOutput"))
    job_data = fetch_json(private_url(job, "jobData"))
    treeview = fetch_json(private_url(job, "progress"))
    if not isinstance(output, dict) or not isinstance(job_data, dict):
        raise CertoraResultError("Certora result endpoints returned an unexpected schema")
    validate_job_status(job_data)
    normalized = validate_rule_results(output, declared_rules(config_path, root))
    validate_sanity(treeview)
    revision = prover_revision(job_data)
    report = private_report_url(job)
    return {
        "schema": 2,
        "target": target,
        "mode": "cloud",
        "status": "pass",
        "evidence_status": "bootstrap",
        "git_commit": git_commit,
        "started_at_epoch": started_at,
        "duration_seconds": duration_seconds,
        "source_fingerprint": fingerprint,
        "fingerprint_scope": "certora-advisory-v1",
        "certora_cli": cli_version.strip(),
        "prover_revision": revision,
        "solc": solc_version.strip(),
        "rule_results": normalized,
        "private_report_url": report,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--recent-jobs", type=Path, required=True)
    parser.add_argument("--started-at", type=int, required=True)
    parser.add_argument("--duration-seconds", type=int, required=True)
    parser.add_argument("--git-commit", required=True)
    parser.add_argument("--fingerprint", type=Path, required=True)
    parser.add_argument("--cli-version", required=True)
    parser.add_argument("--solc-version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    summary = build_summary(
        target=args.target,
        config_path=args.config,
        root=args.root,
        recent_jobs=args.recent_jobs,
        started_at=args.started_at,
        duration_seconds=args.duration_seconds,
        git_commit=args.git_commit,
        fingerprint=json.loads(args.fingerprint.read_text(encoding="utf-8")),
        cli_version=args.cli_version,
        solc_version=args.solc_version,
    )
    args.output.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
