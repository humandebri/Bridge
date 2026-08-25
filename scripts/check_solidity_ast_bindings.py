#!/usr/bin/env python3
"""Validate Solidity proof and wrapper bindings against compiler AST references."""

from __future__ import annotations

from dataclasses import dataclass
import argparse
import json
import re
from pathlib import Path
from typing import Iterator
from collections import Counter

from smt_obligations import SmtObligation, parse_smt_obligations


ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(
    r"(?P<path>[^#]+)#(?P<contract>[A-Za-z_][A-Za-z0-9_]*)\."
    r"(?P<function>[A-Za-z_][A-Za-z0-9_]*)\((?P<parameters>.*)\)"
)


def walk(node: object) -> Iterator[dict[str, object]]:
    if isinstance(node, dict):
        yield node
        for value in node.values():
            yield from walk(value)
    elif isinstance(node, list):
        for value in node:
            yield from walk(value)


def canonical_type(value: str) -> str:
    for prefix in ("struct ", "enum ", "contract "):
        if value.startswith(prefix):
            return value.removeprefix(prefix)
    return value


@dataclass(frozen=True)
class FunctionRecord:
    source: Path
    contract: str
    signature: str
    declaration_id: int
    node: dict[str, object]


class AstIndex:
    def __init__(
        self, artifact_root: Path, project_root: Path, link_root: Path = ROOT
    ) -> None:
        self.link_root = link_root.resolve()
        self.functions: list[FunctionRecord] = []
        self.contract_nodes: dict[tuple[Path, str], list[dict[str, object]]] = {}
        seen_contracts: set[tuple[Path, str, int]] = set()
        if not artifact_root.is_dir():
            raise ValueError(f"missing Solidity AST artifact root: {artifact_root}")
        for artifact in artifact_root.rglob("*.json"):
            try:
                ast = json.loads(artifact.read_text(encoding="utf-8")).get("ast")
            except (OSError, json.JSONDecodeError):
                continue
            if not isinstance(ast, dict) or not isinstance(ast.get("absolutePath"), str):
                continue
            raw_path = Path(ast["absolutePath"])
            source = (
                raw_path.resolve()
                if raw_path.is_absolute()
                else (project_root / raw_path).resolve()
            )
            for contract in ast.get("nodes", []):
                if not isinstance(contract, dict) or contract.get("nodeType") != "ContractDefinition":
                    continue
                contract_name = contract.get("name")
                contract_id = contract.get("id")
                if not isinstance(contract_name, str) or not isinstance(contract_id, int):
                    continue
                contract_key = (source, contract_name, contract_id)
                if contract_key in seen_contracts:
                    continue
                seen_contracts.add(contract_key)
                self.contract_nodes.setdefault((source, contract_name), []).append(contract)
                for node in contract.get("nodes", []):
                    if not isinstance(node, dict) or node.get("nodeType") != "FunctionDefinition":
                        continue
                    function_name = node.get("name")
                    declaration_id = node.get("id")
                    parameters = node.get("parameters", {}).get("parameters", [])
                    if not isinstance(function_name, str) or not isinstance(declaration_id, int):
                        continue
                    types = [
                        canonical_type(parameter["typeDescriptions"]["typeString"])
                        for parameter in parameters
                    ]
                    self.functions.append(
                        FunctionRecord(
                            source,
                            contract_name,
                            f"{function_name}({','.join(types)})",
                            declaration_id,
                            node,
                        )
                    )

    def resolve(self, link: str) -> tuple[FunctionRecord, ...]:
        match = LINK.fullmatch(link)
        if match is None:
            raise ValueError(f"invalid canonical Solidity function link: {link}")
        source = (self.link_root / match.group("path")).resolve()
        signature = f"{match.group('function')}({match.group('parameters')})"
        records = tuple(
            record
            for record in self.functions
            if record.source == source
            and record.contract == match.group("contract")
            and record.signature == signature
        )
        if not records:
            raise ValueError(f"unresolved Solidity AST function link: {link}")
        return records


def referenced_declarations(node: object) -> set[int]:
    return {
        value
        for item in walk(node)
        if isinstance((value := item.get("referencedDeclaration")), int)
        and value >= 0
    }


def function_calls(node: object) -> set[int]:
    calls: set[int] = set()
    for item in walk(node):
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        if not isinstance(expression, dict):
            continue
        declaration = expression.get("referencedDeclaration")
        if isinstance(declaration, int) and declaration >= 0:
            calls.add(declaration)
    return calls


