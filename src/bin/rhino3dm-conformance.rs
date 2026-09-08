//! Run the initial language-neutral math conformance cases without invoking
//! Python. The paired Python runner lives in `tools/parity/` and is test-only.

use rhino3dm_rs::{
    Interval, Line, Point2d, Point3d, Point3f, Point4d, Transform, Vector2d, Vector3d, Vector3f,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone)]
enum RuntimeValue {
    Point2d(Point2d),
    Point(Point3d),
    Point3f(Point3f),
    Point4d(Point4d),
    Vector2d(Vector2d),
    Vector(Vector3d),
    Vector3f(Vector3f),
    Interval(Interval),
    Line(Line),
    Transform(Transform),
    FloatArray([f64; 16]),
    Coordinates(BTreeMap<String, f64>),
    Number(f64),
    Boolean(bool),
    None,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: rhino3dm-conformance CASE.json")?,
    );
    let case: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    println!("{}", execute(&case)?);
    Ok(())
}

fn execute(case: &Value) -> Result<Value, String> {
    if case.get("schema").and_then(Value::as_str) != Some("rhino3dm-rs.conformance-case.v1") {
        return Err("unsupported case schema".into());
    }
    let case_id = case
        .get("case_id")
        .and_then(Value::as_str)
        .ok_or("case_id must be a string")?;
    let operations = case
        .get("operations")
        .and_then(Value::as_array)
        .ok_or("operations must be an array")?;
    let mut values = BTreeMap::new();
    for operation in operations {
        let object = operation
            .as_object()
            .ok_or("each operation must be an object")?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("operation id must be a non-empty string")?;
        if values.contains_key(id) {
            return Err(format!("duplicate operation id {id:?}"));
        }
        let call = object
            .get("call")
            .and_then(Value::as_str)
            .ok_or("operation call must be a string")?;
        let args = object
            .get("args")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let receiver = match object.get("receiver") {
            Some(Value::String(id)) => Some(
                values
                    .get(id)
                    .cloned()
                    .ok_or_else(|| format!("unknown receiver {id:?}"))?,
            ),
            Some(_) => return Err("receiver must be a string".into()),
            None => None,
        };
        let value = match call {
            "point2d.new" => RuntimeValue::Point2d(Point2d::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
            )),
            "point3f.new" => RuntimeValue::Point3f(Point3f::new(
                number(args, 0, &values, call)? as f32,
                number(args, 1, &values, call)? as f32,
                number(args, 2, &values, call)? as f32,
            )),
            "point4d.new" => RuntimeValue::Point4d(Point4d::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
                number(args, 2, &values, call)?,
                number(args, 3, &values, call)?,
            )),
            "point3d.new" => RuntimeValue::Point(Point3d::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
                number(args, 2, &values, call)?,
            )),
            "point3d.unset" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Point(Point3d::unset())
            }
            "vector3d.new" => RuntimeValue::Vector(Vector3d::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
                number(args, 2, &values, call)?,
            )),
            "vector3f.new" => RuntimeValue::Vector3f(Vector3f::new(
                number(args, 0, &values, call)? as f32,
                number(args, 1, &values, call)? as f32,
                number(args, 2, &values, call)? as f32,
            )),
            "vector2d.new" => RuntimeValue::Vector2d(Vector2d::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
            )),
            "interval.new" => RuntimeValue::Interval(Interval::new(
                number(args, 0, &values, call)?,
                number(args, 1, &values, call)?,
            )),
            "line.new" => {
                expect_argument_count(args, 2, call)?;
                RuntimeValue::Line(Line::new(
                    point(&args[0], &values, call)?,
                    point(&args[1], &values, call)?,
                ))
            }
            "transform.identity" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Transform(Transform::identity())
            }
            "transform.zero_transformation" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Transform(Transform::zero_transformation())
            }
            "transform.unset" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Transform(Transform::unset())
            }
            "transform.diagonal" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Transform(Transform::diagonal(number(args, 0, &values, call)?))
            }
            "transform.translation" => RuntimeValue::Transform(match args {
                [argument] => {
                    Transform::python_translation_vector(vector(argument, &values, call)?)
                }
                _ => Transform::translation(
                    number(args, 0, &values, call)?,
                    number(args, 1, &values, call)?,
                    number(args, 2, &values, call)?,
                ),
            }),
            "transform.rotation_axis_angle" => {
                expect_argument_count(args, 3, call)?;
                RuntimeValue::Transform(
                    Transform::try_rotation_axis_angle(
                        number(args, 0, &values, call)?,
                        vector(&args[1], &values, call)?,
                        point(&args[2], &values, call)?,
                    )
                    .ok_or("transform.rotation_axis_angle needs a finite non-zero axis")?,
                )
            }
            "transform.multiply" => {
                expect_argument_count(args, 2, call)?;
                RuntimeValue::Transform(
                    transform(&args[0], &values, call)?
                        .multiply(transform(&args[1], &values, call)?),
                )
            }
            "transform.try_get_inverse" => RuntimeValue::Transform(
                expect_transform_receiver(receiver, call)?.python_try_get_inverse(),
            ),
            "transform.transpose" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Transform(expect_transform_receiver(receiver, call)?.transpose())
            }
            "transform.to_float_array" => {
                expect_argument_count(args, 1, call)?;
                let row_dominant = args[0]
                    .as_bool()
                    .ok_or("transform.to_float_array argument must be boolean")?;
                RuntimeValue::FloatArray(
                    expect_transform_receiver(receiver, call)?.to_float_array(row_dominant),
                )
            }
            "point3d.distance_to" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Number(
                    expect_point_receiver(receiver, call)?
                        .distance_to(point(&args[0], &values, call)?),
                )
            }
            "point2d.distance_to" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Number(
                    expect_point2d_receiver(receiver, call)?
                        .distance_to(point2d(&args[0], &values, call)?),
                )
            }
            "point3f.add_point" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point3f(
                    expect_point3f_receiver(receiver, call)?
                        .add_point(point3f(&args[0], &values, call)?),
                )
            }
            "point3f.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(
                    expect_point3f_receiver(receiver, call)?
                        .encode()
                        .into_iter()
                        .map(|(key, value)| (key, f64::from(value)))
                        .collect(),
                )
            }
            "point3f.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point3f_receiver(receiver, call)? == point3f(&args[0], &values, call)?,
                )
            }
            "point3f.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point3f_receiver(receiver, call)? != point3f(&args[0], &values, call)?,
                )
            }
            "point3f.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut point = expect_point3f_receiver(receiver, call)?;
                set_point3f_coordinate(
                    &mut point,
                    coordinate(args, call)?,
                    number(args, 1, &values, call)? as f32,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Point3f(point), call)?;
                RuntimeValue::None
            }
            "point4d.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(expect_point4d_receiver(receiver, call)?.encode())
            }
            "point4d.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point4d_receiver(receiver, call)? == point4d(&args[0], &values, call)?,
                )
            }
            "point4d.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point4d_receiver(receiver, call)? != point4d(&args[0], &values, call)?,
                )
            }
            "point4d.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut point = expect_point4d_receiver(receiver, call)?;
                set_point4d_coordinate(
                    &mut point,
                    coordinate4(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Point4d(point), call)?;
                RuntimeValue::None
            }
            "point2d.add_point" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point2d(
                    expect_point2d_receiver(receiver, call)?
                        .add_point(point2d(&args[0], &values, call)?),
                )
            }
            "point2d.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(expect_point2d_receiver(receiver, call)?.encode())
            }
            "point2d.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point2d_receiver(receiver, call)? == point2d(&args[0], &values, call)?,
                )
            }
            "point2d.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point2d_receiver(receiver, call)? != point2d(&args[0], &values, call)?,
                )
            }
            "point2d.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut point = expect_point2d_receiver(receiver, call)?;
                set_point2d_coordinate(
                    &mut point,
                    coordinate2(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Point2d(point), call)?;
                RuntimeValue::None
            }
            "point3d.transform" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point(
                    expect_point_receiver(receiver, call)?
                        .transformed(transform(&args[0], &values, call)?),
                )
            }
            "point3d.add_point" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point(
                    expect_point_receiver(receiver, call)?
                        .add_point(point(&args[0], &values, call)?),
                )
            }
            "point3d.add_vector" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point(
                    expect_point_receiver(receiver, call)?
                        .add_vector(vector(&args[0], &values, call)?),
                )
            }
            "point3d.scale" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point(
                    expect_point_receiver(receiver, call)?.scaled(number(args, 0, &values, call)?),
                )
            }
            "point3d.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(expect_point_receiver(receiver, call)?.encode())
            }
            "point3d.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point_receiver(receiver, call)? == point(&args[0], &values, call)?,
                )
            }
            "point3d.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_point_receiver(receiver, call)? != point(&args[0], &values, call)?,
                )
            }
            "point3d.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut point = expect_point_receiver(receiver, call)?;
                set_point_coordinate(
                    &mut point,
                    coordinate(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Point(point), call)?;
                RuntimeValue::None
            }
            "vector3d.length" => {
                RuntimeValue::Number(expect_vector_receiver(receiver, call)?.length())
            }
            "vector2d.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(expect_vector2d_receiver(receiver, call)?.encode())
            }
            "vector3f.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(
                    expect_vector3f_receiver(receiver, call)?
                        .encode()
                        .into_iter()
                        .map(|(key, value)| (key, f64::from(value)))
                        .collect(),
                )
            }
            "vector3f.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector3f_receiver(receiver, call)? == vector3f(&args[0], &values, call)?,
                )
            }
            "vector3f.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector3f_receiver(receiver, call)? != vector3f(&args[0], &values, call)?,
                )
            }
            "vector3f.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut vector = expect_vector3f_receiver(receiver, call)?;
                set_vector3f_coordinate(
                    &mut vector,
                    coordinate(args, call)?,
                    number(args, 1, &values, call)? as f32,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Vector3f(vector), call)?;
                RuntimeValue::None
            }
            "vector2d.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector2d_receiver(receiver, call)? == vector2d(&args[0], &values, call)?,
                )
            }
            "vector2d.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector2d_receiver(receiver, call)? != vector2d(&args[0], &values, call)?,
                )
            }
            "vector2d.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut vector = expect_vector2d_receiver(receiver, call)?;
                set_vector2d_coordinate(
                    &mut vector,
                    coordinate2(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Vector2d(vector), call)?;
                RuntimeValue::None
            }
            "interval.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_interval_receiver(receiver, call)? == interval(&args[0], &values, call)?,
                )
            }
            "interval.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_interval_receiver(receiver, call)? != interval(&args[0], &values, call)?,
                )
            }
            "interval.set_endpoint" => {
                expect_argument_count(args, 2, call)?;
                let mut interval = expect_interval_receiver(receiver, call)?;
                set_interval_endpoint(
                    &mut interval,
                    endpoint(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Interval(interval), call)?;
                RuntimeValue::None
            }
            "line.direction" => {
                RuntimeValue::Vector(expect_line_receiver(receiver, call)?.direction())
            }
            "line.length" => RuntimeValue::Number(expect_line_receiver(receiver, call)?.length()),
            "line.unit_tangent" => {
                RuntimeValue::Vector(expect_line_receiver(receiver, call)?.unit_tangent())
            }
            "line.is_valid" => {
                RuntimeValue::Boolean(expect_line_receiver(receiver, call)?.is_valid())
            }
            "line.point_at" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Point(
                    expect_line_receiver(receiver, call)?.point_at(number(args, 0, &values, call)?),
                )
            }
            "line.transform" => {
                expect_argument_count(args, 1, call)?;
                let mut line = expect_line_receiver(receiver, call)?;
                let success = line.transform(transform(&args[0], &values, call)?);
                replace_receiver(&mut values, object, RuntimeValue::Line(line), call)?;
                RuntimeValue::Boolean(success)
            }
            "line.set_endpoint" => {
                expect_argument_count(args, 2, call)?;
                let mut line = expect_line_receiver(receiver, call)?;
                set_line_endpoint(
                    &mut line,
                    line_endpoint(args, call)?,
                    point(&args[1], &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Line(line), call)?;
                RuntimeValue::None
            }
            "vector3d.unitize" => {
                let mut vector = expect_vector_receiver(receiver, call)?;
                vector.unitize();
                replace_receiver(&mut values, object, RuntimeValue::Vector(vector), call)?;
                RuntimeValue::None
            }
            "vector3d.encode" => {
                expect_argument_count(args, 0, call)?;
                RuntimeValue::Coordinates(expect_vector_receiver(receiver, call)?.encode())
            }
            "vector3d.equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector_receiver(receiver, call)? == vector(&args[0], &values, call)?,
                )
            }
            "vector3d.not_equals" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Boolean(
                    expect_vector_receiver(receiver, call)? != vector(&args[0], &values, call)?,
                )
            }
            "vector3d.set_coordinate" => {
                expect_argument_count(args, 2, call)?;
                let mut vector = expect_vector_receiver(receiver, call)?;
                set_vector_coordinate(
                    &mut vector,
                    coordinate(args, call)?,
                    number(args, 1, &values, call)?,
                )?;
                replace_receiver(&mut values, object, RuntimeValue::Vector(vector), call)?;
                RuntimeValue::None
            }
            "vector3d.dot_product" => {
                expect_argument_count(args, 2, call)?;
                RuntimeValue::Number(Vector3d::dot_product(
                    vector(&args[0], &values, call)?,
                    vector(&args[1], &values, call)?,
                ))
            }
            "vector3d.cross_product" => {
                expect_argument_count(args, 2, call)?;
                RuntimeValue::Vector(Vector3d::cross_product(
                    vector(&args[0], &values, call)?,
                    vector(&args[1], &values, call)?,
                ))
            }
            "vector3d.is_parallel_to" => {
                let receiver = expect_vector_receiver(receiver, call)?;
                let other = vector(
                    args.first()
                        .ok_or("vector3d.is_parallel_to requires one vector")?,
                    &values,
                    call,
                )?;
                let result = match args {
                    [_] => receiver.is_parallel_to(other),
                    [_, _] => receiver
                        .is_parallel_to_with_tolerance(other, number(args, 1, &values, call)?),
                    _ => return Err("vector3d.is_parallel_to needs one or two arguments".into()),
                };
                RuntimeValue::Number(f64::from(result))
            }
            "vector3d.vector_angle" => {
                expect_argument_count(args, 1, call)?;
                RuntimeValue::Number(
                    expect_vector_receiver(receiver, call)?
                        .vector_angle(vector(&args[0], &values, call)?),
                )
            }
            _ => return Err(format!("unsupported Rust runner operation {call:?}")),
        };
        values.insert(id.to_owned(), value);
    }
    let observations = case
        .get("observe")
        .and_then(Value::as_array)
        .ok_or("observe must be an array")?
        .iter()
        .map(|value| {
            let id = value.as_str().ok_or("observation must be a string")?;
            let value = values
                .get(id)
                .ok_or_else(|| format!("unknown observation {id:?}"))?;
            Ok(json!({"id": id, "value": encode(value.clone())}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({
        "schema": "rhino3dm-rs.conformance-result.v1",
        "case_id": case_id,
        "status": "ok",
        "observed": observations,
    }))
}

fn expect_argument_count(args: &[Value], count: usize, call: &str) -> Result<(), String> {
    if args.len() == count {
        Ok(())
    } else {
        Err(format!("{call} needs exactly {count} arguments"))
    }
}

fn resolved<'a>(
    value: &'a Value,
    values: &'a BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, String> {
    if let Some(reference) = value
        .as_object()
        .and_then(|object| (object.len() == 1).then_some(object))
        .and_then(|object| object.get("ref"))
        .and_then(Value::as_str)
    {
        values
            .get(reference)
            .cloned()
            .ok_or_else(|| format!("unknown reference {reference:?}"))
    } else if let Some(number) = value.as_f64() {
        if number.is_finite() {
            Ok(RuntimeValue::Number(number))
        } else {
            Err("numbers must be finite".into())
        }
    } else {
        Err("argument must be a finite number or {ref: string}".into())
    }
}

fn number(
    args: &[Value],
    index: usize,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<f64, String> {
    let Some(argument) = args.get(index) else {
        return Err(format!("{call} needs exactly three arguments"));
    };
    match resolved(argument, values)? {
        RuntimeValue::Number(value) => Ok(value),
        _ => Err(format!("{call} argument {index} must be a number")),
    }
}

fn coordinate<'a>(args: &'a [Value], call: &str) -> Result<&'a str, String> {
    args.first()
        .and_then(Value::as_str)
        .filter(|coordinate| matches!(*coordinate, "X" | "Y" | "Z"))
        .ok_or_else(|| format!("{call} coordinate must be one of X, Y or Z"))
}

