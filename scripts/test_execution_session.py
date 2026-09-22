#!/usr/bin/env python3
"""Exercise result ownership, test identity, and fail-closed reuse boundaries."""
import hashlib
import json
from pathlib import Path
import re
import tempfile
import unittest
from unittest.mock import patch, Mock

import execution_session as execution

BASELINE = {"algorithm": "sha256", "digest": "a" * 64, "input_count": 1}


def fixture_evidence(baseline):
    evidence = {
        "run_id": "1" * 32, "source_fingerprint": baseline,
        "context": {"head_sha": "2" * 40, "trusted_base_sha": None,
                    "environment_sha256": "3" * 64, "submodules": "fixture",
                    "tools": {tool: "fixture" for tool in ("python3", "rustc", "cargo", "node", "pnpm", "forge", "anvil", "icp", "lean", "verus", "z3")}},
        "artifacts": {name: "5" * 64 for name in execution.JEST_ARTIFACTS}, "units": {},
    }
    stages = []
    for stage, runner, target, selector in execution.required_consumers():
        stages.append(stage)
        canonical = "canister/bridge-canister/src/lib.rs" if runner.startswith("rust-canister") else target
        key = runner + ":" + canonical
        name = selector
        if runner.startswith("rust-canister"):
            parts = list(Path(target).relative_to("canister/bridge-canister/src").with_suffix("").parts)
            if parts[-1] in {"lib", "mod"}:
                parts.pop()
            name = "::".join([*parts, "tests", selector])
        elif runner == "rust-profile":
            name = "tests::" + selector
        unit = evidence["units"].setdefault(key, {"command": ["fixture"], "report_sha256": "4" * 64, "results": []})
        record = {"target": target, "name": name, "status": "passed"}
        if record not in unit["results"]:
            unit["results"].append(record)
    evidence["consumers"] = execution.consumer_bindings(evidence, stages)
    return evidence


