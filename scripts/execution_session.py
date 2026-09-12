#!/usr/bin/env python3
"""Own test processes and share their results only within one gate invocation."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import uuid

from proof_fingerprint import source_fingerprint

ROOT = Path(__file__).resolve().parents[1]
SESSION_ENV = "BRIDGE_TEST_SESSION"
MAX_REQUEST = 1024 * 1024
JEST_ARTIFACTS = (
    "target/test-deployment/staging/bridge_canister.wasm",
    "target/test-deployment/predecessor-v35/bridge_canister.wasm",
    "target/wasm32-unknown-unknown/release/mock_external.wasm",
)

def environment_digest() -> str:
    names = ("CARGO_", "RUST", "FOUNDRY_", "NODE_", "PNPM_", "VITE_", "BRIDGE_", "PLAYWRIGHT_")
    values = {key: value for key, value in os.environ.items()
              if key != SESSION_ENV and (key.startswith(names) or key in {"PATH", "CI", "HOME", "TMPDIR"})}
    return hashlib.sha256(json.dumps(values, sort_keys=True).encode()).hexdigest()



def request(payload: dict) -> dict:
    endpoint = os.environ.get(SESSION_ENV)
    if not endpoint:
        raise ValueError("no active test execution owner")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(endpoint)
        stream = connection.makefile("rwb")
        stream.write(json.dumps(payload).encode() + b"\n")
        stream.flush()
        result = json.loads(stream.readline())
    if "error" in result:
        raise ValueError(result["error"])
    return result


def active() -> bool:
    return bool(os.environ.get(SESSION_ENV))


def assert_selected(results: list[dict], runner: str, target: str, selectors: list[str]) -> None:
    if len(set(selectors)) != len(selectors):
        raise ValueError("duplicate requested test selector")
    for selector in selectors:
        matches = []
        for result in results:
            name = result["name"]
            if runner.startswith("rust-canister"):
                parts = list(Path(target).relative_to("canister/bridge-canister/src").with_suffix("").parts)
                if parts[-1] in {"mod", "lib"}:
                    parts.pop()
                prefix = "::".join(parts)
                match = name.endswith("::" + selector) and (not prefix or name.startswith(prefix + "::"))
            elif runner == "rust-profile":
                match = name.endswith("::" + selector)
            elif runner.startswith("rust"):
                match = name == selector
            else:
                match = result["target"] == target and name == selector
            if match:
                matches.append(result)
        if len(matches) != 1 or matches[0]["status"] != "passed":
            raise ValueError(f"test did not pass exactly once: {target}#{selector}")


def execute(runner: str, target: str, selectors: list[str]) -> None:
    response = request({"action": "execute", "runner": runner, "target": target, "selectors": selectors, "environment": environment_digest()})
    assert_selected(response["results"], runner, target, selectors)


class Session:
    def __init__(self, root: Path, mode: str, temporary: Path):
        self.root, self.mode, self.temporary = root.resolve(), mode, temporary
        self.run_id = uuid.uuid4().hex
        self.baseline = source_fingerprint(self.root)
        self.results: dict[str, dict] = {}
        self.lock = threading.Lock()
        self.failed = False
        self.current_process: subprocess.Popen | None = None
        self.started_at = time.time()
        self.environment = environment_digest()
        self.context: dict | None = None
        self.artifacts: dict[str, str] = {}

    def execution_context(self) -> dict:
        if self.context is None:
            def read(command):
                return subprocess.run(command, cwd=self.root, capture_output=True, text=True, check=True, timeout=30).stdout.strip()
            self.context = {
                "head_sha": read(["git", "rev-parse", "HEAD"]),
                "trusted_base_sha": os.environ.get("BRIDGE_TRUSTED_BASE_SHA"),
                "environment_sha256": self.environment,
                "submodules": read(["git", "submodule", "status", "--recursive"]),
                "tools": {tool: read([tool, "--version"]) for tool in
                          ("python3", "rustc", "cargo", "node", "pnpm", "forge", "anvil", "icp", "lean", "verus", "z3")},
            }
        return self.context


    def check_inputs(self):
        if self.failed or source_fingerprint(self.root) != self.baseline:
            self.failed = True
            raise ValueError("test execution inputs changed or the run already failed")

    def plan(self, runner: str, target: str) -> tuple[str, list[str], Path]:
        root = self.root
        filters: list[str] = []
        candidate = root / target
        if target and (not candidate.is_file() or candidate.resolve() != candidate.absolute() or ".." in Path(target).parts):
            raise ValueError("test target must be an ordinary repository file")
        if runner in {"vitest", "jest", "foundry"}:
            prefix = {"vitest": "ui/", "jest": "integration/", "foundry": "contracts/test/"}[runner]
            if target and not target.startswith(prefix):
                raise ValueError("test target is outside the runner source root")
            scope = "" if self.mode == "all" else target
            if not scope and self.mode != "all":
                raise ValueError("whole-suite execution requires all mode")
            if runner == "vitest":
                command = [str(root / "ui/node_modules/.bin/vitest"), "run", "--reporter=json"]
                if scope:
                    command.append(scope.removeprefix("ui/"))
                return runner + ":" + scope, command, root / "ui"
            if runner == "jest":
                report_path = self.temporary / ("jest-" + hashlib.sha256(scope.encode()).hexdigest() + ".json")
                command = [str(root / "node_modules/.bin/jest"), "--config", "integration/jest.config.js", "--runInBand", "--json", "--outputFile", str(report_path)]
                if scope:
                    command += ["--runTestsByPath", scope]
                return runner + ":" + scope, command, root
            command = ["forge", "test", "--root", "contracts", "--json"]
            if scope:
                command += ["--match-path", scope.removeprefix("contracts/")]
            return runner + ":" + scope, command, root
        if runner in {"rust-core-lib", "rust-mock"}:
            package = "bridge-core" if runner == "rust-core-lib" else "mock-external"
            if target != f"canister/{package}/src/lib.rs":
                raise ValueError("invalid Rust library target")
            command = ["cargo", "test", "--locked", "-p", package, "--lib"]
        elif runner == "rust-core":
            if not target.startswith("canister/bridge-core/tests/") or not target.endswith('.rs'):
                raise ValueError("invalid Rust core test target")
            command = ["cargo", "test", "--locked", "-p", "bridge-core", "--test", Path(target).stem]
        elif runner in {"rust-canister", "rust-canister-test-deployment"}:
            if not target.startswith("canister/bridge-canister/src/"):
                raise ValueError("invalid Rust canister target")
            target = "canister/bridge-canister/src/lib.rs"
            command = ["cargo", "test", "--locked", "-p", "bridge-canister", "--lib"]
            if runner.endswith("test-deployment"):
                command += ["--features", "test-deployment"]
        elif runner == "rust-profile":
            if target != "tools/bridge-profile/src/main.rs":
                raise ValueError("invalid Rust profile target")
            command = ["cargo", "test", "--locked", "-p", "bridge-profile", "--bin", "bridge-profile"]
            if self.mode != "all":
                filters = sorted({selector for _, kind, path, selector in required_consumers()
                                  if kind == runner and path == target})
                if not filters:
                    raise ValueError("profile proof execution has no registered selectors")
        else:
            raise ValueError("unknown test runner")
        return runner + ":" + target, command + ["--", "--test-threads=1", "--format=pretty"] + filters, root

    def parse_results(self, runner: str, target: str, output: str) -> list[dict]:
        if runner.startswith("rust"):
            records = [{"target": target, "name": name, "status": "passed" if status == "ok" else status}
                       for name, status in re.findall(r"^test ([^\r\n]+) \.\.\. ([^\r\n]+)$", output, re.MULTILINE)]
            totals = re.findall(r"^running (\d+) tests?$", output, re.MULTILINE)
            if totals != [str(len(records))]:
                raise ValueError("Rust test result count differs from the runner total")
        else:
            from check_claim_test_manifest import unique_json_object
            report = json.loads(output, object_pairs_hook=unique_json_object)
            records = []
            if runner in {"vitest", "jest"}:
                if report.get("success") is not True or report.get("numFailedTests") != 0:
                    raise ValueError("JavaScript suite failed")
                for suite in report.get("testResults", []):
                    path = Path(suite["name"]).resolve().relative_to(self.root).as_posix()
                    for assertion in suite.get("assertionResults", []):
                        if assertion.get("fullName") != " ".join([*assertion["ancestorTitles"], assertion["title"]]):
                            raise ValueError("invalid JavaScript test identity")
                        records.append({"target": path, "name": assertion["title"], "status": assertion["status"]})
                if report.get("numTotalTests") != len(records):
                    raise ValueError("JavaScript test count differs from the report")
            else:
                for suite, value in report.items():
                    path = "contracts/" + suite.split(":", 1)[0]
                    for name, result in value.get("test_results", {}).items():
                        records.append({"target": path, "name": name.removesuffix("()"), "status": "passed" if result.get("status") == "Success" else result.get("status")})
        if (not records and not runner.startswith("rust")) or any(record["status"] != "passed" for record in records):
            raise ValueError("test suite is empty, skipped, or failed")
        return records

    def check_artifacts(self):
        for name in JEST_ARTIFACTS:
            path = self.root / name
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if self.artifacts.setdefault(name, digest) != digest:
                self.failed = True
                raise ValueError("a consumed test artifact changed during execution")

    def execute(self, runner: str, target: str, selectors: list[str]) -> dict:
        with self.lock:
            self.check_inputs()
            key, command, cwd = self.plan(runner, target)
            self.execution_context()
            if runner == "jest":
                self.check_artifacts()
            if key not in self.results:
                print(f"test execution started: {key}", file=sys.stderr, flush=True)
                report_path = Path(command[command.index("--outputFile") + 1]) if runner == "jest" else None
                if report_path is not None and report_path.exists():
                    self.failed = True
                    raise ValueError("test report already exists before its owner executes the runner")
                started = time.monotonic()
                process = subprocess.Popen(command, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
                self.current_process = process
                try:
                    stdout, stderr = process.communicate(timeout=5400)
                    report_dir = self.root / "verification/output/test-execution" / self.run_id
                    report_dir.mkdir(parents=True, exist_ok=True)
                    report_name = hashlib.sha256(key.encode()).hexdigest()
                    (report_dir / (report_name + ".stdout.log")).write_text(stdout)
                    (report_dir / (report_name + ".stderr.log")).write_text(stderr)
                    self.check_inputs()
                    if process.returncode:
                        raise ValueError(f"test execution failed ({process.returncode}): {key}\n{stdout}\n{stderr}")
                    report_output = stdout
                    if report_path is not None:
                        if not report_path.is_file() or report_path.is_symlink():
                            raise ValueError("runner did not create its owned report file")
                        report_output = report_path.read_text()
                        (report_dir / (report_name + ".json")).write_text(report_output)
                    records = self.parse_results(runner, target, report_output)
                    if runner == "jest":
                        self.check_artifacts()
                except BaseException:
                    self.failed = True
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.communicate(timeout=10)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.communicate()
                    raise
                self.current_process = None
                self.results[key] = {"command": command, "cwd": str(cwd), "results": records,
                                     "elapsed_seconds": time.monotonic() - started,
                                     "report_sha256": hashlib.sha256(report_output.encode()).hexdigest()}
                print(f"test execution passed: {key}", file=sys.stderr, flush=True)
            result = self.results[key]
            try:
                assert_selected(result["results"], runner, target, selectors)
            except ValueError:
                self.failed = True
                raise
            return result

    def stop(self):
        self.failed = True
        process = self.current_process
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()

    def dispatch(self, value: dict) -> dict:
        if value.get("action") == "execute" and set(value) == {"action", "runner", "target", "selectors", "environment"}:
            if not all(isinstance(value[key], str) for key in ("runner", "target")) or not isinstance(value["selectors"], list) or not all(isinstance(s, str) for s in value["selectors"]):
                raise ValueError("invalid execution request")
            if value["environment"] != self.environment:
                raise ValueError("test execution environment differs from the run owner")
            return self.execute(value["runner"], value["target"], value["selectors"])
        if value == {"action": "snapshot"}:
            with self.lock:
                self.check_inputs()
                return {"run_id": self.run_id, "source_fingerprint": self.baseline, "units": self.results,
                        "started_at": self.started_at, "context": self.execution_context(), "artifacts": self.artifacts}
        raise ValueError("unsupported test execution request")


def required_consumers() -> list[tuple[str, str, str, str]]:
    from check_claim_test_manifest import parse_manifest
    from generate_refinement_harness import RENDERERS
    tests = parse_manifest((ROOT / "verification/claims.tsv").read_text(),
                           (ROOT / "verification/claim-test-manifest.tsv").read_text())
    required = [("claim-transaction-tests", t.runner, t.target, t.selector) for t in tests]
    for line in (ROOT / "verification/refinement-manifest.tsv").read_text().splitlines():
        section, _, _, _, runner = line.split("\t")
        renderer = RENDERERS[(section, runner)]
        required.append(("refinement-gate", "rust-core" if runner == "rust" else runner, renderer.target, renderer.selector))
    for line in (ROOT / "verification/known-answer-manifest.tsv").read_text().splitlines():
        _, runner, target, selector = line.split("\t")
        required.append(("known-answer-consumers", "rust-canister" if runner == "rust" else runner, target, selector))
    return required


def consumer_bindings(evidence: dict, stages: list[str]) -> dict[str, list[dict]]:
    bindings: dict[str, list[dict]] = {}
    for stage, runner, target, selector in required_consumers():
        if stage not in stages:
            continue
        canonical = "canister/bridge-canister/src/lib.rs" if runner.startswith("rust-canister") else target
        keys = [key for key in (runner + ":" + canonical, runner + ":") if key in evidence["units"]]
        if len(keys) != 1:
            raise ValueError(f"execution evidence is missing or ambiguous: {stage} {target}#{selector}")
        key = keys[0]
        assert_selected(evidence["units"][key]["results"], runner, target, [selector])
        bindings.setdefault(stage, []).append({"target": target, "selector": selector, "unit": key})
    return bindings


def validate_evidence(evidence: object, baseline: dict, stages: list[str]) -> None:
    if not isinstance(evidence, dict) or not isinstance(evidence.get("run_id"), str) or not re.fullmatch("[0-9a-f]{32}", evidence["run_id"]):
        raise ValueError("proof execution has no valid run identity")
    if evidence.get("source_fingerprint") != baseline:
        raise ValueError("proof execution fingerprint differs from the baseline")
    context = evidence.get("context")
    if not isinstance(context, dict) or not re.fullmatch("[0-9a-f]{40}", str(context.get("head_sha"))):
        raise ValueError("proof execution has no exact checkout identity")
    if context.get("trusted_base_sha") is not None and not re.fullmatch("[0-9a-f]{40}", str(context["trusted_base_sha"])):
        raise ValueError("proof execution has an invalid trusted base")
    if not re.fullmatch("[0-9a-f]{64}", str(context.get("environment_sha256"))) or not isinstance(context.get("submodules"), str) or not context["submodules"]:
        raise ValueError("proof execution environment is incomplete")
    tools = context.get("tools")
    if not isinstance(tools, dict) or set(tools) != {"python3", "rustc", "cargo", "node", "pnpm", "forge", "anvil", "icp", "lean", "verus", "z3"} or not all(isinstance(value, str) and value for value in tools.values()):
        raise ValueError("proof execution tool identities are incomplete")
    if not isinstance(evidence.get("units"), dict) or not isinstance(evidence.get("artifacts"), dict):
        raise ValueError("proof execution result inventory is malformed")
    if any(key.startswith("jest:") for key in evidence["units"]) and set(evidence["artifacts"]) != set(JEST_ARTIFACTS):
        raise ValueError("proof execution is missing consumed Wasm bindings")
    for name, digest in evidence["artifacts"].items():
        if not isinstance(name, str) or not re.fullmatch("[0-9a-f]{64}", str(digest)):
            raise ValueError("proof execution artifact binding is malformed")
    for key, unit in evidence["units"].items():
        if not isinstance(key, str) or not isinstance(unit, dict) or not isinstance(unit.get("command"), list) or not unit["command"] or not all(isinstance(arg, str) for arg in unit["command"]):
            raise ValueError("proof execution command is malformed")
        if not re.fullmatch("[0-9a-f]{64}", str(unit.get("report_sha256"))) or not isinstance(unit.get("results"), list):
            raise ValueError("proof execution report is malformed")
        if not all(isinstance(result, dict) and isinstance(result.get("name"), str) and isinstance(result.get("target"), str) and result.get("status") == "passed" for result in unit["results"]):
            raise ValueError("proof execution contains a non-passing test")
    if evidence.get("consumers") != consumer_bindings(evidence, stages):
        raise ValueError("proof execution consumer bindings differ from the manifests")


def verify_current_evidence(evidence: dict, root: Path) -> None:
    current = Session(root, "proofs", root).execution_context()
    for field in ("head_sha", "submodules", "tools"):
        if evidence["context"][field] != current[field]:
            raise ValueError(f"proof execution {field} differs from the current checkout")
    if current["trusted_base_sha"] is not None and evidence["context"]["trusted_base_sha"] != current["trusted_base_sha"]:
        raise ValueError("proof execution trusted base differs")
    for name, digest in evidence["artifacts"].items():
        path = root / name
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError(f"proof execution artifact differs: {name}")
    if active():
        owned = request({"action": "snapshot"})
        if evidence["run_id"] != owned["run_id"] or any(
            owned["units"].get(key) != unit for key, unit in evidence["units"].items()
        ):
            raise ValueError("proof execution does not belong to the active owner")


def collect_evidence(baseline: dict, stages: list[str]) -> dict:
    evidence = request({"action": "snapshot"})
    evidence["consumers"] = consumer_bindings(evidence, stages)
    validate_evidence(evidence, baseline, stages)
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="action", required=True)
    start = sub.add_parser("start")
    start.add_argument("mode", choices=["all", "proofs", "proofs-impacted"])
    start.add_argument("command", nargs=argparse.REMAINDER)
    suite = sub.add_parser("suite")
    suite.add_argument("runner", choices=["vitest", "jest", "foundry", "rust"])
    sub.add_parser("snapshot")
    args = parser.parse_args()
    if args.action == "suite":
        if args.runner == "rust":
            metadata = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version=1"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
            for package in metadata["packages"]:
                for target in package["targets"]:
                    if not target.get("test"):
                        continue
                    path = Path(target["src_path"]).relative_to(ROOT).as_posix()
                    runner = {"bridge-canister": "rust-canister", "bridge-profile": "rust-profile", "mock-external": "rust-mock", "bridge-core": "rust-core-lib"}.get(package["name"])
                    if package["name"] == "bridge-core" and "test" in target["kind"]:
                        runner = "rust-core"
                    if runner is None:
                        raise ValueError("workspace test target has no execution owner")
                    execute(runner, path, [])
        else:
            execute(args.runner, "", [])
        return 0
    if args.action == "snapshot":
        print(json.dumps(request({"action": "snapshot"})))
        return 0
    if active():
        raise ValueError("cannot adopt or resume another test execution session")
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("a gate command is required")
    with tempfile.TemporaryDirectory(prefix="bridge-test-owner.") as temporary:
        session = Session(ROOT, args.mode, Path(temporary))
        endpoint = str(Path(temporary) / "owner.sock")
        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                try:
                    payload = self.rfile.readline(MAX_REQUEST + 1)
                    if len(payload) > MAX_REQUEST:
                        raise ValueError("test request too large")
                    response = session.dispatch(json.loads(payload))
                except Exception as error:
                    response = {"error": str(error)}
                self.wfile.write(json.dumps(response).encode() + b"\n")
        with socketserver.ThreadingUnixStreamServer(endpoint, Handler) as server:
            os.chmod(endpoint, 0o600)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            child = subprocess.Popen(command, cwd=ROOT, env={**os.environ, SESSION_ENV: endpoint}, start_new_session=True)
            def interrupted(signum, _frame):
                raise SystemExit(128 + signum)
            signal.signal(signal.SIGTERM, interrupted)
            signal.signal(signal.SIGINT, interrupted)
            try:
                status = child.wait()
                if status == 0:
                    session.check_inputs()
                return status if status else int(session.failed)
            finally:
                session.stop()
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGTERM)
                    try:
                        child.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait()
                server.shutdown()
                thread.join()


if __name__ == "__main__":
    raise SystemExit(main())