fn coordinate2<'a>(args: &'a [Value], call: &str) -> Result<&'a str, String> {
    args.first()
        .and_then(Value::as_str)
        .filter(|coordinate| matches!(*coordinate, "X" | "Y"))
        .ok_or_else(|| format!("{call} coordinate must be X or Y"))
}

fn coordinate4<'a>(args: &'a [Value], call: &str) -> Result<&'a str, String> {
    args.first()
        .and_then(Value::as_str)
        .filter(|coordinate| matches!(*coordinate, "X" | "Y" | "Z" | "W"))
        .ok_or_else(|| format!("{call} coordinate must be X, Y, Z or W"))
}

fn endpoint<'a>(args: &'a [Value], call: &str) -> Result<&'a str, String> {
    args.first()
        .and_then(Value::as_str)
        .filter(|endpoint| matches!(*endpoint, "T0" | "T1"))
        .ok_or_else(|| format!("{call} endpoint must be T0 or T1"))
}

fn line_endpoint<'a>(args: &'a [Value], call: &str) -> Result<&'a str, String> {
    args.first()
        .and_then(Value::as_str)
        .filter(|endpoint| matches!(*endpoint, "From" | "To"))
        .ok_or_else(|| format!("{call} endpoint must be From or To"))
}

fn replace_receiver(
    values: &mut BTreeMap<String, RuntimeValue>,
    operation: &serde_json::Map<String, Value>,
    value: RuntimeValue,
    call: &str,
) -> Result<(), String> {
    let receiver_id = operation
        .get("receiver")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{call} requires a string receiver id"))?;
    values.insert(receiver_id.to_owned(), value);
    Ok(())
}

