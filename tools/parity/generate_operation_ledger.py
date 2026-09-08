#!/usr/bin/env python3
"""Merge a runtime API inventory and shipped .pyi into a parity work ledger.

The ledger is a reviewed work queue, not a compatibility score. It deliberately
records runtime/stub disagreements rather than guessing which source is right.
Run it against a hash-pinned isolated oracle and write only a new destination.
"""

import argparse
import ast
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re


def stable_id(*parts):
    cleaned = [re.sub(r"[^A-Za-z0-9]+", "-", part).strip("-") for part in parts]
    return "py-" + ".".join(part.lower() for part in cleaned if part)


def decorator_name(decorator):
    if isinstance(decorator, ast.Name):
        return decorator.id
    if isinstance(decorator, ast.Attribute):
        return f"{ast.unparse(decorator.value)}.{decorator.attr}"
    return ast.unparse(decorator)


def function_signature(node):
    result = f"({ast.unparse(node.args)})"
    if node.returns is not None:
        result += f" -> {ast.unparse(node.returns)}"
    return result


def parse_stub(path):
    parsed = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    classes = defaultdict(lambda: defaultdict(list))
    module_members = defaultdict(list)

    def add(container, name, item):
        container[name].append(item)

    for node in parsed.body:
        if isinstance(node, ast.ClassDef):
            for member in node.body:
                if isinstance(member, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    decorators = [decorator_name(item) for item in member.decorator_list]
                    if "property" in decorators:
                        kind = "property_getter"
                    elif any(value.endswith(".setter") for value in decorators):
                        kind = "property_setter"
                    elif "staticmethod" in decorators:
                        kind = "static_method"
                    elif "classmethod" in decorators:
                        kind = "class_method"
                    else:
                        kind = "method"
                    add(classes[node.name], member.name, {
                        "kind": kind,
                        "signature": function_signature(member),
                        "decorators": decorators,
                        "line": member.lineno,
                    })
                elif isinstance(member, ast.AnnAssign) and isinstance(member.target, ast.Name):
                    add(classes[node.name], member.target.id, {
                        "kind": "annotated_constant",
                        "annotation": ast.unparse(member.annotation),
                        "line": member.lineno,
                    })
                elif isinstance(member, ast.Assign):
                    for target in member.targets:
                        if isinstance(target, ast.Name):
                            add(classes[node.name], target.id, {
                                "kind": "constant",
                                "value": ast.unparse(member.value),
                                "line": member.lineno,
                            })
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            decorators = [decorator_name(item) for item in node.decorator_list]
            add(module_members, node.name, {
                "kind": "module_function",
                "signature": function_signature(node),
                "decorators": decorators,
                "line": node.lineno,
            })
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            add(module_members, node.target.id, {
                "kind": "module_constant",
                "annotation": ast.unparse(node.annotation),
                "line": node.lineno,
            })
        elif isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name):
                    add(module_members, target.id, {
                        "kind": "module_constant",
                        "value": ast.unparse(node.value),
                        "line": node.lineno,
                    })
    return classes, module_members


def runtime_callable_overloads(runtime):
    """Return runtime overload variants extracted from pybind's docstring.

    pybind exposes one callable object for overloaded bindings. Its generated
    docstring is the only runtime inventory evidence that distinguishes the
    alternatives when the shipped `.pyi` is stale or omits the member.
    """
    if not runtime or runtime.get("kind") not in {"callable", "staticmethod", "classmethod"}:
        return [runtime] if runtime else []
    signatures = re.findall(r"(?m)^\d+\. ([^\n]+)$", runtime.get("doc", ""))
    if not signatures:
        return [runtime]
    return [
        {
            **runtime,
            "overload_index": index,
            "overload_signature": signature,
        }
        for index, signature in enumerate(signatures, 1)
    ]


