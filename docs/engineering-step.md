# Exact engineering STEP import

`rhino3dm-rs::step::import_exact_step` accepts ISO 10303-21 STEP sources and
writes a Rhino 3DM only when native B-rep/NURBS transfer passes every
engineering gate. It never approximates a STEP model with a triangle mesh. A
rejection is a successful safety result: no output file is written.

The gate verifies:

- the STEP decoder transferred geometry;
- there are B-rep bodies, and by default every one is a closed solid;
- neutral B-rep topology, carrier reachability, bounds, and geometric
  consistency pass validation;
- there are no geometry, topology, units, or product-structure losses;
- the Rhino encoder reports no geometry, topology, units, or product loss;
- the final target is atomically replaced only after all checks pass.

```rust
use rhino3dm_rs::step::{import_exact_step, ExactStepOptions};

let report = import_exact_step(
    "gearbox.step",
    "gearbox.3dm",
    ExactStepOptions::default(),
)?;
assert!(report.source_bodies > 0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Sheets and wires are rejected by default because the intended engineering
workflow requires closed manifold solids. Use
`ExactStepOptions { require_closed_manifold_solids: false, ..Default::default() }`
only when the source is intentionally non-solid and downstream validation
understands that distinction.

## Assemblies and placements

`import_exact_steps` is currently a safe batch importer: it produces one
independent exact 3DM per STEP file. It deliberately does **not** flatten or
invent a Rhino block assembly. The current underlying 3DM writer does not yet
write block definition membership and occurrence transforms. Treating several
STEP files as a positioned 3DM assembly before that support exists would lose
occurrence structure, so this route is refused rather than silently producing
the wrong engineering model.

The next admission milestone is a fixture-backed block writer that proves,
for each component, stable definition/occurrence IDs, member UUIDs, local and
world transforms, nested block closure, and dimensional consistency after a
Rhino/Python round trip.