fn set_point_coordinate(point: &mut Point3d, coordinate: &str, value: f64) -> Result<(), String> {
    match coordinate {
        "X" => point.x = value,
        "Y" => point.y = value,
        "Z" => point.z = value,
        _ => return Err("unsupported Point3d coordinate".into()),
    }
    Ok(())
}

fn set_point2d_coordinate(point: &mut Point2d, coordinate: &str, value: f64) -> Result<(), String> {
    match coordinate {
        "X" => point.x = value,
        "Y" => point.y = value,
        _ => return Err("unsupported Point2d coordinate".into()),
    }
    Ok(())
}

fn set_point3f_coordinate(point: &mut Point3f, coordinate: &str, value: f32) -> Result<(), String> {
    match coordinate {
        "X" => point.x = value,
        "Y" => point.y = value,
        "Z" => point.z = value,
        _ => return Err("unsupported Point3f coordinate".into()),
    }
    Ok(())
}

fn set_point4d_coordinate(point: &mut Point4d, coordinate: &str, value: f64) -> Result<(), String> {
    match coordinate {
        "X" => point.x = value,
        "Y" => point.y = value,
        "Z" => point.z = value,
        "W" => point.w = value,
        _ => return Err("unsupported Point4d coordinate".into()),
    }
    Ok(())
}