def runtime_member_items(runtime, stub_items):
    """Make one obligation per runtime/stub overload or property direction."""
    result = []
    runtime_items = runtime_callable_overloads(runtime)
    ordinary = [item for item in stub_items if item["kind"] not in {"property_getter", "property_setter"}]
    properties = [item for item in stub_items if item["kind"] in {"property_getter", "property_setter"}]
    if runtime and runtime.get("kind") == "property":
        getter = next((item for item in properties if item["kind"] == "property_getter"), None)
        setter = next((item for item in properties if item["kind"] == "property_setter"), None)
        # Runtime mutability is authoritative for pybind properties. The
        # shipped stub often includes only a getter despite `writable: true`.
        # Synthesizing a setter obligation keeps that missing direction visible
        # rather than quietly reducing the parity denominator.
        result.append(("property_getter", runtime, getter))
        if runtime.get("writable") or setter:
            result.append(("property_setter", runtime if runtime.get("writable") else None, setter))
        return result
    callable_runtime = runtime and runtime.get("kind") in {"callable", "staticmethod", "classmethod"}
    if ordinary or callable_runtime:
        overload_count = max(len(runtime_items), len(ordinary))
        if overload_count > 1:
            for index in range(overload_count):
                result.append((
                    f"overload-{index + 1}",
                    runtime_items[index] if index < len(runtime_items) else None,
                    ordinary[index] if index < len(ordinary) else None,
                ))
        elif overload_count == 1:
            runtime_item = runtime_items[0] if runtime_items else None
            stub_item = ordinary[0] if ordinary else None
            suffix = stub_item["kind"] if stub_item else "runtime"
            result.append((suffix, runtime_item, stub_item))
    elif runtime and not properties:
        result.append(("runtime", runtime, None))
    for item in properties:
        result.append((item["kind"], runtime, item))
    return result


VALID_OVERRIDE_KEYS = {
    "status",
    "backend",
    "rust_mapping",
    "acceptance_case_ids",
    "notes",
}


def load_overrides(path):
    if path is None:
        return {}
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema") != "rhino3dm-rs.operation-overrides.v1":
        raise ValueError("unsupported override schema")
    operations = payload.get("operations")
    if not isinstance(operations, dict):
        raise ValueError("override operations must be an object keyed by operation id")
    for identifier, override in operations.items():
        if not isinstance(identifier, str) or not isinstance(override, dict):
            raise ValueError("each operation override must have a string id and object value")
        unknown = set(override) - VALID_OVERRIDE_KEYS
        if unknown:
            raise ValueError(f"unknown override fields for {identifier}: {sorted(unknown)}")
        if "acceptance_case_ids" in override and (
            not isinstance(override["acceptance_case_ids"], list)
            or not all(isinstance(case, str) and case for case in override["acceptance_case_ids"])
        ):
            raise ValueError(f"acceptance_case_ids for {identifier} must be non-empty strings")
    return operations


def apply_overrides(operations, overrides):
    known = {operation["id"]: operation for operation in operations}
    unknown = sorted(set(overrides) - set(known))
    if unknown:
        raise ValueError(f"overrides name unknown operation ids: {unknown}")
    for identifier, override in overrides.items():
        operation = known[identifier]
        operation.update(override)
        if operation["status"] == "passing" and not operation["acceptance_case_ids"]:
            raise ValueError(f"passing operation {identifier} needs an acceptance case")
        if operation["status"] == "passing" and operation["backend"] == "missing":
            raise ValueError(f"passing operation {identifier} needs a backend")


def add_source_pointers(operations):
    """Attach stable inventory/stub pointers without inventing bindings source."""
    for operation in operations:
        python_name = operation["python"]
        if python_name.startswith("rhino3dm."):
            member = python_name.removeprefix("rhino3dm.")
            inventory_pointer = f"#/module_exports/{member}"
        else:
            class_name, member = python_name.split(".", 1)
            inventory_pointer = (
                f"#/classes/{class_name}/declared_members_and_protocols/{member}"
            )
        stub = operation.get("stub")
        operation["source_pointers"] = {
            "inventory_json": inventory_pointer,
            "stub": (
                f"rhino3dm-8.32.1.pyi:{stub['line']}"
                if stub and "line" in stub
                else None
            ),
            "release_source": "oracle-lock.json#/primary/source_archive",
        }


