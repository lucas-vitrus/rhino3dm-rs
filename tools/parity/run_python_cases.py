#!/usr/bin/env python3
"""Execute the initial JSON conformance cases against a pinned rhino3dm wheel.

This runner intentionally supports only the operations represented by the
checked-in P00/P02 cases. Adding a case means adding an equivalent Rust runner
operation and recording it in the operation ledger; it is not a generic Python
evaluation escape hatch.
"""

import argparse
import importlib.metadata
import json
from pathlib import Path

import rhino3dm


def number(value, context):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{context} must be a number")
    if value != value or value in (float("inf"), float("-inf")):
        raise ValueError(f"{context} must be finite")
    return float(value)


def resolve(value, values):
    if isinstance(value, dict) and set(value) == {"ref"}:
        reference = value["ref"]
        if reference not in values:
            raise ValueError(f"unknown reference {reference!r}")
        return values[reference]
    return value


def observed(value):
    if isinstance(value, rhino3dm.Point2d):
        return {"type": "Point2d", "x": value.X, "y": value.Y}
    if isinstance(value, rhino3dm.Point3d):
        return {"type": "Point3d", "x": value.X, "y": value.Y, "z": value.Z}
    if isinstance(value, rhino3dm.Point3f):
        return {"type": "Point3f", "x": value.X, "y": value.Y, "z": value.Z}
    if isinstance(value, rhino3dm.Point4d):
        return {"type": "Point4d", "x": value.X, "y": value.Y, "z": value.Z, "w": value.W}
    if isinstance(value, rhino3dm.Vector2d):
        return {"type": "Vector2d", "x": value.X, "y": value.Y}
    if isinstance(value, rhino3dm.Vector3d):
        return {"type": "Vector3d", "x": value.X, "y": value.Y, "z": value.Z}
    if isinstance(value, rhino3dm.Vector3f):
        return {"type": "Vector3f", "x": value.X, "y": value.Y, "z": value.Z}
    if isinstance(value, rhino3dm.Interval):
        return {"type": "Interval", "t0": value.T0, "t1": value.T1}
    if isinstance(value, rhino3dm.Line):
        return {
            "type": "Line",
            "from": {"x": value.From.X, "y": value.From.Y, "z": value.From.Z},
            "to": {"x": value.To.X, "y": value.To.Y, "z": value.To.Z},
        }
    if isinstance(value, rhino3dm.Transform):
        return {
            "type": "Transform",
            "matrix": [encoded_float(item) for item in value.ToFloatArray(True)],
            "is_identity": value.IsIdentity,
            "is_affine": value.IsAffine,
            "is_valid": value.IsValid,
            "is_zero": value.IsZero,
            "is_zero_4x4": value.IsZero4x4,
            "is_zero_transformation": value.IsZeroTransformation,
            "is_linear": value.IsLinear,
            "is_rotation": value.IsRotation,
            "determinant": value.Determinant(),
        }
    if isinstance(value, tuple):
        return {"type": "FloatArray", "values": [encoded_float(item) for item in value]}
    if isinstance(value, dict):
        return {"type": "Coordinates", "value": value}
    if value is None:
        return {"type": "None"}
    if isinstance(value, bool):
        return {"type": "Boolean", "value": value}
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return {"type": "Number", "value": value}
    raise ValueError(f"unsupported observed value {type(value).__name__}")


def encoded_float(value):
    if value == value and value not in (float("inf"), float("-inf")):
        return value
    return {
        "tag": "float",
        "value": "nan" if value != value else "negative_infinity" if value < 0 else "positive_infinity",
    }


def values3(args, values, call):
    if len(args) != 3:
        raise ValueError(f"{call} needs exactly three arguments")
    return [number(resolve(item, values), f"{call} argument {index}") for index, item in enumerate(args)]


def values2(args, values, call):
    if len(args) != 2:
        raise ValueError(f"{call} needs exactly two arguments")
    return [number(resolve(item, values), f"{call} argument {index}") for index, item in enumerate(args)]