def call_name_count(node: object, name: str) -> int:
    count = 0
    for item in walk(node):
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        if not isinstance(expression, dict):
            continue
        if expression.get("name") == name or expression.get("memberName") == name:
            count += 1
    return count


def emit_name_count(node: object, name: str) -> int:
    count = 0
    for item in walk(node):
        if item.get("nodeType") != "EmitStatement":
            continue
        event_call = item.get("eventCall")
        if not isinstance(event_call, dict):
            continue
        expression = event_call.get("expression")
        if isinstance(expression, dict) and (
            expression.get("name") == name or expression.get("memberName") == name
        ):
            count += 1
    return count


def validate_smt_call_graph(
    index: AstIndex, obligations: dict[str, SmtObligation] | None = None
) -> None:
    if obligations is None:
        obligations = parse_smt_obligations(
            (ROOT / "verification" / "smt" / "obligations.tsv").read_text(
                encoding="utf-8"
            )
        )
    for obligation in obligations.values():
        production_groups = [index.resolve(link) for link in obligation.production_links]
        production_ids = {
            record.declaration_id for group in production_groups for record in group
        }
        covered: set[int] = set()
        for link in obligation.pass_links:
            pass_records = index.resolve(link)
            if len(pass_records) != 1:
                raise ValueError(f"ambiguous SMT pass function AST: {link}")
            pass_record = pass_records[0]
            if call_name_count(pass_record.node.get("body"), "assert") == 0:
                raise ValueError(f"SMT pass function has no assertion: {link}")
            calls = function_calls(pass_record.node.get("body"))
            linked = calls & production_ids
            if not linked:
                raise ValueError(
                    f"SMT pass function does not call its production kernel: "
                    f"{obligation.obligation_id}/{link}"
                )
            covered.update(linked)
        for link, group in zip(obligation.production_links, production_groups, strict=True):
            if not ({record.declaration_id for record in group} & covered):
                raise ValueError(
                    f"SMT production kernel is not covered by a pass function: "
                    f"{obligation.obligation_id}/{link}"
                )


def _state_variables(record: FunctionRecord, index: AstIndex) -> dict[str, int]:
    contracts = index.contract_nodes.get((record.source, record.contract), [])
    if not contracts:
        raise ValueError(f"missing contract AST for wrapper: {record.contract}")
    variables: dict[str, int] = {}
    for contract in contracts:
        for node in contract.get("nodes", []):
            if (
                isinstance(node, dict)
                and node.get("nodeType") == "VariableDeclaration"
                and node.get("stateVariable") is True
                and isinstance(node.get("name"), str)
                and isinstance(node.get("id"), int)
            ):
                variables[node["name"]] = node["id"]
    return variables


def _assignment_counts(node: object, variable_ids: dict[str, int]) -> dict[str, int]:
    counts = {name: 0 for name in variable_ids}
    for item in walk(node):
        if item.get("nodeType") != "Assignment":
            continue
        left = item.get("leftHandSide")
        declarations = referenced_declarations(left)
        for name, declaration_id in variable_ids.items():
            if declaration_id in declarations:
                counts[name] += 1
    return counts


def expression_path(node: object) -> str:
    if not isinstance(node, dict):
        return "?"
    if node.get("nodeType") == "Identifier" and isinstance(node.get("name"), str):
        return node["name"]
    if node.get("nodeType") == "MemberAccess" and isinstance(node.get("memberName"), str):
        return f"{expression_path(node.get('expression'))}.{node['memberName']}"
    if node.get("nodeType") == "IndexAccess":
        return f"{expression_path(node.get('baseExpression'))}[{expression_path(node.get('indexExpression'))}]"
    return str(node.get("nodeType", "?"))


def assignment_values(node: object, variable_ids: dict[str, int]) -> dict[str, list[str]]:
    values = {name: [] for name in variable_ids}
    for item in walk(node):
        if item.get("nodeType") != "Assignment":
            continue
        declarations = referenced_declarations(item.get("leftHandSide"))
        for name, declaration_id in variable_ids.items():
            if declaration_id in declarations:
                values[name].append(expression_path(item.get("rightHandSide")))
    return values


def named_calls(node: object, name: str) -> list[dict[str, object]]:
    calls: list[dict[str, object]] = []
    for item in walk(node):
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        if isinstance(expression, dict) and (
            expression.get("name") == name or expression.get("memberName") == name
        ):
            calls.append(item)
    return calls


