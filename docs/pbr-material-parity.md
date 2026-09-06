# PBR material reconstruction gate

`rhino3dm-rs` does **not** currently provide PBR material parity with Python
`rhino3dm`. The Rust geometry bridge retains some Rhino material and RDK
userdata as opaque source data and exposes neutral appearance fields, but
opaque retention is not enough to recreate a material faithfully.

`File3dm::capabilities().complete_pbr_reconstruction()` therefore remains
`false`. A renderer must reject strict PBR requests while any required channel
is unsupported; it must never silently replace missing appearance with a
default gray material.

## Required decoded model

The public Rust API must preserve, per material:

- stable material, render-content, plug-in, and asset identifiers;
- material name and source assignment;
- base color and alpha with an explicit color-space interpretation;
- metallic, roughness, specular level/tint, and index of refraction;
- opacity, opacity roughness, and opacity IOR;
- emission color and strength;
- clearcoat and clearcoat roughness;
- anisotropy and anisotropy rotation;
- sheen, sheen tint, and subsurface parameters;
- legacy Rhino material values when no PBR representation exists;
- unknown or plug-in-specific parameters as losslessly retained typed/opaque
  extensions with diagnostics.

The public texture API must preserve:

- semantic slot: base color, metallic, roughness, combined ORM, normal, bump,
  opacity, emission, clearcoat, environment, or unknown custom slot;
- enabled state, blend amount, and channel selection;
- original URI/path plus embedded-file identity;
- exact embedded bytes, byte length, MIME type when known, and SHA-256;
- color-space role (`sRGB`, linear/data, or explicitly unknown);
- U/V/W wrap modes;
- offset, repeat, rotation, and complete UVW transform;
- mapping channel and mapping primitive/source;
- normal-versus-height interpretation and strength.

Geometry must preserve:

- every mesh UV channel with its channel ID;
- vertex/corner indexing and seams without reordering ambiguity;
- per-face and per-object material assignment;
- accumulated instance transforms without baking UVs twice;
- render meshes associated with their source object/Brep.

## Public parity fixture

A dedicated `pbr-material-v1.3dm` fixture must contain:

1. A UV-mapped mesh with an asymmetric color-checker texture.
2. Metallic/roughness variation with non-default scalar values.
3. Normal or bump texture data.
4. Emission, opacity, clearcoat, and anisotropy values.
5. Non-default repeat, offset, rotation, and wrap modes.
6. At least two UV seams and a second mapping channel.
7. Per-object and per-face material assignment.
8. One embedded texture and one external texture reference.
9. An instanced copy under translation, rotation, and non-uniform scale.
10. One unknown/custom render parameter to prove loss reporting.

The fixture generator, texture assets, and expected Python census must be
redistributable and committed with the repository.

## Admission tests

Rust and Python extraction must produce matching canonical JSON for every
supported field. Binary assets compare by SHA-256. Floating-point values use
documented absolute and relative tolerances; identifiers, indices, slot names,
wrap modes, and hashes require exact equality.

Rendering admission additionally requires:

- a UV diagnostic render with no flipped, swapped, or double-transformed UVs;
- paired Rust/Python-or-Rhino reference renders from two nonparallel cameras;
- object/material ID buffers proving the correct assignment;
- no missing-texture or substituted-material diagnostics;
- an explicit renderer manifest with source hash, texture hashes, camera
  matrices, color management, lighting, and all losses.

Performance results are reported only after parity passes. A faster decoder
that omits PBR data, textures, or UVs is not a successful rendering benchmark.
