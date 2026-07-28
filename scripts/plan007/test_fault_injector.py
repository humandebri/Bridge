#!/usr/bin/env python3
"""Exercise restoration guarantees in the test-only EVM RPC fault injector."""

from __future__ import annotations

import hashlib
import importlib.machinery
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch


INJECTOR_PATH = Path(__file__).with_name("evm-rpc-fault-injector")
LOADER = importlib.machinery.SourceFileLoader("evm_rpc_fault_injector", str(INJECTOR_PATH))
SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
assert SPEC is not None
injector = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(injector)


class FaultInjectorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.events: list[dict[str, object]] = []
        self.rpc_urls = [f"https://rpc-{index}.example.test" for index in range(3)]
        self.config = {
            "schema_version": 1,
            "bridge_canister_id": "aaaaa-aa",
            "identity": "staging-test",
            "provider_controls": [
                {"rpc_url": url, "control_url": f"https://control-{index}.example.test"}
                for index, url in enumerate(self.rpc_urls)
            ],
            "request_deposit_candid_args": "(record {})",
            "audit_cursor": 0,
        }

    def request(self, scenario: str) -> dict[str, object]:
        return {
            "rehearsal_id": "test-run",
            "scenario": scenario,
            "run_reference": f"{scenario}-1",
            "provider_url_digests": [hashlib.sha256(url.encode()).hexdigest() for url in self.rpc_urls],
            "failed_provider_indices": [0] if scenario == "single_provider_failure" else [0, 1],
            "failure_rule": "connection-refused",
        }

    def post(self, url: str, token: str, body: dict[str, object]) -> dict[str, str]:
        self.events.append(body)
        return {"result": "applied" if body["failed"] else "restored"}

    @staticmethod
    def icp(config: dict[str, object], method: str, candid_args: str, *, query: bool = False) -> dict[str, object]:
        if method != "get_audit_events":
            return {"Ok": {}}
        return {
            "Ok": {
                "events": [
                    {
                        "sequence": "9",
                        "timestamp_ns": "123",
                        "kind": {
                            "EvmRpcDecision": {
                                "kind": "QuorumContinued",
                                "operation": "request_deposit",
                                "configured_provider_count": 3,
                                "required_threshold": 2,
                                "ledger_call_performed": False,
                                "bridge_operation_continued": True,
                            }
                        },
                    }
                ]
            }
        }

    def run_scenario(self, scenario: str) -> dict[str, object]:
        with patch.object(injector, "post_control", side_effect=self.post), patch.object(injector, "run_icp", side_effect=self.icp):
            return injector.execute(self.request(scenario), self.config, "not-recorded")

    def test_single_provider_is_applied_and_restored(self) -> None:
        result = self.run_scenario("single_provider_failure")
        self.assertEqual(result["applied_provider_indices"], [0])
        self.assertEqual(result["restored_provider_indices"], [0])
        self.assertEqual([event["failed"] for event in self.events], [True, False])

    def test_quorum_loss_restores_every_applied_provider(self) -> None:
        result = self.run_scenario("quorum_loss")
        self.assertEqual(result["applied_provider_indices"], [0, 1])
        self.assertEqual(result["restored_provider_indices"], [0, 1])
        self.assertEqual([event["failed"] for event in self.events], [True, True, False, False])

    def test_partial_apply_failure_still_restores_the_first_provider(self) -> None:
        calls = 0

        def fail_second(url: str, token: str, body: dict[str, object]) -> dict[str, str]:
            nonlocal calls
            calls += 1
            self.events.append(body)
            if calls == 2:
                raise RuntimeError("second controller unavailable")
            return {"result": "applied" if body["failed"] else "restored"}

        with patch.object(injector, "post_control", side_effect=fail_second):
            with self.assertRaisesRegex(RuntimeError, "second controller unavailable"):
                injector.execute(self.request("quorum_loss"), self.config, "not-recorded")
        self.assertEqual([event["failed"] for event in self.events], [True, True, False])


if __name__ == "__main__":
    unittest.main()