fn set_vector_coordinate(
    vector: &mut Vector3d,
    coordinate: &str,
    value: f64,
) -> Result<(), String> {
    match coordinate {
        "X" => vector.x = value,
        "Y" => vector.y = value,
        "Z" => vector.z = value,
        _ => return Err("unsupported Vector3d coordinate".into()),
    }
    Ok(())
}

fn set_vector2d_coordinate(
    vector: &mut Vector2d,
    coordinate: &str,
    value: f64,
) -> Result<(), String> {
    match coordinate {
        "X" => vector.x = value,
        "Y" => vector.y = value,
        _ => return Err("unsupported Vector2d coordinate".into()),
    }
    Ok(())
}

fn set_vector3f_coordinate(
    vector: &mut Vector3f,
    coordinate: &str,
    value: f32,
) -> Result<(), String> {
    match coordinate {
        "X" => vector.x = value,
        "Y" => vector.y = value,
        "Z" => vector.z = value,
        _ => return Err("unsupported Vector3f coordinate".into()),
    }
    Ok(())
}

fn set_interval_endpoint(
    interval: &mut Interval,
    endpoint: &str,
    value: f64,
) -> Result<(), String> {
    match endpoint {
        "T0" => interval.t0 = value,
        "T1" => interval.t1 = value,
        _ => return Err("unsupported Interval endpoint".into()),
    }
    Ok(())
}

