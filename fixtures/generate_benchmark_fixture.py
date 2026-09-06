#!/usr/bin/env python3
"""Generate the public structural benchmark fixture with rhino3dm."""

from pathlib import Path
import uuid

import rhino3dm


ROOT = Path(__file__).resolve().parent
OUTPUT = ROOT / "structural-benchmark-v1.3dm"
POINT_COUNT = 2048
INSTANCE_COUNT = 256
NAMESPACE = uuid.UUID("6f72afc9-993a-4668-b8dc-16856c30fd7e")


def object_attributes(name: str, layer: int, kind: str, index: int):
    attributes = rhino3dm.ObjectAttributes()
    attributes.Id = uuid.uuid5(NAMESPACE, f"{kind}:{index}")
    attributes.Name = name
    attributes.LayerIndex = layer
    attributes.SetUserString("fixture", "structural-benchmark-v1")
    attributes.SetUserString("kind", kind)
    attributes.SetUserString("index", str(index))
    return attributes


def main():
    model = rhino3dm.File3dm()
    model.ApplicationName = "rhino3dm-rs fixture generator"
    model.ApplicationDetails = "Public structural benchmark fixture"
    model.Settings.ModelUnitSystem = rhino3dm.UnitSystem.Millimeters
    model.Settings.ModelAbsoluteTolerance = 0.001

    for name, color in (
        ("points", (74, 222, 190, 255)),
        ("instances", (255, 146, 82, 255)),
    ):
        layer = rhino3dm.Layer()
        layer.Name = name
        layer.Color = color
        model.Layers.Add(layer)

    for index in range(POINT_COUNT):
        x = float(index % 64) * 5.0
        y = float((index // 64) % 32) * 5.0
        z = float(index // (64 * 32)) * 12.0
        model.Objects.AddPoint(
            rhino3dm.Point3d(x, y, z),
            object_attributes(f"point-{index:04d}", 0, "point", index),
        )

    definition_attributes = object_attributes("benchmark-marker", 1, "definition", 0)
    definition_index = model.InstanceDefinitions.Add(
        "benchmark-marker",
        "Single-point marker used to exercise instance references",
        "",
        "",
        rhino3dm.Point3d(0.0, 0.0, 0.0),
        (rhino3dm.Point(rhino3dm.Point3d(0.0, 0.0, 0.0)),),
        (definition_attributes,),
    )
    definition = model.InstanceDefinitions.FindIndex(definition_index)

    for index in range(INSTANCE_COUNT):
        transform = rhino3dm.Transform.Translation(
            float(index % 32) * 10.0,
            float(index // 32) * 10.0,
            24.0,
        )
        reference = rhino3dm.InstanceReference(definition.Id, transform)
        model.Objects.AddInstanceObject(
            reference,
            object_attributes(f"instance-{index:03d}", 1, "instance", index),
        )

    if not model.Write(str(OUTPUT), 8):
        raise RuntimeError(f"failed to write {OUTPUT}")
    print(
        f"wrote {OUTPUT} with {len(model.Objects)} objects, "
        f"{len(model.InstanceDefinitions)} definition, units=millimeters"
    )


if __name__ == "__main__":
    main()