def direct_calls(node: object, declaration_id: int) -> list[dict[str, object]]:
    calls: list[dict[str, object]] = []
    for item in walk(node):
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        if (
            isinstance(expression, dict)
            and expression.get("referencedDeclaration") == declaration_id
        ):
            calls.append(item)
    return calls


def require_closed_call_set(node: object, expected: Counter[str], context: str) -> None:
    actual: Counter[str] = Counter()
    for item in walk(node):
        if item.get("nodeType") == "InlineAssembly":
            raise ValueError(f"{context} must not contain inline assembly")
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        if not isinstance(expression, dict):
            raise ValueError(f"{context} contains an unclassified call")
        if expression.get("nodeType") == "FunctionCallOptions":
            raise ValueError(f"{context} must not contain call options")
        member = expression.get("memberName")
        if member in {"call", "callcode", "delegatecall", "staticcall", "send", "transfer"}:
            raise ValueError(f"{context} must not contain low-level calls")
        actual[expression_path(expression)] += 1
    if actual != expected:
        raise ValueError(
            f"{context} call set differs: actual={dict(actual)} expected={dict(expected)}"
        )


def require_signature_guard(
    body: object,
    error_id: int,
    recovered_id: int,
    bridge_signer_id: int,
) -> None:
    guards = [item for item in walk(body) if item.get("nodeType") == "IfStatement"]
    if len(guards) != 1:
        raise ValueError("Bridge Mint wrapper must contain exactly one signature guard")
    guard = guards[0]
    condition = guard.get("condition")
    if not isinstance(condition, dict) or condition.get("nodeType") != "BinaryOperation" \
            or condition.get("operator") != "||":
        raise ValueError("Bridge Mint signature guard must be fail-closed OR")
    left = condition.get("leftExpression")
    right = condition.get("rightExpression")
    if not isinstance(left, dict) or not isinstance(right, dict):
        raise ValueError("Bridge Mint signature guard operands are missing")
    if (
        left.get("nodeType") != "BinaryOperation"
        or left.get("operator") != "!="
        or referenced_declarations(left.get("leftExpression")) != {error_id}
        or expression_path(left.get("rightExpression")) != "ECDSA.RecoverError.NoError"
        or right.get("nodeType") != "BinaryOperation"
        or right.get("operator") != "!="
        or referenced_declarations(right.get("leftExpression")) != {recovered_id}
        or referenced_declarations(right.get("rightExpression")) != {bridge_signer_id}
    ):
        raise ValueError("Bridge Mint signature guard binding differs")
    true_body = guard.get("trueBody")
    statements = true_body.get("statements") if isinstance(true_body, dict) else None
    if not isinstance(statements, list) or len(statements) != 1 \
            or statements[0].get("nodeType") != "RevertStatement":
        raise ValueError("Bridge Mint invalid signature branch must only revert")
    error_call = statements[0].get("errorCall")
    expression = error_call.get("expression") if isinstance(error_call, dict) else None
    if not isinstance(expression, dict) \
            or expression_path(expression) != "IBridge.InvalidMintAuthorizationSignature":
        raise ValueError("Bridge Mint signature guard must use the canonical error")


def require_digest_body(
    digest: FunctionRecord,
    authorization_id: int,
    typehash_id: int,
    hash_typed_data_id: int,
) -> None:
    body = digest.node.get("body")
    statements = body.get("statements") if isinstance(body, dict) else None
    if not isinstance(statements, list) or len(statements) != 1 \
            or statements[0].get("nodeType") != "Return":
        raise ValueError("Bridge Mint digest must be a single closed return")
    outer = statements[0].get("expression")
    if not isinstance(outer, dict) or outer.get("nodeType") != "FunctionCall" \
            or outer.get("expression", {}).get("referencedDeclaration") != hash_typed_data_id:
        raise ValueError("Bridge Mint digest must use the EIP-712 domain hash")
    outer_args = outer.get("arguments")
    inner = outer_args[0] if isinstance(outer_args, list) and len(outer_args) == 1 else None
    if not isinstance(inner, dict) or expression_path(inner.get("expression")) != "keccak256":
        raise ValueError("Bridge Mint digest must keccak the encoded struct")
    inner_args = inner.get("arguments")
    encoded = inner_args[0] if isinstance(inner_args, list) and len(inner_args) == 1 else None
    if not isinstance(encoded, dict) or expression_path(encoded.get("expression")) != "abi.encode":
        raise ValueError("Bridge Mint digest must use abi.encode")
    auth = ("declaration", authorization_id)
    expected = (
        ("declaration", typehash_id),
        ("member", auth, "depositId"),
        ("member", auth, "recipient"),
        ("member", auth, "grossAmount"),
        ("member", auth, "maxServiceFee"),
        ("member", auth, "chargedServiceFee"),
        ("member", auth, "deadline"),
        ("member", auth, "authorizationEpoch"),
    )
    arguments = encoded.get("arguments")
    actual = tuple(expression_declaration_binding(value) for value in arguments) \
        if isinstance(arguments, list) else ()
    if actual != expected:
        raise ValueError(f"Bridge Mint digest fields differ: {actual}")
    require_closed_call_set(
        body,
        Counter({"_hashTypedDataV4": 1, "keccak256": 1, "abi.encode": 1}),
        "Bridge Mint digest",
    )