fn set_line_endpoint(line: &mut Line, endpoint: &str, value: Point3d) -> Result<(), String> {
    match endpoint {
        "From" => line.from = value,
        "To" => line.to = value,
        _ => return Err("unsupported Line endpoint".into()),
    }
    Ok(())
}

fn point(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Point3d, String> {
    match resolved(value, values)? {
        RuntimeValue::Point(value) => Ok(value),
        _ => Err(format!("{call} requires Point3d arguments")),
    }
}

fn point2d(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Point2d, String> {
    match resolved(value, values)? {
        RuntimeValue::Point2d(value) => Ok(value),
        _ => Err(format!("{call} requires Point2d arguments")),
    }
}

fn point3f(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Point3f, String> {
    match resolved(value, values)? {
        RuntimeValue::Point3f(value) => Ok(value),
        _ => Err(format!("{call} requires Point3f arguments")),
    }
}

fn point4d(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Point4d, String> {
    match resolved(value, values)? {
        RuntimeValue::Point4d(value) => Ok(value),
        _ => Err(format!("{call} requires Point4d arguments")),
    }
}

fn vector(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Vector3d, String> {
    match resolved(value, values)? {
        RuntimeValue::Vector(value) => Ok(value),
        _ => Err(format!("{call} single argument must be Vector3d")),
    }
}

fn vector2d(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Vector2d, String> {
    match resolved(value, values)? {
        RuntimeValue::Vector2d(value) => Ok(value),
        _ => Err(format!("{call} requires Vector2d arguments")),
    }
}

fn vector3f(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Vector3f, String> {
    match resolved(value, values)? {
        RuntimeValue::Vector3f(value) => Ok(value),
        _ => Err(format!("{call} requires Vector3f arguments")),
    }
}

fn interval(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Interval, String> {
    match resolved(value, values)? {
        RuntimeValue::Interval(value) => Ok(value),
        _ => Err(format!("{call} requires Interval arguments")),
    }
}

fn transform(
    value: &Value,
    values: &BTreeMap<String, RuntimeValue>,
    call: &str,
) -> Result<Transform, String> {
    match resolved(value, values)? {
        RuntimeValue::Transform(value) => Ok(value),
        _ => Err(format!("{call} arguments must be Transform")),
    }
}

fn expect_point_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Point3d, String> {
    match receiver {
        Some(RuntimeValue::Point(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Point3d")),
    }
}

fn expect_point2d_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Point2d, String> {
    match receiver {
        Some(RuntimeValue::Point2d(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Point2d")),
    }
}

fn expect_point3f_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Point3f, String> {
    match receiver {
        Some(RuntimeValue::Point3f(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Point3f")),
    }
}

fn expect_point4d_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Point4d, String> {
    match receiver {
        Some(RuntimeValue::Point4d(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Point4d")),
    }
}

fn expect_vector_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Vector3d, String> {
    match receiver {
        Some(RuntimeValue::Vector(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Vector3d")),
    }
}

fn expect_vector2d_receiver(
    receiver: Option<RuntimeValue>,
    call: &str,
) -> Result<Vector2d, String> {
    match receiver {
        Some(RuntimeValue::Vector2d(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Vector2d")),
    }
}

fn expect_vector3f_receiver(
    receiver: Option<RuntimeValue>,
    call: &str,
) -> Result<Vector3f, String> {
    match receiver {
        Some(RuntimeValue::Vector3f(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Vector3f")),
    }
}

fn expect_interval_receiver(
    receiver: Option<RuntimeValue>,
    call: &str,
) -> Result<Interval, String> {
    match receiver {
        Some(RuntimeValue::Interval(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Interval")),
    }
}

fn expect_line_receiver(receiver: Option<RuntimeValue>, call: &str) -> Result<Line, String> {
    match receiver {
        Some(RuntimeValue::Line(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Line")),
    }
}

fn expect_transform_receiver(
    receiver: Option<RuntimeValue>,
    call: &str,
) -> Result<Transform, String> {
    match receiver {
        Some(RuntimeValue::Transform(value)) => Ok(value),
        _ => Err(format!("{call} receiver must be Transform")),
    }
}

fn encode(value: RuntimeValue) -> Value {
    match value {
        RuntimeValue::Point2d(value) => json!({"type":"Point2d", "x":value.x, "y":value.y}),
        RuntimeValue::Point(value) => {
            json!({"type":"Point3d", "x":value.x, "y":value.y, "z":value.z})
        }
        RuntimeValue::Point3f(value) => json!({
            "type":"Point3f",
            "x":f64::from(value.x),
            "y":f64::from(value.y),
            "z":f64::from(value.z),
        }),
        RuntimeValue::Point4d(value) => json!({
            "type":"Point4d",
            "x":value.x,
            "y":value.y,
            "z":value.z,
            "w":value.w,
        }),
        RuntimeValue::Vector(value) => {
            json!({"type":"Vector3d", "x":value.x, "y":value.y, "z":value.z})
        }
        RuntimeValue::Vector3f(value) => json!({
            "type":"Vector3f",
            "x":f64::from(value.x),
            "y":f64::from(value.y),
            "z":f64::from(value.z),
        }),
        RuntimeValue::Vector2d(value) => json!({"type":"Vector2d", "x":value.x, "y":value.y}),
        RuntimeValue::Interval(value) => json!({"type":"Interval", "t0":value.t0, "t1":value.t1}),
        RuntimeValue::Line(value) => json!({
            "type":"Line",
            "from":{"x":value.from.x,"y":value.from.y,"z":value.from.z},
            "to":{"x":value.to.x,"y":value.to.y,"z":value.to.z},
        }),
        RuntimeValue::Transform(value) => json!({
            "type":"Transform",
            "matrix": value.to_row_major_array().map(encode_float),
            "is_identity": value.is_identity(),
            "is_affine": value.is_affine(),
            "is_valid": value.is_valid(),
            "is_zero": value.is_zero(),
            "is_zero_4x4": value.is_zero_4x4(),
            "is_zero_transformation": value.is_zero_transformation(),
            "is_linear": value.is_linear(),
            "is_rotation": value.is_rotation(),
            "determinant": value.determinant(),
        }),
        RuntimeValue::FloatArray(values) => json!({
            "type": "FloatArray",
            "values": values.map(encode_float),
        }),
        RuntimeValue::Coordinates(values) => json!({
            "type": "Coordinates",
            "value": values,
        }),
        RuntimeValue::Number(value) => json!({"type":"Number", "value":value}),
        RuntimeValue::Boolean(value) => json!({"type":"Boolean", "value":value}),
        RuntimeValue::None => json!({"type":"None"}),
    }
}

fn encode_float(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else {
        let kind = if value.is_nan() {
            "nan"
        } else if value.is_sign_negative() {
            "negative_infinity"
        } else {
            "positive_infinity"
        };
        json!({"tag": "float", "value": kind})
    }
}
