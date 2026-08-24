#!/usr/bin/env python3
"""Regression tests for compiler-AST proof and production bindings."""

from pathlib import Path
from collections import Counter
import tempfile
import unittest

from check_solidity_ast_bindings import (
    AstIndex,
    FunctionRecord,
    declaration_initializer_call,
    declaration_initializer_library_call,
    library_calls,
    require_call_argument_declarations,
    require_closed_call_set,
    require_evaluate_input_binding,
    require_no_declaration_reassignment,
    validate_smt_call_graph,
)
from smt_obligations import SmtObligation


def call(name: str, declaration: int) -> dict[str, object]:
    return {
        "nodeType": "FunctionCall",
        "expression": {
            "nodeType": "Identifier",
            "name": name,
            "referencedDeclaration": declaration,
        },
        "arguments": [],
    }


class FakeIndex:
    def __init__(self, records: dict[str, tuple[FunctionRecord, ...]]) -> None:
        self.records = records

    def resolve(self, link: str) -> tuple[FunctionRecord, ...]:
        return self.records[link]


class SolidityAstBindingTests(unittest.TestCase):
    @staticmethod
    def library_call(library: str, member: str) -> dict[str, object]:
        return {
            "id": 17,
            "nodeType": "FunctionCall",
            "expression": {
                "nodeType": "MemberAccess",
                "memberName": member,
                "expression": {
                    "nodeType": "Identifier",
                    "name": library,
                    "typeDescriptions": {"typeString": f"type(library {library})"},
                },
            },
            "arguments": [],
        }

    @staticmethod
    def evaluate_input() -> tuple[dict[str, object], dict[str, int]]:
        authorization = 11
        variable_names = (
            "mintAuthorizationEpoch",
            "bsns",
            "MAX_SERVICE_FEE",
            "perDepositLimit",
            "mintedInWindow",
            "mintWindowLimit",
            "mintWindowStartedAt",
            "mintWindowDuration",
            "depositMintsPaused",
            "_processedDeposits",
        )
        variables = {name: 100 + index for index, name in enumerate(variable_names)}

        def identifier(declaration: int, name: str = "value") -> dict[str, object]:
            return {
                "nodeType": "Identifier",
                "name": name,
                "referencedDeclaration": declaration,
            }

        def member(base: dict[str, object], name: str) -> dict[str, object]:
            return {"nodeType": "MemberAccess", "expression": base, "memberName": name}

        def address(value: dict[str, object]) -> dict[str, object]:
            return {
                "nodeType": "FunctionCall",
                "kind": "typeConversion",
                "expression": {
                    "nodeType": "ElementaryTypeNameExpression",
                    "typeName": {"name": "address"},
                },
                "arguments": [value],
            }

        def auth() -> dict[str, object]:
            return identifier(authorization, "authorization")
        names = [
            "timestamp",
            "deadline",
            "authorizationEpoch",
            "currentEpoch",
            "recipient",
            "bridge",
            "token",
            "grossAmount",
            "maximumFee",
            "chargedFee",
            "protocolMaximumFee",
            "perDepositLimit",
            "consumedInWindow",
            "windowLimit",
            "windowStartedAt",
            "windowDuration",
            "paused",
            "processed",
        ]
        arguments = [
            member(identifier(-4, "block"), "timestamp"),
            member(auth(), "deadline"),
            member(auth(), "authorizationEpoch"),
            identifier(variables["mintAuthorizationEpoch"]),
            member(auth(), "recipient"),
            address(identifier(-28, "this")),
            address(identifier(variables["bsns"], "bsns")),
            member(auth(), "grossAmount"),
            member(auth(), "maxServiceFee"),
            member(auth(), "chargedServiceFee"),
            identifier(variables["MAX_SERVICE_FEE"]),
            identifier(variables["perDepositLimit"]),
            identifier(variables["mintedInWindow"]),
            identifier(variables["mintWindowLimit"]),
            identifier(variables["mintWindowStartedAt"]),
            identifier(variables["mintWindowDuration"]),
            identifier(variables["depositMintsPaused"]),
            {
                "nodeType": "IndexAccess",
                "baseExpression": identifier(variables["_processedDeposits"]),
                "indexExpression": member(auth(), "depositId"),
            },
        ]
        return (
            {
                "arguments": [
                    {
                        "nodeType": "FunctionCall",
                        "kind": "structConstructorCall",
                        "names": names,
                        "arguments": arguments,
                    }
                ]
            },
            variables,
        )

    def obligation(self) -> SmtObligation:
        return SmtObligation(
            "example",
            "supporting",
            ("pass.sol#Harness.check()",),
            ("production.sol#Policy.kernel(uint256)",),
            ("failure",),
            ("claim",),
        )

    def records(self, kernel_declaration: int) -> FakeIndex:
        source = Path("/")
        pass_record = FunctionRecord(
            source,
            "Harness",
            "check()",
            1,
            {"body": {"statements": [call("kernel", kernel_declaration), call("assert", 99)]}},
        )
        production_record = FunctionRecord(
            source, "Policy", "kernel(uint256)", 2, {"body": {"statements": []}}
        )
        return FakeIndex(
            {
                "pass.sol#Harness.check()": (pass_record,),
                "production.sol#Policy.kernel(uint256)": (production_record,),
            }
        )

    def test_accepts_direct_declaration_binding(self) -> None:
        validate_smt_call_graph(self.records(2), {"example": self.obligation()})

    def test_rejects_same_name_bound_to_another_declaration(self) -> None:
        with self.assertRaisesRegex(ValueError, "does not call its production kernel"):
            validate_smt_call_graph(self.records(3), {"example": self.obligation()})

    def test_rejects_pass_function_without_an_assertion(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        pass_record.node["body"] = {"statements": [call("kernel", 2)]}
        with self.assertRaisesRegex(ValueError, "has no assertion"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_registered_production_kernel_without_pass_coverage(self) -> None:
        records = self.records(2)
        second_link = "production.sol#Policy.second(uint256)"
        records.records[second_link] = (
            FunctionRecord(Path("/"), "Policy", "second(uint256)", 3, {"body": {}}),
        )
        obligation = SmtObligation(
            "example",
            "supporting",
            self.obligation().pass_links,
            (*self.obligation().production_links, second_link),
            ("failure",),
            ("claim",),
        )
        with self.assertRaisesRegex(ValueError, "not covered by a pass function"):
            validate_smt_call_graph(records, {"example": obligation})

    def test_rejects_noncanonical_link_before_artifact_lookup(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            index = AstIndex(Path(directory), Path(directory))
            with self.assertRaisesRegex(ValueError, "invalid canonical Solidity function link"):
                index.resolve("Bridge.sol#mintDepositWithAuthorization")

    def test_accepts_exact_commit_argument_declarations(self) -> None:
        call_node = {
            "arguments": [
                {"nodeType": "Identifier", "referencedDeclaration": 11},
                {"nodeType": "Identifier", "referencedDeclaration": 12},
                {"nodeType": "Identifier", "referencedDeclaration": 13},
            ]
        }
        require_call_argument_declarations(call_node, (11, 12, 13))

    def test_accepts_compiler_bound_library_initializer_across_build_units(self) -> None:
        initializer = self.library_call("Policy", "evaluate")
        body = {
            "nodeType": "Block",
            "statements": [
                {
                    "nodeType": "VariableDeclarationStatement",
                    "declarations": [{"id": 11}],
                    "initialValue": initializer,
                }
            ],
        }
        self.assertEqual(library_calls(body, "Policy", "evaluate"), [initializer])
        self.assertIs(
            declaration_initializer_library_call(body, 11, "Policy", "evaluate"),
            initializer,
        )

    def test_rejects_same_member_name_not_bound_to_expected_library(self) -> None:
        initializer = self.library_call("Spoof", "evaluate")
        body = {
            "nodeType": "Block",
            "statements": [
                {
                    "nodeType": "VariableDeclarationStatement",
                    "declarations": [{"id": 11}],
                    "initialValue": initializer,
                }
            ],
        }
        with self.assertRaisesRegex(ValueError, "not compiler-bound to Policy.evaluate"):
            declaration_initializer_library_call(body, 11, "Policy", "evaluate")

    def test_rejects_reordered_commit_arguments(self) -> None:
        call_node = {
            "arguments": [
                {"nodeType": "Identifier", "referencedDeclaration": 12},
                {"nodeType": "Identifier", "referencedDeclaration": 11},
                {"nodeType": "Identifier", "referencedDeclaration": 13},
            ]
        }
        with self.assertRaisesRegex(ValueError, "argument declarations differ"):
            require_call_argument_declarations(call_node, (11, 12, 13))

    def test_rejects_same_name_with_another_declaration(self) -> None:
        call_node = {
            "arguments": [
                {
                    "nodeType": "Identifier",
                    "name": "authorization",
                    "referencedDeclaration": 99,
                },
                {"nodeType": "Identifier", "referencedDeclaration": 12},
                {"nodeType": "Identifier", "referencedDeclaration": 13},
            ]
        }
        with self.assertRaisesRegex(ValueError, "argument declarations differ"):
            require_call_argument_declarations(call_node, (11, 12, 13))

    def test_rejects_derived_commit_argument(self) -> None:
        call_node = {
            "arguments": [
                {"nodeType": "Identifier", "referencedDeclaration": 11},
                {"nodeType": "FunctionCall", "referencedDeclaration": 12},
                {"nodeType": "Identifier", "referencedDeclaration": 13},
            ]
        }
        with self.assertRaisesRegex(ValueError, "argument declarations differ"):
            require_call_argument_declarations(call_node, (11, 12, 13))

    def test_rejects_commit_argument_count_mismatch(self) -> None:
        call_node = {
            "arguments": [
                {"nodeType": "Identifier", "referencedDeclaration": 11},
                {"nodeType": "Identifier", "referencedDeclaration": 12},
            ]
        }
        with self.assertRaisesRegex(ValueError, "argument declarations differ"):
            require_call_argument_declarations(call_node, (11, 12, 13))

    def test_accepts_exact_declaration_initializer(self) -> None:
        initializer = call("derive", 21)
        initializer["arguments"] = [
            {"nodeType": "Identifier", "referencedDeclaration": 11}
        ]
        node = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 12}],
            "initialValue": initializer,
        }
        actual = declaration_initializer_call(node, 12, 21)
        require_call_argument_declarations(actual, (11,))

    def test_rejects_initializer_from_another_function(self) -> None:
        node = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 12}],
            "initialValue": call("other", 22),
        }
        with self.assertRaisesRegex(ValueError, "initializer differs"):
            declaration_initializer_call(node, 12, 21)

    def test_rejects_bound_declaration_reassignment(self) -> None:
        node = {
            "nodeType": "Assignment",
            "leftHandSide": {
                "nodeType": "MemberAccess",
                "expression": {
                    "nodeType": "Identifier",
                    "referencedDeclaration": 13,
                },
            },
        }
        with self.assertRaisesRegex(ValueError, "reassigns bound declarations"):
            require_no_declaration_reassignment(node, {12, 13})

    def test_accepts_read_only_bound_declarations(self) -> None:
        node = {
            "nodeType": "FunctionCall",
            "arguments": [
                {"nodeType": "Identifier", "referencedDeclaration": 12},
                {"nodeType": "Identifier", "referencedDeclaration": 13},
            ],
        }
        require_no_declaration_reassignment(node, {12, 13})

    def test_accepts_exact_evaluate_transition_input(self) -> None:
        call_node, variables = self.evaluate_input()
        require_evaluate_input_binding(call_node, 11, variables)

    def test_rejects_reordered_evaluate_transition_input(self) -> None:
        call_node, variables = self.evaluate_input()
        arguments = call_node["arguments"][0]["arguments"]
        arguments[1], arguments[2] = arguments[2], arguments[1]
        with self.assertRaisesRegex(ValueError, "input binding differs"):
            require_evaluate_input_binding(call_node, 11, variables)

    def test_rejects_low_level_call_even_when_expected_calls_remain(self) -> None:
        node = {
            "nodeType": "Block",
            "statements": [
                call("expected", 1),
                {
                    "nodeType": "FunctionCall",
                    "expression": {
                        "nodeType": "MemberAccess",
                        "memberName": "call",
                        "expression": {"nodeType": "Identifier", "name": "token"},
                    },
                },
            ],
        }
        with self.assertRaisesRegex(ValueError, "low-level calls"):
            require_closed_call_set(node, Counter({"expected": 1}), "mint wrapper")

    def test_rejects_extra_direct_call_in_closed_wrapper(self) -> None:
        node = {
            "nodeType": "Block",
            "statements": [call("expected", 1), call("unexpectedMint", 2)],
        }
        with self.assertRaisesRegex(ValueError, "call set differs"):
            require_closed_call_set(node, Counter({"expected": 1}), "mint wrapper")

    def test_rejects_inline_assembly_in_closed_wrapper(self) -> None:
        node = {
            "nodeType": "Block",
            "statements": [call("expected", 1), {"nodeType": "InlineAssembly"}],
        }
        with self.assertRaisesRegex(ValueError, "inline assembly"):
            require_closed_call_set(node, Counter({"expected": 1}), "mint wrapper")


if __name__ == "__main__":
    unittest.main()
