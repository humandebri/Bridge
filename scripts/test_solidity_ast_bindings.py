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
    require_exact_state_writes,
    require_exact_commit_statements,
    require_no_modifiers,
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
        kernel_call = call("kernel", kernel_declaration)
        kernel_call["src"] = "10:1:0"
        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [kernel_call]
        pass_record = FunctionRecord(
            source,
            "Harness",
            "check()",
            1,
            {"body": {"statements": [assertion]}},
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

    def test_accepts_direct_nested_assertion_with_real_source_order(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        assertion = pass_record.node["body"]["statements"][0]
        assertion["src"] = "10:20:0"
        assertion["arguments"][0]["src"] = "17:5:0"
        validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_same_name_bound_to_another_declaration(self) -> None:
        with self.assertRaisesRegex(ValueError, "does not call its production kernel"):
            validate_smt_call_graph(self.records(3), {"example": self.obligation()})

    def test_rejects_pass_function_without_an_assertion(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        pass_record.node["body"] = {"statements": [kernel_call]}
        with self.assertRaisesRegex(ValueError, "has no assertion"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_assert_true_unrelated_to_production_call(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [{"nodeType": "Literal", "value": "true"}]
        pass_record.node["body"] = {"statements": [kernel_call, assertion]}
        with self.assertRaisesRegex(ValueError, "does not depend on production result"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_assertion_before_production_call(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "20:1:0"
        assertion = call("assert", -3)
        assertion["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }
        assertion["arguments"] = [
            {"nodeType": "Identifier", "referencedDeclaration": 7}
        ]
        pass_record.node["body"] = {"statements": [assertion, declaration]}
        with self.assertRaisesRegex(ValueError, "does not depend on production result"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_control_dependent_assert_true(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }
        assertion = call("assert", -3)
        assertion["src"] = "30:1:0"
        assertion["arguments"] = [{"nodeType": "Literal", "value": "true"}]
        branch = {
            "nodeType": "IfStatement",
            "condition": {"nodeType": "Identifier", "referencedDeclaration": 7},
            "trueBody": {"statements": [assertion]},
        }
        pass_record.node["body"] = {"statements": [declaration, branch]}
        with self.assertRaisesRegex(ValueError, "does not depend on production result"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_reassigned_production_result(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }
        reassignment = {
            "nodeType": "Assignment",
            "operator": "=",
            "leftHandSide": {
                "nodeType": "Identifier",
                "referencedDeclaration": 7,
            },
            "rightHandSide": {"nodeType": "Literal", "value": "true"},
        }
        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [
            {"nodeType": "Identifier", "referencedDeclaration": 7}
        ]
        pass_record.node["body"] = {
            "statements": [declaration, reassignment, assertion]
        }
        with self.assertRaisesRegex(ValueError, "production result is reassigned"):
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

    def test_rejects_modifier_on_production_kernel(self) -> None:
        records = self.records(2)
        production = records.records["production.sol#Policy.kernel(uint256)"][0]
        production.node["modifiers"] = [{"modifierName": {"name": "never"}}]
        with self.assertRaisesRegex(ValueError, "must not use modifiers"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_user_defined_or_member_assert(self) -> None:
        for expression in (
            {
                "nodeType": "Identifier",
                "name": "assert",
                "referencedDeclaration": 99,
            },
            {
                "nodeType": "MemberAccess",
                "memberName": "assert",
                "referencedDeclaration": 99,
                "expression": {"nodeType": "Identifier", "name": "helper"},
            },
        ):
            with self.subTest(expression=expression):
                records = self.records(2)
                assertion = records.records["pass.sol#Harness.check()"][0].node[
                    "body"
                ]["statements"][0]
                assertion["expression"] = expression
                with self.assertRaisesRegex(ValueError, "has no assertion"):
                    validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_smt_pass_modifier(self) -> None:
        records = self.records(2)
        records.records["pass.sol#Harness.check()"][0].node["modifiers"] = [
            {"modifierName": {"name": "vacuous"}}
        ]
        with self.assertRaisesRegex(ValueError, "must not use modifiers"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_self_comparison_of_production_result(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }
        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [
            {
                "nodeType": "BinaryOperation",
                "operator": "==",
                "leftExpression": {
                    "nodeType": "Identifier",
                    "referencedDeclaration": 7,
                },
                "rightExpression": {
                    "nodeType": "Identifier",
                    "referencedDeclaration": 7,
                },
            }
        ]
        pass_record.node["body"] = {"statements": [declaration, assertion]}
        with self.assertRaisesRegex(ValueError, "does not depend on production result"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_accepts_distinct_arithmetic_expressions_using_production_result(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }
        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [
            {
                "nodeType": "BinaryOperation",
                "operator": "==",
                "leftExpression": {
                    "nodeType": "BinaryOperation",
                    "operator": "+",
                    "leftExpression": {
                        "nodeType": "Identifier",
                        "referencedDeclaration": 7,
                    },
                    "rightExpression": {"nodeType": "Literal", "value": "1"},
                },
                "rightExpression": {
                    "nodeType": "BinaryOperation",
                    "operator": "+",
                    "leftExpression": {
                        "nodeType": "Identifier",
                        "referencedDeclaration": 8,
                    },
                    "rightExpression": {"nodeType": "Literal", "value": "1"},
                },
            }
        ]
        pass_record.node["body"] = {"statements": [declaration, assertion]}
        validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_identical_type_conversions_of_production_result(self) -> None:
        records = self.records(2)
        pass_record = records.records["pass.sol#Harness.check()"][0]
        kernel_call = call("kernel", 2)
        kernel_call["src"] = "10:1:0"
        declaration = {
            "nodeType": "VariableDeclarationStatement",
            "declarations": [{"nodeType": "VariableDeclaration", "id": 7}],
            "initialValue": kernel_call,
        }

        def conversion() -> dict[str, object]:
            return {
                "nodeType": "FunctionCall",
                "kind": "typeConversion",
                "expression": {
                    "nodeType": "ElementaryTypeNameExpression",
                    "typeName": {"name": "uint256"},
                },
                "arguments": [
                    {"nodeType": "Identifier", "referencedDeclaration": 7}
                ],
            }

        assertion = call("assert", -3)
        assertion["src"] = "20:1:0"
        assertion["arguments"] = [
            {
                "nodeType": "BinaryOperation",
                "operator": "==",
                "leftExpression": conversion(),
                "rightExpression": conversion(),
            }
        ]
        pass_record.node["body"] = {"statements": [declaration, assertion]}
        with self.assertRaisesRegex(ValueError, "does not depend on production result"):
            validate_smt_call_graph(records, {"example": self.obligation()})

    def test_rejects_noncanonical_link_before_artifact_lookup(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            index = AstIndex(Path(directory), Path(directory))
            with self.assertRaisesRegex(ValueError, "invalid canonical Solidity function link"):
                index.resolve("Bridge.sol#mintDepositWithAuthorization")

    def test_rejects_modifier_on_trusted_function(self) -> None:
        record = FunctionRecord(
            Path("/Bridge.sol"),
            "Bridge",
            "commit()",
            1,
            {"modifiers": [{"nodeType": "ModifierInvocation"}]},
        )
        with self.assertRaisesRegex(ValueError, "must not use modifiers"):
            require_no_modifiers(record)

    def test_commit_statements_must_be_unconditional_and_ordered(self) -> None:
        def assignment(left: int, right: int) -> dict[str, object]:
            return {
                "nodeType": "Assignment",
                "operator": "=",
                "leftHandSide": {"nodeType": "Identifier", "referencedDeclaration": left},
                "rightHandSide": {"nodeType": "Identifier", "referencedDeclaration": right},
            }

        writes = tuple(
            ("=", ("declaration", left), ("declaration", right))
            for left, right in ((1, 11), (2, 12), (3, 13))
        )
        expressions = [
            {"nodeType": "ExpressionStatement", "expression": assignment(left, right)}
            for left, right in ((1, 11), (2, 12), (3, 13))
        ]
        expressions.extend(
            (
                {"nodeType": "ExpressionStatement", "expression": call("bridgeMint", 20)},
                {"nodeType": "EmitStatement"},
            )
        )
        require_exact_commit_statements({"statements": expressions}, writes)
        conditional = {"nodeType": "IfStatement", "trueBody": expressions[0]}
        with self.assertRaisesRegex(ValueError, "statement sequence differs"):
            require_exact_commit_statements(
                {"statements": [conditional, *expressions[1:]]}, writes
            )
        with self.assertRaisesRegex(ValueError, "write order differs"):
            require_exact_commit_statements(
                {"statements": [expressions[1], expressions[0], *expressions[2:]]}, writes
            )

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

    @staticmethod
    def exact_commit_write() -> tuple[
        dict[str, object], dict[str, int], Counter[tuple[object, ...]]
    ]:
        variables = {"_processedDeposits": 100}
        authorization = {
            "nodeType": "Identifier",
            "name": "authorization",
            "referencedDeclaration": 11,
        }
        effects = {
            "nodeType": "Identifier",
            "name": "effects",
            "referencedDeclaration": 12,
        }
        left = {
            "nodeType": "IndexAccess",
            "baseExpression": {
                "nodeType": "Identifier",
                "name": "_processedDeposits",
                "referencedDeclaration": 100,
            },
            "indexExpression": {
                "nodeType": "MemberAccess",
                "expression": authorization,
                "memberName": "depositId",
            },
        }
        right = {
            "nodeType": "MemberAccess",
            "expression": effects,
            "memberName": "processedAfter",
        }
        node = {
            "nodeType": "Assignment",
            "operator": "=",
            "leftHandSide": left,
            "rightHandSide": right,
        }
        expected = Counter(
            {
                (
                    "=",
                    (
                        "index",
                        ("declaration", 100),
                        ("member", ("declaration", 11), "depositId"),
                    ),
                    ("member", ("declaration", 12), "processedAfter"),
                ): 1
            }
        )
        return node, variables, expected

    def test_accepts_exact_commit_state_write(self) -> None:
        node, variables, expected = self.exact_commit_write()
        require_exact_state_writes(node, variables, expected, "commit")

    def test_rejects_commit_write_with_wrong_mapping_index(self) -> None:
        node, variables, expected = self.exact_commit_write()
        node["leftHandSide"]["indexExpression"]["memberName"] = "digest"
        with self.assertRaisesRegex(ValueError, "state writes differ"):
            require_exact_state_writes(node, variables, expected, "commit")

    def test_rejects_commit_compound_assignment(self) -> None:
        node, variables, expected = self.exact_commit_write()
        node["operator"] = "+="
        with self.assertRaisesRegex(ValueError, "state writes differ"):
            require_exact_state_writes(node, variables, expected, "commit")

    def test_rejects_commit_unary_increment(self) -> None:
        node, variables, expected = self.exact_commit_write()
        increment = {
            "nodeType": "UnaryOperation",
            "operator": "++",
            "subExpression": node["leftHandSide"]["baseExpression"],
        }
        with self.assertRaisesRegex(ValueError, "state writes differ"):
            require_exact_state_writes([node, increment], variables, expected, "commit")

    def test_rejects_commit_delete(self) -> None:
        node, variables, expected = self.exact_commit_write()
        delete = {
            "nodeType": "UnaryOperation",
            "operator": "delete",
            "subExpression": node["leftHandSide"],
        }
        with self.assertRaisesRegex(ValueError, "state writes differ"):
            require_exact_state_writes([node, delete], variables, expected, "commit")

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
