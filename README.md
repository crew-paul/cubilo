# 큐빌로 (Cubilo) — 개발 중인 3D 모델링 앱

Bounded first slice of Phase 0, **not a finished app or completed Phase 0**.
Workspace: `studio-geometry` generates validated indexed meshes,
`studio-io` writes binary STL, and `studio-app` provides an egui/wgpu native viewport plus a sample CLI.
`studio-render` renders depth-tested meshes into a wgpu texture.
Project reference: [development plan v1.5](docs/development-plan.md).

Install stable Rust with rustfmt and clippy. On a new shell, source
`"$HOME/.cargo/env"` if Cargo is not on PATH.

```sh
# Native GUI (Korean UI, orbit/pan/zoom, box/cylinder properties, STL export)
cargo run -p studio-app
# Headless samples
cargo run -p studio-app -- --output-dir ./generated/samples
cargo fmt --all -- --check
cargo test -p studio-geometry -p studio-core -p studio-io -p studio-app
# Explicit hardware-only validation (not run by default CI)
cargo test -p studio-render offscreen_gpu_scene_and_selection_smoke -- --ignored --nocapture
cargo clippy -p studio-geometry -p studio-io -p studio-app --all-targets -- -D warnings
```

The output directory must be explicitly supplied; it is created as needed.
Existing sample files are never overwritten (use a fresh directory for each run).
Outputs: `hollow-box.stl` (60×50×80 mm) and `hollow-cylinder.stl`
(60 mm diameter ×80 mm high, 96 segments), both open topped, wall 2 mm,
bottom 3 mm. STL has no unit metadata; import as millimetres.
Generated files and `target/` are ignored by Git.

Box and cylinder parameters retain wall, bottom and open-top settings when
Hollow is disabled. Closed hollows use the bottom thickness for the ceiling.
Dimensions and stored thicknesses must be finite, 0.0001–1,000,000 mm;
enabled hollows must leave a positive cavity. Cylinder subdivisions: 3–4096.
Cylinder circles are inscribed polygons with radial wall offsets; tessellation
approximates circular dimensions/volume. Geometry uses f64; STL export rejects
f32 conversion that merges vertices or collapses triangles.

UI font: bundled Nanum Gothic, licensed under SIL OFL (see assets/fonts/OFL-NanumGothic.txt).

Implemented: multiple objects, picking, numeric transforms, XY move tool/snapping, grouping,
bounded Undo/Redo, project save/load and local recovery (parameter-only JSON).
Six primitives are available: box/cylinder/sphere/cone/torus/hemisphere.
Box rounding supports Hollow; cylinder supports Taper+Hollow and standalone edge rounding.
Unsupported cylinder rounding combinations are disabled. Geometry dimensions remain parametric.

Implemented: editable Boolean trees (union/difference/intersection), Korean/Latin
3D text with holes, native file dialogs, and one background geometry/export thread.
Preview requests coalesce; STL uses the click-time scene snapshot. Export can be cancelled.

Pending: integrated native-dialog/CSG/text UI validation, gizmo/alignment,
real Windows 10/11 x64 testing and real slicer (Bambu Studio) validation.
Native Mac mini M1 / 8 GB / macOS 15.2: Metal launch and hollow primitive UI verified.
Build a local app bundle with `scripts/package-macos.sh debug` or `scripts/package-macos.sh release`.
CI definitions have not yet run remotely.
No release or major Phase push is warranted by this slice.