def require_all_reject_reasons_fail_closed(record: FunctionRecord) -> None:
    body = record.node.get("body")
    reason_id = named_declaration_id(record.node.get("parameters", {}), "reason")
    expected = {
        "None",
        "Paused",
        "Expired",
        "DeadlineTooFar",
        "EpochMismatch",
        "ZeroRecipient",
        "InvalidRecipient",
        "GrossExceedsU128",
        "MaximumFeeExceedsU128",
        "ChargedFeeExceedsU128",
        "Processed",
        "ProtocolFeeExceeded",
        "UserFeeExceeded",
        "InvalidAmount",
        "PerDepositLimitExceeded",
        "WindowLimitExceeded",
        "TimestampExceedsU64",
    }
    observed: set[str] = set()
    for guard in [item for item in walk(body) if item.get("nodeType") == "IfStatement"]:
        condition = guard.get("condition")
        if not isinstance(condition, dict) or condition.get("nodeType") != "BinaryOperation" \
                or condition.get("operator") != "==" \
                or referenced_declarations(condition.get("leftExpression")) != {reason_id}:
            raise ValueError("Bridge Mint reject guard binding differs")
        path = expression_path(condition.get("rightExpression"))
        prefix = "MintAuthorizationPolicy.RejectReason."
        if not path.startswith(prefix):
            raise ValueError("Bridge Mint reject guard is not bound to RejectReason")
        variant = path.removeprefix(prefix)
        true_body = guard.get("trueBody")
        statements = (
            true_body.get("statements")
            if isinstance(true_body, dict) and true_body.get("nodeType") == "Block"
            else [true_body]
        )
        node_types = [item.get("nodeType") for item in statements if isinstance(item, dict)] \
            if isinstance(statements, list) else []
        expected_body = ["Return"] if variant == "None" else ["RevertStatement"]
        if node_types != expected_body:
            raise ValueError(f"Bridge Mint reject reason does not fail closed: {variant}")
        if variant in observed:
            raise ValueError(f"duplicate Bridge Mint reject reason: {variant}")
        observed.add(variant)
    if observed != expected:
        raise ValueError(
            f"Bridge Mint RejectReason coverage differs: missing={sorted(expected-observed)} "
            f"extra={sorted(observed-expected)}"
        )
    statements = body.get("statements") if isinstance(body, dict) else None
    if not isinstance(statements, list) or statements[-1].get("nodeType") != "RevertStatement":
        raise ValueError("Bridge Mint unknown reject reason must fail closed")


def named_declaration_id(node: object, name: str) -> int:
    declarations = {
        item["id"]
        for item in walk(node)
        if item.get("nodeType") == "VariableDeclaration"
        and item.get("name") == name
        and isinstance(item.get("id"), int)
    }
    if len(declarations) != 1:
        raise ValueError(
            f"Solidity declaration must resolve exactly once: {name}/{sorted(declarations)}"
        )
    return declarations.pop()


def require_call_argument_declarations(
    call: dict[str, object], expected: tuple[int, ...]
) -> None:
    arguments = call.get("arguments")
    if not isinstance(arguments, list):
        raise ValueError("Solidity call arguments are missing")
    actual: list[int | None] = []
    for argument in arguments:
        if not isinstance(argument, dict) or argument.get("nodeType") != "Identifier":
            actual.append(None)
            continue
        declaration = argument.get("referencedDeclaration")
        actual.append(declaration if isinstance(declaration, int) else None)
    if tuple(actual) != expected:
        raise ValueError(
            f"Solidity call argument declarations differ: "
            f"actual={actual} expected={list(expected)}"
        )