def execute(case):
    if case.get("schema") != "rhino3dm-rs.conformance-case.v1":
        raise ValueError("unsupported case schema")
    values = {}
    for operation in case.get("operations", []):
        identifier = operation.get("id")
        call = operation.get("call")
        if not isinstance(identifier, str) or not identifier:
            raise ValueError("operation id must be a non-empty string")
        if identifier in values:
            raise ValueError(f"duplicate operation id {identifier!r}")
        args = operation.get("args", [])
        receiver = values.get(operation.get("receiver")) if operation.get("receiver") else None
        if operation.get("receiver") and receiver is None:
            raise ValueError(f"unknown receiver {operation['receiver']!r}")

        if call == "point3d.new":
            values[identifier] = rhino3dm.Point3d(*values3(args, values, call))
        elif call == "point3f.new":
            values[identifier] = rhino3dm.Point3f(*values3(args, values, call))
        elif call == "point4d.new":
            if len(args) != 4:
                raise ValueError("point4d.new needs exactly four arguments")
            values[identifier] = rhino3dm.Point4d(*[number(resolve(item, values), call) for item in args])
        elif call == "point2d.new":
            values[identifier] = rhino3dm.Point2d(*values2(args, values, call))
        elif call == "point3d.unset":
            if args:
                raise ValueError("point3d.unset has no arguments")
            values[identifier] = rhino3dm.Point3d.Unset
        elif call == "vector3d.new":
            values[identifier] = rhino3dm.Vector3d(*values3(args, values, call))
        elif call == "vector3f.new":
            values[identifier] = rhino3dm.Vector3f(*values3(args, values, call))
        elif call == "vector2d.new":
            values[identifier] = rhino3dm.Vector2d(*values2(args, values, call))
        elif call == "interval.new":
            values[identifier] = rhino3dm.Interval(*values2(args, values, call))
        elif call == "line.new":
            if len(args) != 2:
                raise ValueError("line.new needs two Point3d arguments")
            start, end = (resolve(item, values) for item in args)
            if not isinstance(start, rhino3dm.Point3d) or not isinstance(end, rhino3dm.Point3d):
                raise ValueError("line.new needs two Point3d arguments")
            values[identifier] = rhino3dm.Line(start, end)
        elif call == "transform.identity":
            if args:
                raise ValueError("transform.identity has no arguments")
            values[identifier] = rhino3dm.Transform.Identity()
        elif call == "transform.zero_transformation":
            if args:
                raise ValueError("transform.zero_transformation has no arguments")
            values[identifier] = rhino3dm.Transform.ZeroTransformation()
        elif call == "transform.unset":
            if args:
                raise ValueError("transform.unset has no arguments")
            values[identifier] = rhino3dm.Transform.Unset()
        elif call == "transform.diagonal":
            if len(args) != 1:
                raise ValueError("transform.diagonal needs one argument")
            values[identifier] = rhino3dm.Transform(number(resolve(args[0], values), call))
        elif call == "transform.translation":
            if len(args) == 1:
                vector = resolve(args[0], values)
                if not isinstance(vector, rhino3dm.Vector3d):
                    raise ValueError("transform.translation single argument must be Vector3d")
                values[identifier] = rhino3dm.Transform.Translation(vector)
            else:
                values[identifier] = rhino3dm.Transform.Translation(*values3(args, values, call))
        elif call == "transform.rotation_axis_angle":
            if len(args) != 3:
                raise ValueError("transform.rotation_axis_angle needs angle, Vector3d axis and Point3d center")
            angle = number(resolve(args[0], values), call)
            axis = resolve(args[1], values)
            center = resolve(args[2], values)
            if not isinstance(axis, rhino3dm.Vector3d) or not isinstance(center, rhino3dm.Point3d):
                raise ValueError("transform.rotation_axis_angle needs Vector3d axis and Point3d center")
            values[identifier] = rhino3dm.Transform.Rotation(angle, axis, center)
        elif call == "transform.multiply":
            if len(args) != 2:
                raise ValueError("transform.multiply needs two transforms")
            left, right = (resolve(item, values) for item in args)
            if not isinstance(left, rhino3dm.Transform) or not isinstance(right, rhino3dm.Transform):
                raise ValueError("transform.multiply arguments must be Transform")
            values[identifier] = rhino3dm.Transform.Multiply(left, right)
        elif call == "transform.try_get_inverse":
            if not isinstance(receiver, rhino3dm.Transform):
                raise ValueError("transform.try_get_inverse receiver must be Transform")
            values[identifier] = receiver.TryGetInverse()
        elif call == "transform.transpose":
            if args or not isinstance(receiver, rhino3dm.Transform):
                raise ValueError("transform.transpose requires a Transform receiver and no arguments")
            values[identifier] = receiver.Transpose()
        elif call == "transform.to_float_array":
            if len(args) != 1 or not isinstance(args[0], bool) or not isinstance(receiver, rhino3dm.Transform):
                raise ValueError("transform.to_float_array requires a Transform receiver and one boolean argument")
            values[identifier] = receiver.ToFloatArray(args[0])
        elif call == "point3d.distance_to":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(other, rhino3dm.Point3d):
                raise ValueError("point3d.distance_to requires Point3d receiver and argument")
            values[identifier] = receiver.DistanceTo(other)
        elif call == "point2d.distance_to":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point2d) or not isinstance(other, rhino3dm.Point2d):
                raise ValueError("point2d.distance_to requires Point2d receiver and argument")
            values[identifier] = receiver.DistanceTo(other)
        elif call == "point3f.add_point":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3f) or not isinstance(other, rhino3dm.Point3f):
                raise ValueError("point3f.add_point requires Point3f receiver and argument")
            values[identifier] = receiver + other
        elif call == "point3f.encode":
            if args or not isinstance(receiver, rhino3dm.Point3f):
                raise ValueError("point3f.encode requires a Point3f receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "point3f.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3f) or not isinstance(other, rhino3dm.Point3f):
                raise ValueError("point3f.equals requires Point3f receiver and argument")
            values[identifier] = receiver == other
        elif call == "point3f.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3f) or not isinstance(other, rhino3dm.Point3f):
                raise ValueError("point3f.not_equals requires Point3f receiver and argument")
            values[identifier] = receiver != other
        elif call == "point3f.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y", "Z") or not isinstance(receiver, rhino3dm.Point3f):
                raise ValueError("point3f.set_coordinate requires Point3f receiver, X/Y/Z and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "point4d.encode":
            if args or not isinstance(receiver, rhino3dm.Point4d):
                raise ValueError("point4d.encode requires a Point4d receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "point4d.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point4d) or not isinstance(other, rhino3dm.Point4d):
                raise ValueError("point4d.equals requires Point4d receiver and argument")
            values[identifier] = receiver == other
        elif call == "point4d.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point4d) or not isinstance(other, rhino3dm.Point4d):
                raise ValueError("point4d.not_equals requires Point4d receiver and argument")
            values[identifier] = receiver != other
        elif call == "point4d.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y", "Z", "W") or not isinstance(receiver, rhino3dm.Point4d):
                raise ValueError("point4d.set_coordinate requires Point4d receiver, X/Y/Z/W and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "point2d.add_point":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point2d) or not isinstance(other, rhino3dm.Point2d):
                raise ValueError("point2d.add_point requires Point2d receiver and argument")
            values[identifier] = receiver + other
        elif call == "point2d.encode":
            if args or not isinstance(receiver, rhino3dm.Point2d):
                raise ValueError("point2d.encode requires a Point2d receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "point2d.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point2d) or not isinstance(other, rhino3dm.Point2d):
                raise ValueError("point2d.equals requires Point2d receiver and argument")
            values[identifier] = receiver == other
        elif call == "point2d.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point2d) or not isinstance(other, rhino3dm.Point2d):
                raise ValueError("point2d.not_equals requires Point2d receiver and argument")
            values[identifier] = receiver != other
        elif call == "point2d.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y") or not isinstance(receiver, rhino3dm.Point2d):
                raise ValueError("point2d.set_coordinate requires Point2d receiver, X/Y and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "point3d.transform":
            transform = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(transform, rhino3dm.Transform):
                raise ValueError("point3d.transform requires Point3d receiver and Transform")
            values[identifier] = receiver.Transform(transform)
        elif call == "point3d.add_point":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(other, rhino3dm.Point3d):
                raise ValueError("point3d.add_point requires Point3d receiver and argument")
            values[identifier] = receiver + other
        elif call == "point3d.add_vector":
            vector = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(vector, rhino3dm.Vector3d):
                raise ValueError("point3d.add_vector requires Point3d receiver and Vector3d argument")
            values[identifier] = receiver + vector
        elif call == "point3d.scale":
            value = number(resolve(args[0], values), call) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or value is None:
                raise ValueError("point3d.scale requires Point3d receiver and scalar argument")
            values[identifier] = receiver * value
        elif call == "point3d.encode":
            if args or not isinstance(receiver, rhino3dm.Point3d):
                raise ValueError("point3d.encode requires a Point3d receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "point3d.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(other, rhino3dm.Point3d):
                raise ValueError("point3d.equals requires Point3d receiver and argument")
            values[identifier] = receiver == other
        elif call == "point3d.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Point3d) or not isinstance(other, rhino3dm.Point3d):
                raise ValueError("point3d.not_equals requires Point3d receiver and argument")
            values[identifier] = receiver != other
        elif call == "point3d.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y", "Z") or not isinstance(receiver, rhino3dm.Point3d):
                raise ValueError("point3d.set_coordinate requires Point3d receiver, X/Y/Z and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "vector3d.length":
            if not isinstance(receiver, rhino3dm.Vector3d):
                raise ValueError("vector3d.length receiver must be Vector3d")
            values[identifier] = receiver.Length()
        elif call == "vector2d.encode":
            if args or not isinstance(receiver, rhino3dm.Vector2d):
                raise ValueError("vector2d.encode requires a Vector2d receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "vector3f.encode":
            if args or not isinstance(receiver, rhino3dm.Vector3f):
                raise ValueError("vector3f.encode requires a Vector3f receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "vector3f.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector3f) or not isinstance(other, rhino3dm.Vector3f):
                raise ValueError("vector3f.equals requires Vector3f receiver and argument")
            values[identifier] = receiver == other
        elif call == "vector3f.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector3f) or not isinstance(other, rhino3dm.Vector3f):
                raise ValueError("vector3f.not_equals requires Vector3f receiver and argument")
            values[identifier] = receiver != other
        elif call == "vector3f.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y", "Z") or not isinstance(receiver, rhino3dm.Vector3f):
                raise ValueError("vector3f.set_coordinate requires Vector3f receiver, X/Y/Z and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "vector2d.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector2d) or not isinstance(other, rhino3dm.Vector2d):
                raise ValueError("vector2d.equals requires Vector2d receiver and argument")
            values[identifier] = receiver == other
        elif call == "vector2d.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector2d) or not isinstance(other, rhino3dm.Vector2d):
                raise ValueError("vector2d.not_equals requires Vector2d receiver and argument")
            values[identifier] = receiver != other
        elif call == "vector2d.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y") or not isinstance(receiver, rhino3dm.Vector2d):
                raise ValueError("vector2d.set_coordinate requires Vector2d receiver, X/Y and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "interval.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Interval) or not isinstance(other, rhino3dm.Interval):
                raise ValueError("interval.equals requires Interval receiver and argument")
            values[identifier] = receiver == other
        elif call == "interval.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Interval) or not isinstance(other, rhino3dm.Interval):
                raise ValueError("interval.not_equals requires Interval receiver and argument")
            values[identifier] = receiver != other
        elif call == "interval.set_endpoint":
            if len(args) != 2 or args[0] not in ("T0", "T1") or not isinstance(receiver, rhino3dm.Interval):
                raise ValueError("interval.set_endpoint requires Interval receiver, T0/T1 and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "line.direction":
            if args or not isinstance(receiver, rhino3dm.Line):
                raise ValueError("line.direction requires a Line receiver and no arguments")
            values[identifier] = receiver.Direction
        elif call == "line.length":
            if args or not isinstance(receiver, rhino3dm.Line):
                raise ValueError("line.length requires a Line receiver and no arguments")
            values[identifier] = receiver.Length
        elif call == "line.unit_tangent":
            if args or not isinstance(receiver, rhino3dm.Line):
                raise ValueError("line.unit_tangent requires a Line receiver and no arguments")
            values[identifier] = receiver.UnitTangent
        elif call == "line.is_valid":
            if args or not isinstance(receiver, rhino3dm.Line):
                raise ValueError("line.is_valid requires a Line receiver and no arguments")
            values[identifier] = receiver.IsValid
        elif call == "line.point_at":
            if len(args) != 1 or not isinstance(receiver, rhino3dm.Line):
                raise ValueError("line.point_at requires a Line receiver and one number")
            values[identifier] = receiver.PointAt(number(resolve(args[0], values), call))
        elif call == "line.transform":
            transform = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Line) or not isinstance(transform, rhino3dm.Transform):
                raise ValueError("line.transform requires a Line receiver and Transform argument")
            values[identifier] = receiver.Transform(transform)
        elif call == "line.set_endpoint":
            point = resolve(args[1], values) if len(args) == 2 else None
            if args[0] not in ("From", "To") or not isinstance(receiver, rhino3dm.Line) or not isinstance(point, rhino3dm.Point3d):
                raise ValueError("line.set_endpoint requires Line receiver, From/To, and Point3d")
            setattr(receiver, args[0], point)
            values[identifier] = None
        elif call == "vector3d.unitize":
            if not isinstance(receiver, rhino3dm.Vector3d):
                raise ValueError("vector3d.unitize receiver must be Vector3d")
            receiver.Unitize()
            values[identifier] = None
        elif call == "vector3d.encode":
            if args or not isinstance(receiver, rhino3dm.Vector3d):
                raise ValueError("vector3d.encode requires a Vector3d receiver and no arguments")
            values[identifier] = receiver.Encode()
        elif call == "vector3d.equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector3d) or not isinstance(other, rhino3dm.Vector3d):
                raise ValueError("vector3d.equals requires Vector3d receiver and argument")
            values[identifier] = receiver == other
        elif call == "vector3d.not_equals":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector3d) or not isinstance(other, rhino3dm.Vector3d):
                raise ValueError("vector3d.not_equals requires Vector3d receiver and argument")
            values[identifier] = receiver != other
        elif call == "vector3d.set_coordinate":
            if len(args) != 2 or args[0] not in ("X", "Y", "Z") or not isinstance(receiver, rhino3dm.Vector3d):
                raise ValueError("vector3d.set_coordinate requires Vector3d receiver, X/Y/Z and a number")
            setattr(receiver, args[0], number(resolve(args[1], values), call))
            values[identifier] = None
        elif call == "vector3d.dot_product":
            if len(args) != 2:
                raise ValueError("vector3d.dot_product needs two vectors")
            left, right = (resolve(item, values) for item in args)
            if not isinstance(left, rhino3dm.Vector3d) or not isinstance(right, rhino3dm.Vector3d):
                raise ValueError("vector3d.dot_product needs two vectors")
            values[identifier] = rhino3dm.Vector3d.DotProduct(left, right)
        elif call == "vector3d.cross_product":
            if len(args) != 2:
                raise ValueError("vector3d.cross_product needs two vectors")
            left, right = (resolve(item, values) for item in args)
            if not isinstance(left, rhino3dm.Vector3d) or not isinstance(right, rhino3dm.Vector3d):
                raise ValueError("vector3d.cross_product needs two vectors")
            values[identifier] = rhino3dm.Vector3d.CrossProduct(left, right)
        elif call == "vector3d.is_parallel_to":
            other = resolve(args[0], values) if args else None
            if not isinstance(receiver, rhino3dm.Vector3d) or not isinstance(other, rhino3dm.Vector3d):
                raise ValueError("vector3d.is_parallel_to requires Vector3d receiver and argument")
            if len(args) == 1:
                values[identifier] = receiver.IsParallelTo(other)
            elif len(args) == 2:
                values[identifier] = receiver.IsParallelTo(other, number(resolve(args[1], values), call))
            else:
                raise ValueError("vector3d.is_parallel_to needs one or two arguments")
        elif call == "vector3d.vector_angle":
            other = resolve(args[0], values) if len(args) == 1 else None
            if not isinstance(receiver, rhino3dm.Vector3d) or not isinstance(other, rhino3dm.Vector3d):
                raise ValueError("vector3d.vector_angle requires Vector3d receiver and argument")
            values[identifier] = rhino3dm.Vector3d.VectorAngle(receiver, other)
        else:
            raise ValueError(f"unsupported Python runner operation {call!r}")

    observations = []
    for identifier in case.get("observe", []):
        if identifier not in values:
            raise ValueError(f"unknown observation {identifier!r}")
        observations.append({"id": identifier, "value": observed(values[identifier])})
    return {
        "schema": "rhino3dm-rs.conformance-result.v1",
        "case_id": case["case_id"],
        "status": "ok",
        "observed": observations,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("case", type=Path)
    parser.add_argument("--expect-distribution", required=True)
    args = parser.parse_args()
    actual = importlib.metadata.version("rhino3dm")
    if actual != args.expect_distribution:
        parser.error(f"expected distribution {args.expect_distribution}, found {actual}")
    try:
        result = execute(json.loads(args.case.read_text(encoding="utf-8")))
    except (KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        result = {
            "schema": "rhino3dm-rs.conformance-result.v1",
            "case_id": None,
            "status": "runner_error",
            "error": str(error),
        }
    print(json.dumps(result, allow_nan=False, sort_keys=True))
    if result["status"] != "ok":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
