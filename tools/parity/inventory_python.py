#!/usr/bin/env python3
"""Inventory an installed rhino3dm oracle without constructing geometry.

Generated inventories are observations, not compatibility scores. Overload
resolution and behavior must still be checked against the pinned bindings and
executable fixtures. Python is a development oracle only.
"""

import argparse
import hashlib
import importlib.metadata
import inspect
import json
import platform
from pathlib import Path
import sys

import rhino3dm


PROTOCOLS = {
    "__init__", "__len__", "__getitem__", "__setitem__", "__delitem__",
    "__iter__", "__next__", "__contains__", "__bool__", "__eq__", "__ne__",
    "__lt__", "__le__", "__gt__", "__ge__", "__hash__", "__str__", "__repr__",
    "__add__", "__sub__", "__mul__", "__truediv__", "__neg__", "__rmul__",
    "__radd__", "__rsub__", "__iadd__", "__isub__", "__imul__", "__int__",
    "__float__", "__index__", "__copy__", "__deepcopy__",
}


def describe(value):
    if isinstance(value, property):
        return {"kind": "property", "readable": value.fget is not None,
                "writable": value.fset is not None}
    if isinstance(value, (staticmethod, classmethod)):
        return {"kind": type(value).__name__, "doc": value.__func__.__doc__ or ""}
    if inspect.isroutine(value) or callable(value):
        return {"kind": "callable", "doc": value.__doc__ or ""}
    if isinstance(value, (str, int, float, bool)) or value is None:
        return {"kind": "constant", "value": value}
    try:
        return {"kind": "enum_value", "integer_value": int(value)}
    except (TypeError, ValueError):
        return {"kind": "descriptor", "descriptor_type": type(value).__name__}


def member_owner(cls, member):
    """Return the first Python MRO class that declares an exposed member."""
    for owner in cls.__mro__:
        if member in vars(owner):
            return owner.__name__
    return None


def inventory():
    classes = {}
    module_exports = {}
    for name in sorted(dir(rhino3dm)):
        if name.startswith("_"):
            continue
        cls = getattr(rhino3dm, name)
        if not inspect.isclass(cls):
            module_exports[name] = describe(cls)
            continue
        public = sorted(member for member in dir(cls) if not member.startswith("_"))
        own = {
            member: describe(value)
            for member, value in sorted(vars(cls).items())
            if not member.startswith("_") or member in PROTOCOLS
        }
        classes[name] = {
            "bases": [base.__name__ for base in cls.__bases__],
            "is_enum": hasattr(cls, "__members__"),
            "public_members_including_inherited": public,
            "public_member_owners": {
                member: member_owner(cls, member) for member in public
            },
            "declared_members_and_protocols": own,
            "compatibility_status": "not_assessed_per_symbol",
        }
    distribution = importlib.metadata.distribution("rhino3dm")
    binary_hashes = {}
    for entry in distribution.files or []:
        if str(entry).endswith((".so", ".pyd", ".dylib")):
            binary_hashes[str(entry)] = hashlib.sha256(distribution.locate_file(entry).read_bytes()).hexdigest()
    return {
        "schema": "rhino3dm-rs.python-api-inventory.v1",
        "source": "McNeel rhino3dm (MIT), https://github.com/mcneel/rhino3dm",
        "distribution_version": distribution.version,
        "runtime_version": rhino3dm.__version__,
        "environment": {"python": platform.python_version(), "platform": platform.platform()},
        "binary_sha256": binary_hashes,
        "counting_rule": "Exported runtime classes include enums and BND iterator classes; member slots include inheritance; neither count measures implemented behavior.",
        "counts": {
            "exported_classes": len(classes),
            "enum_classes": sum(item["is_enum"] for item in classes.values()),
            "public_member_slots_including_inherited": sum(len(item["public_members_including_inherited"]) for item in classes.values()),
            "declared_members_and_protocols": sum(len(item["declared_members_and_protocols"]) for item in classes.values()),
        },
        "module_exports": module_exports,
        "classes": classes,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expect-distribution", required=True)
    args = parser.parse_args()
    actual = importlib.metadata.version("rhino3dm")
    if actual != args.expect_distribution:
        parser.error(f"expected distribution {args.expect_distribution}, found {actual}")
    result = inventory()
    # Creating a snapshot must never silently replace an earlier oracle.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x", encoding="utf-8") as output:
        output.write(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n")
    print(json.dumps({"output": str(args.output), "distribution": actual,
                      "runtime": result["runtime_version"], **result["counts"]}))


if __name__ == "__main__":
    main()