def declaration_initializer_call(
    node: object, declaration_id: int, callee_id: int
) -> dict[str, object]:
    statements = [
        item
        for item in walk(node)
        if item.get("nodeType") == "VariableDeclarationStatement"
        and any(
            isinstance(declaration, dict) and declaration.get("id") == declaration_id
            for declaration in item.get("declarations", [])
        )
    ]
    if len(statements) != 1:
        raise ValueError(
            f"Solidity declaration statement must resolve exactly once: "
            f"{declaration_id}/{len(statements)}"
        )
    initializer = statements[0].get("initialValue")
    expression = initializer.get("expression") if isinstance(initializer, dict) else None
    if (
        not isinstance(initializer, dict)
        or initializer.get("nodeType") != "FunctionCall"
        or not isinstance(expression, dict)
        or expression.get("referencedDeclaration") != callee_id
    ):
        raise ValueError(
            f"Solidity declaration initializer differs: {declaration_id}/{callee_id}"
        )
    return initializer


def library_calls(node: object, library: str, member: str) -> list[dict[str, object]]:
    calls: list[dict[str, object]] = []
    expected_type = f"type(library {library})"
    for item in walk(node):
        if item.get("nodeType") != "FunctionCall":
            continue
        expression = item.get("expression")
        base = expression.get("expression") if isinstance(expression, dict) else None
        base_type = base.get("typeDescriptions", {}) if isinstance(base, dict) else {}
        if (
            isinstance(expression, dict)
            and expression.get("nodeType") == "MemberAccess"
            and expression.get("memberName") == member
            and isinstance(base_type, dict)
            and base_type.get("typeString") == expected_type
        ):
            calls.append(item)
    return calls


def declaration_initializer_library_call(
    node: object, declaration_id: int, library: str, member: str
) -> dict[str, object]:
    statements = [
        item
        for item in walk(node)
        if item.get("nodeType") == "VariableDeclarationStatement"
        and any(
            isinstance(declaration, dict) and declaration.get("id") == declaration_id
            for declaration in item.get("declarations", [])
        )
    ]
    if len(statements) != 1:
        raise ValueError(
            f"Solidity declaration statement must resolve exactly once: "
            f"{declaration_id}/{len(statements)}"
        )
    initializer = statements[0].get("initialValue")
    if not isinstance(initializer, dict) or initializer not in library_calls(
        initializer, library, member
    ):
        raise ValueError(
            f"Solidity declaration initializer is not compiler-bound to "
            f"{library}.{member}: {declaration_id}"
        )
    return initializer


def require_no_declaration_reassignment(node: object, declaration_ids: set[int]) -> None:
    reassigned: set[int] = set()
    for item in walk(node):
        if item.get("nodeType") == "Assignment":
            target = item.get("leftHandSide")
        elif item.get("nodeType") == "UnaryOperation" and item.get("operator") in {
            "++",
            "--",
            "delete",
        }:
            target = item.get("subExpression")
        else:
            continue
        reassigned.update(referenced_declarations(target) & declaration_ids)
    if reassigned:
        raise ValueError(
            f"Bridge Mint wrapper reassigns bound declarations: {sorted(reassigned)}"
        )


def expression_declaration_binding(node: object) -> tuple[object, ...]:
    if not isinstance(node, dict):
        return ("invalid",)
    node_type = node.get("nodeType")
    if node_type == "Identifier":
        declaration = node.get("referencedDeclaration")
        if isinstance(declaration, int) and declaration >= 0:
            return ("declaration", declaration)
        return ("magic", node.get("name"))
    if node_type == "MemberAccess":
        return (
            "member",
            expression_declaration_binding(node.get("expression")),
            node.get("memberName"),
        )
    if node_type == "IndexAccess":
        return (
            "index",
            expression_declaration_binding(node.get("baseExpression")),
            expression_declaration_binding(node.get("indexExpression")),
        )
    if node_type == "FunctionCall" and node.get("kind") == "typeConversion":
        expression = node.get("expression")
        type_name = expression.get("typeName") if isinstance(expression, dict) else None
        arguments = node.get("arguments")
        if (
            isinstance(type_name, dict)
            and type_name.get("name") == "address"
            and isinstance(arguments, list)
            and len(arguments) == 1
        ):
            return ("address", expression_declaration_binding(arguments[0]))
    return ("unsupported", node_type)