def build_ledger(inventory, stub_path, oracle_lock, overrides):
    stub_classes, stub_module = parse_stub(stub_path)
    runtime_classes = inventory["classes"]
    operations = []
    exposures = []
    source_hash = hashlib.sha256(stub_path.read_bytes()).hexdigest()

    for class_name in sorted(set(runtime_classes) | set(stub_classes)):
        runtime = runtime_classes.get(class_name)
        runtime_members = (runtime or {}).get("declared_members_and_protocols", {})
        members = sorted(set(runtime_members) | set(stub_classes[class_name]))
        for member in members:
            runtime_item = runtime_members.get(member)
            stub_items = stub_classes[class_name].get(member, [])
            variants = runtime_member_items(runtime_item, stub_items)
            for suffix, runtime_variant, stub_item in variants:
                source_kind = "both" if runtime_variant and stub_item else "runtime_only" if runtime_variant else "stub_only"
                status = "not_assessed" if source_kind == "both" else "runtime_stub_divergence"
                operations.append({
                    "id": stable_id("8.32.1", class_name, member, suffix),
                    "python": f"{class_name}.{member}",
                    "variant": suffix,
                    "source_kind": source_kind,
                    "status": status,
                    "backend": "missing",
                    "rust_mapping": None,
                    "acceptance_case_ids": [],
                    "runtime": runtime_variant,
                    "stub": stub_item,
                })
        if runtime:
            for member, owner in sorted(runtime["public_member_owners"].items()):
                exposures.append({
                    "class": class_name,
                    "member": member,
                    "declared_by": owner,
                })

    runtime_module = inventory.get("module_exports", {})
    for member in sorted(set(runtime_module) | set(stub_module)):
        runtime_item = runtime_module.get(member)
        stub_items = stub_module.get(member, [])
        for suffix, runtime_variant, stub_item in runtime_member_items(runtime_item, stub_items):
            source_kind = "both" if runtime_variant and stub_item else "runtime_only" if runtime_variant else "stub_only"
            operations.append({
                "id": stable_id("8.32.1", "module", member, suffix),
                "python": f"rhino3dm.{member}",
                "variant": suffix,
                "source_kind": source_kind,
                "status": "not_assessed" if source_kind == "both" else "runtime_stub_divergence",
                "backend": "missing",
                "rust_mapping": None,
                "acceptance_case_ids": [],
                "runtime": runtime_variant,
                "stub": stub_item,
            })

    add_source_pointers(operations)
    apply_overrides(operations, overrides)
    source_kinds = defaultdict(int)
    statuses = defaultdict(int)
    for operation in operations:
        source_kinds[operation["source_kind"]] += 1
        statuses[operation["status"]] += 1
    return {
        "schema": "rhino3dm-rs.operation-ledger.v1",
        "target": {
            "distribution": inventory["distribution_version"],
            "runtime": inventory["runtime_version"],
            "inventory_sha256": hashlib.sha256(
                json.dumps(inventory, sort_keys=True, separators=(",", ":")).encode()
            ).hexdigest(),
            "stub_filename": stub_path.name,
            "stub_sha256": source_hash,
            "oracle_lock": str(oracle_lock),
        },
        "counting_rule": "One entry per shipped-stub overload/property direction or runtime-only member. Exposures record inherited runtime names separately. Every not_assessed or runtime_stub_divergence operation is incomplete.",
        "summary": {
            "operations": len(operations),
            "by_source_kind": dict(sorted(source_kinds.items())),
            "by_status": dict(sorted(statuses.items())),
            "runtime_classes": len(runtime_classes),
            "stub_classes": len(stub_classes),
            "public_inherited_exposures": len(exposures),
        },
        "operations": operations,
        "runtime_public_exposures": exposures,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--stub", type=Path, required=True)
    parser.add_argument("--oracle-lock", type=Path, required=True)
    parser.add_argument("--overrides", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    inventory = json.loads(args.inventory.read_text(encoding="utf-8"))
    lock = json.loads(args.oracle_lock.read_text(encoding="utf-8"))
    if inventory["distribution_version"] != lock["primary"]["distribution_version"]:
        parser.error("inventory distribution does not match primary oracle lock")
    if inventory["runtime_version"] != lock["primary"]["observed_runtime_version"]:
        parser.error("inventory runtime does not match primary oracle lock")
    expected_stub = lock["primary"].get("stub")
    if not isinstance(expected_stub, dict):
        parser.error("primary oracle lock must pin the shipped stub")
    if args.stub.name != expected_stub.get("filename"):
        parser.error("stub filename does not match primary oracle lock")
    actual_stub_hash = hashlib.sha256(args.stub.read_bytes()).hexdigest()
    if actual_stub_hash != expected_stub.get("sha256"):
        parser.error("stub SHA-256 does not match primary oracle lock")
    try:
        overrides = load_overrides(args.overrides)
        ledger = build_ledger(inventory, args.stub, args.oracle_lock, overrides)
    except ValueError as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as output:
        output.write(json.dumps(ledger, indent=2, sort_keys=True, ensure_ascii=False) + "\n")
    print(json.dumps({"output": str(args.output), **ledger["summary"]}, sort_keys=True))


if __name__ == "__main__":
    main()