class ExecutionTests(unittest.TestCase):
    def session(self, root, mode="proofs"):
        with patch.object(execution, "source_fingerprint", return_value=BASELINE):
            session = execution.Session(root, mode, root)
        session.context = {"fixture": True}
        return session

    def test_reuses_only_owner_executed_results_and_rejects_changed_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = "canister/bridge-core/tests/example.rs"
            (root / target).parent.mkdir(parents=True)
            (root / target).write_text("fixture")
            session = self.session(root)
            process = Mock(returncode=0)
            process.communicate.return_value = ("running 2 tests\ntest first ... ok\ntest second ... ok\n", "")
            with patch.object(execution, "source_fingerprint", return_value=BASELINE), patch.object(execution.subprocess, "Popen", return_value=process) as launch:
                session.execute("rust-core", target, ["first"])
                session.execute("rust-core", target, ["second"])
                self.assertEqual(launch.call_count, 1)
                with self.assertRaisesRegex(ValueError, "exactly once"):
                    session.execute("rust-core", target, ["absent"])
            with patch.object(execution, "source_fingerprint", return_value={**BASELINE, "digest": "b" * 64}):
                with self.assertRaisesRegex(ValueError, "inputs changed"):
                    session.execute("rust-core", target, ["first"])
            with patch.object(execution, "source_fingerprint", return_value=BASELINE):
                with self.assertRaisesRegex(ValueError, "already failed"):
                    session.execute("rust-core", target, ["first"])

    def test_no_result_registration_or_environment_switch_is_accepted(self):
        session = self.session(Path("/tmp"))
        for payload in [{"action": "register", "status": "passed"}, {"action": "execute", "runner": "vitest", "target": "ui/example.test.ts", "selectors": ["pass"], "environment": "foreign"}]:
            with self.assertRaises(ValueError):
                session.dispatch(payload)
        self.assertEqual(session.results, {})
        self.assertNotEqual(session.run_id, self.session(Path("/tmp")).run_id)

    def test_changed_consumed_artifact_reports_its_identity_and_digests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in execution.JEST_ARTIFACTS:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
            session = self.session(root)
            session.check_artifacts()
            changed = execution.JEST_ARTIFACTS[1]
            baseline = session.artifacts[changed]
            (root / changed).write_bytes(b"changed")
            actual = hashlib.sha256(b"changed").hexdigest()
            with self.assertRaisesRegex(
                ValueError,
                f"{re.escape(changed)} expected={baseline} actual={actual}",
            ):
                session.check_artifacts()

    def test_missing_duplicate_skipped_or_failed_results_do_not_satisfy_a_selector(self):
        record = {"target": "ui/example.test.ts", "name": "pass", "status": "passed"}
        for results in [[], [record, record], [{**record, "status": "skipped"}], [{**record, "status": "failed"}], [{**record, "target": "ui/other.test.ts"}]]:
            with self.assertRaisesRegex(ValueError, "exactly once"):
                execution.assert_selected(results, "vitest", "ui/example.test.ts", ["pass"])

    def test_receipt_rejects_missing_or_forged_consumer_bindings(self):
        import copy
        evidence = fixture_evidence(BASELINE)
        stages = ["refinement-gate", "claim-transaction-tests", "known-answer-consumers"]
        execution.validate_evidence(evidence, BASELINE, stages)
        mutations = [
            lambda value: value["units"].pop(next(iter(value["units"]))),
            lambda value: value.__setitem__("consumers", {}),
            lambda value: value.__setitem__("artifacts", {}),
            lambda value: value["context"].__setitem__("tools", {}),
            lambda value: value.__setitem__("run_id", "old-run"),
            lambda value: value.__setitem__("source_fingerprint", {**BASELINE, "digest": "b" * 64}),
        ]
        for mutate in mutations:
            altered = copy.deepcopy(evidence)
            mutate(altered)
            with self.assertRaises(ValueError):
                execution.validate_evidence(altered, BASELINE, stages)

    def test_workspace_inventory_distinguishes_library_and_declared_binary_targets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            library = "canister/mock-external/src/lib.rs"
            binary = "canister/mock-external/src/bin/export-candid.rs"
            for target in (library, binary):
                (root / target).parent.mkdir(parents=True, exist_ok=True)
                (root / target).write_text("fixture")
            session = self.session(root, "all")
            session.metadata = {"workspace_members": ["mock-id"], "packages": [
                {"id": "mock-id", "name": "mock-external", "targets": [
                    {"name": "mock_external", "src_path": str(root / library), "kind": ["cdylib", "rlib"], "test": True},
                    {"name": "declared-export-name", "src_path": str(root / binary), "kind": ["bin"], "test": True}]}]}
            self.assertEqual(session.workspace_targets(), [("rust-mock", library), ("rust-bin", binary)])
            command = session.plan("rust-bin", binary)[1]
            self.assertEqual(command[command.index("--bin") + 1], "declared-export-name")
            with self.assertRaisesRegex(ValueError, "not uniquely declared"):
                session.plan("rust-bin", library)

    def test_profile_proofs_select_registered_tests_while_all_keeps_the_full_suite(self):
        target = "tools/bridge-profile/src/main.rs"
        proof_command = self.session(execution.ROOT).plan("rust-profile", target)[1]
        all_command = self.session(execution.ROOT, "all").plan("rust-profile", target)[1]
        for selector in (
            "current_state_gate_rejects_each_operational_drift",
            "production_upgrade_submission_rejects_target_sender_wasm_and_signature_drift",
        ):
            self.assertIn(selector, proof_command)
            self.assertNotIn(selector, all_command)

    def test_jest_uses_a_fresh_owned_report_despite_child_process_stdout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = "integration/example.spec.ts"
            (root / target).parent.mkdir()
            (root / target).write_text("fixture")
            session = self.session(root)
            _, command, _ = session.plan("jest", target)
            report_path = Path(command[command.index("--outputFile") + 1])
            report = {"success": True, "numFailedTests": 0, "numTotalTests": 1,
                      "testResults": [{"name": str(root / target), "assertionResults": [
                          {"title": "first", "fullName": "suite first", "ancestorTitles": ["suite"], "status": "passed"}]}]}
            process = Mock(returncode=0)
            def finish(**_kwargs):
                report_path.write_text(json.dumps(report))
                return "PocketIC child process output\n", ""
            process.communicate.side_effect = finish
            with patch.object(execution, "source_fingerprint", return_value=BASELINE), patch.object(execution.Session, "check_artifacts"), patch.object(execution.subprocess, "Popen", return_value=process) as launch:
                session.execute("jest", target, ["first"])
                session.execute("jest", target, ["first"])
                with self.assertRaisesRegex(ValueError, "already exists"):
                    self.session(root).execute("jest", target, ["first"])
                self.assertEqual(launch.call_count, 1)

    def test_current_checkout_and_active_owner_must_match_receipt(self):
        evidence = fixture_evidence(BASELINE)
        evidence["artifacts"] = {}
        with patch.object(execution.Session, "execution_context", return_value=evidence["context"]), patch.object(execution, "active", return_value=True), patch.object(execution, "source_fingerprint", return_value=BASELINE):
            owned = {"run_id": evidence["run_id"], "units": evidence["units"]}
            with patch.object(execution, "request", return_value=owned):
                execution.verify_current_evidence(evidence, execution.ROOT)
            with patch.object(execution, "request", return_value={**owned, "run_id": "2" * 32}):
                with self.assertRaisesRegex(ValueError, "active owner"):
                    execution.verify_current_evidence(evidence, execution.ROOT)
        with patch.object(execution.Session, "execution_context", return_value={**evidence["context"], "head_sha": "9" * 40}), patch.object(execution, "source_fingerprint", return_value=BASELINE):
            with self.assertRaisesRegex(ValueError, "head_sha"):
                execution.verify_current_evidence(evidence, execution.ROOT)

    def test_canister_module_identity_and_features_are_distinct(self):
        session = self.session(execution.ROOT)
        target = "canister/bridge-canister/src/storage/mod.rs"
        normal = session.plan("rust-canister", target)
        staging = session.plan("rust-canister-test-deployment", target)
        self.assertNotEqual(normal[0], staging[0])
        with self.assertRaisesRegex(ValueError, "exactly once"):
            execution.assert_selected([{"target": target, "name": "api::tests::example", "status": "passed"}], "rust-canister", target, ["example"])

    def test_malformed_or_incomplete_runner_output_is_rejected(self):
        session = self.session(Path("/tmp"))
        for runner, output in [("rust-core", "running 2 tests\ntest first ... ok\n"), ("rust-core", "running 1 test\ntest first ... ignored\n"), ("vitest", ""), ("vitest", json.dumps({"success": False})), ("foundry", "{}"), ("foundry", '{"duplicate":1,"duplicate":2}')]:
            with self.assertRaises(ValueError):
                session.parse_results(runner, "target", output)


if __name__ == "__main__":
    unittest.main()