def require_evaluate_input_binding(
    call: dict[str, object], authorization_id: int, variables: dict[str, int]
) -> None:
    arguments = call.get("arguments")
    if not isinstance(arguments, list) or len(arguments) != 1:
        raise ValueError("Bridge Mint evaluateMint must receive one transition input")
    transition = arguments[0]
    if (
        not isinstance(transition, dict)
        or transition.get("nodeType") != "FunctionCall"
        or transition.get("kind") != "structConstructorCall"
    ):
        raise ValueError("Bridge Mint evaluateMint input is not the transition struct")
    names = (
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
    )
    auth = ("declaration", authorization_id)
    expected = (
        ("member", ("magic", "block"), "timestamp"),
        ("member", auth, "deadline"),
        ("member", auth, "authorizationEpoch"),
        ("declaration", variables["mintAuthorizationEpoch"]),
        ("member", auth, "recipient"),
        ("address", ("magic", "this")),
        ("address", ("declaration", variables["bsns"])),
        ("member", auth, "grossAmount"),
        ("member", auth, "maxServiceFee"),
        ("member", auth, "chargedServiceFee"),
        ("declaration", variables["MAX_SERVICE_FEE"]),
        ("declaration", variables["perDepositLimit"]),
        ("declaration", variables["mintedInWindow"]),
        ("declaration", variables["mintWindowLimit"]),
        ("declaration", variables["mintWindowStartedAt"]),
        ("declaration", variables["mintWindowDuration"]),
        ("declaration", variables["depositMintsPaused"]),
        (
            "index",
            ("declaration", variables["_processedDeposits"]),
            ("member", auth, "depositId"),
        ),
    )
    actual_arguments = transition.get("arguments")
    actual = (
        tuple(expression_declaration_binding(argument) for argument in actual_arguments)
        if isinstance(actual_arguments, list)
        else ()
    )
    if tuple(transition.get("names", [])) != names or actual != expected:
        raise ValueError(
            f"Bridge Mint evaluateMint input binding differs: "
            f"names={transition.get('names')} actual={actual}"
        )


def named_emits(node: object, name: str) -> list[dict[str, object]]:
    emits: list[dict[str, object]] = []
    for item in walk(node):
        if item.get("nodeType") != "EmitStatement":
            continue
        call = item.get("eventCall")
        expression = call.get("expression") if isinstance(call, dict) else None
        if isinstance(expression, dict) and (
            expression.get("name") == name or expression.get("memberName") == name
        ):
            emits.append(call)
    return emits


def validate_bridge_commit(index: AstIndex) -> None:
    wrapper_link = (
        "contracts/src/Bridge.sol#Bridge.mintDepositWithAuthorization("
        "IBridge.MintAuthorization,bytes)"
    )
    commit_link = (
        "contracts/src/Bridge.sol#Bridge._commitAuthorizedMint("
        "IBridge.MintAuthorization,bytes32,MintAuthorizationPolicy.MintEffects)"
    )
    digest_link = (
        "contracts/src/Bridge.sol#Bridge._mintAuthorizationDigest("
        "IBridge.MintAuthorization)"
    )
    evaluate_link = (
        "contracts/src/libraries/MintAuthorizationPolicy.sol#"
        "MintAuthorizationPolicy.evaluateMint("
        "MintAuthorizationPolicy.MintTransitionInput)"
    )
    recover_link = (
        "contracts/lib/openzeppelin-contracts/contracts/utils/cryptography/"
        "ECDSA.sol#ECDSA.tryRecoverCalldata(bytes32,bytes)"
    )
    reject_link = (
        "contracts/src/Bridge.sol#Bridge._revertRejectedMint("
        "MintAuthorizationPolicy.RejectReason,IBridge.MintAuthorization,uint256)"
    )
    hash_typed_data_link = (
        "contracts/lib/openzeppelin-contracts/contracts/utils/cryptography/"
        "EIP712.sol#EIP712._hashTypedDataV4(bytes32)"
    )
    wrapper_records = index.resolve(wrapper_link)
    commit_records = index.resolve(commit_link)
    digest_records = index.resolve(digest_link)
    evaluate_records = index.resolve(evaluate_link)
    recover_records = index.resolve(recover_link)
    reject_records = index.resolve(reject_link)
    hash_typed_data_records = index.resolve(hash_typed_data_link)
    if any(
        len(records) != 1
        for records in (
            wrapper_records,
            commit_records,
            digest_records,
            evaluate_records,
            recover_records,
            reject_records,
            hash_typed_data_records,
        )
    ):
        raise ValueError("Bridge Mint wrapper AST must resolve exactly once")
    wrapper = wrapper_records[0]
    commit = commit_records[0]
    digest = digest_records[0]
    reject = reject_records[0]
    variables = _state_variables(wrapper, index)
    wrapper_statements = wrapper.node.get("body", {}).get("statements", [])
    if [statement.get("nodeType") for statement in wrapper_statements] != [
        "VariableDeclarationStatement",
        "VariableDeclarationStatement",
        "IfStatement",
        "VariableDeclarationStatement",
        "ExpressionStatement",
        "ExpressionStatement",
    ]:
        raise ValueError("Bridge Mint wrapper statement sequence differs")
    commit_calls = direct_calls(wrapper.node.get("body"), commit.declaration_id)
    if len(commit_calls) != 1:
        raise ValueError("Bridge Mint wrapper must call the commit boundary exactly once")
    wrapper_parameters = wrapper.node.get("parameters", {}).get("parameters", [])
    authorization_id = named_declaration_id(wrapper_parameters, "authorization")
    signature_id = named_declaration_id(wrapper_parameters, "signature")
    digest_id = named_declaration_id(wrapper.node.get("body"), "digest")
    effects_id = named_declaration_id(wrapper.node.get("body"), "effects")
    recovered_id = named_declaration_id(wrapper.node.get("body"), "recovered")
    error_id = named_declaration_id(wrapper.node.get("body"), "error")
    digest_initializer = declaration_initializer_call(
        wrapper.node.get("body"), digest_id, digest_records[0].declaration_id
    )
    require_call_argument_declarations(digest_initializer, (authorization_id,))
    digest_calls = direct_calls(wrapper.node.get("body"), digest_records[0].declaration_id)
    if len(digest_calls) != 1 or digest_calls[0].get("id") != digest_initializer.get("id"):
        raise ValueError(
            "Bridge Mint digest must come from the unique authorization digest call"
        )
    evaluate_initializer = declaration_initializer_library_call(
        wrapper.node.get("body"), effects_id, "MintAuthorizationPolicy", "evaluateMint"
    )
    evaluate_calls = library_calls(
        wrapper.node.get("body"), "MintAuthorizationPolicy", "evaluateMint"
    )
    if (
        len(evaluate_calls) != 1
        or evaluate_calls[0].get("id") != evaluate_initializer.get("id")
    ):
        raise ValueError("Bridge Mint effects must come from the unique evaluateMint call")
    require_evaluate_input_binding(
        evaluate_initializer, authorization_id, variables
    )
    recover_calls = library_calls(wrapper.node.get("body"), "ECDSA", "tryRecoverCalldata")
    if len(recover_calls) != 1:
        raise ValueError("Bridge Mint wrapper must recover exactly one signature")
    require_call_argument_declarations(
        recover_calls[0], (digest_id, signature_id)
    )
    recovery_statements = [
        item
        for item in wrapper_statements
        if item.get("nodeType") == "VariableDeclarationStatement"
        and {declaration.get("id") for declaration in item.get("declarations", []) if declaration}
        >= {recovered_id, error_id}
    ]
    if len(recovery_statements) != 1 \
            or recovery_statements[0].get("initialValue", {}).get("id") != recover_calls[0].get("id"):
        raise ValueError("Bridge Mint recovered signer declarations are not bound to recovery")
    require_signature_guard(
        wrapper.node.get("body"),
        error_id,
        recovered_id,
        variables["bridgeSigner"],
    )
    require_no_declaration_reassignment(
        wrapper.node.get("body"), {digest_id, effects_id, recovered_id, error_id, *variables.values()}
    )
    require_call_argument_declarations(
        commit_calls[0], (authorization_id, digest_id, effects_id)
    )
    reject_calls = direct_calls(wrapper.node.get("body"), reject.declaration_id)
    if len(reject_calls) != 1:
        raise ValueError("Bridge Mint wrapper must call the reject boundary exactly once")
    reason_id = named_declaration_id(wrapper.node.get("body"), "reason")
    window_available_id = named_declaration_id(wrapper.node.get("body"), "windowAvailable")
    require_call_argument_declarations(
        reject_calls[0], (reason_id, authorization_id, window_available_id)
    )
    require_closed_call_set(
        wrapper.node.get("body"),
        Counter({
            "_mintAuthorizationDigest": 1,
            "ECDSA.tryRecoverCalldata": 1,
            "IBridge.InvalidMintAuthorizationSignature": 1,
            "MintAuthorizationPolicy.evaluateMint": 1,
            "MintAuthorizationPolicy.MintTransitionInput": 1,
            "ElementaryTypeNameExpression": 2,
            "_revertRejectedMint": 1,
            "_commitAuthorizedMint": 1,
        }),
        "Bridge Mint wrapper",
    )
    selected = {
        name: variables[name]
        for name in ("_processedDeposits", "mintWindowStartedAt", "mintedInWindow")
    }
    if any(_assignment_counts(wrapper.node.get("body"), selected).values()):
        raise ValueError("Bridge Mint wrapper writes commit state outside the commit boundary")
    commit_assignments = _assignment_counts(commit.node.get("body"), selected)
    if commit_assignments != {name: 1 for name in selected}:
        raise ValueError(f"Bridge Mint commit state assignments differ: {commit_assignments}")
    expected_assignments = {
        "_processedDeposits": ["effects.processedAfter"],
        "mintWindowStartedAt": ["effects.windowStartedAtAfter"],
        "mintedInWindow": ["effects.windowConsumedAfter"],
    }
    actual_assignments = assignment_values(commit.node.get("body"), selected)
    if actual_assignments != expected_assignments:
        raise ValueError(f"Bridge Mint commit assignment values differ: {actual_assignments}")
    all_commit_assignments = _assignment_counts(commit.node.get("body"), variables)
    if {name: count for name, count in all_commit_assignments.items() if count} \
            != {name: 1 for name in selected}:
        raise ValueError(f"Bridge Mint commit has unlisted state writes: {all_commit_assignments}")
    require_no_declaration_reassignment(
        commit.node.get("body"), set(variables.values()) - set(selected.values())
    )
    require_closed_call_set(
        commit.node.get("body"),
        Counter({"bsns.bridgeMint": 1, "IBridge.DepositMinted": 1}),
        "Bridge Mint commit",
    )
    if call_name_count(wrapper.node.get("body"), "bridgeMint") != 0:
        raise ValueError("Bridge Mint wrapper calls bridgeMint outside the commit boundary")
    if call_name_count(commit.node.get("body"), "bridgeMint") != 1:
        raise ValueError("Bridge Mint commit must call bridgeMint exactly once")
    bridge_mint = named_calls(commit.node.get("body"), "bridgeMint")[0]
    bridge_mint_arguments = [expression_path(value) for value in bridge_mint.get("arguments", [])]
    if bridge_mint_arguments != ["authorization.recipient", "effects.supplyIncrease"]:
        raise ValueError(f"Bridge Mint token arguments differ: {bridge_mint_arguments}")
    if emit_name_count(wrapper.node.get("body"), "DepositMinted") != 0:
        raise ValueError("Bridge Mint wrapper emits DepositMinted outside the commit boundary")
    if emit_name_count(commit.node.get("body"), "DepositMinted") != 1:
        raise ValueError("Bridge Mint commit must emit DepositMinted exactly once")
    deposit_minted = named_emits(commit.node.get("body"), "DepositMinted")[0]
    event_arguments = [expression_path(value) for value in deposit_minted.get("arguments", [])]
    expected_event_arguments = [
        "authorization.depositId",
        "authorization.recipient",
        "digest",
        "effects.eventGrossAmount",
        "effects.eventServiceFee",
        "effects.eventMintedAmount",
    ]
    if event_arguments != expected_event_arguments:
        raise ValueError(f"Bridge Mint event arguments differ: {event_arguments}")
    digest_authorization_id = named_declaration_id(
        digest.node.get("parameters", {}), "authorization"
    )
    require_digest_body(
        digest,
        digest_authorization_id,
        variables["MINT_AUTHORIZATION_TYPEHASH"],
        hash_typed_data_records[0].declaration_id,
    )
    require_all_reject_reasons_fail_closed(reject)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scope", choices=("all", "smt", "bridge"), default="all")
    args = parser.parse_args()
    if args.scope in {"all", "smt"}:
        smt_index = AstIndex(
            ROOT / "verification" / "smt" / "out", ROOT / "verification" / "smt"
        )
        validate_smt_call_graph(smt_index)
    if args.scope in {"all", "bridge"}:
        contract_index = AstIndex(ROOT / "contracts" / "out", ROOT / "contracts")
        validate_bridge_commit(contract_index)
    print("Solidity AST bindings passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
