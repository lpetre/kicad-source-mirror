# kicad-geom: the SHAPE hierarchy

Decision for package G2 (binding for G3-G5 and the board model). C++: `libs/kimath/include/geometry/shape*.h`.

## Types

| C++ | Rust |
|---|---|
| `SHAPE_BASE` + `SHAPE` (abstract, virtual) | `trait Shape: Debug + Send + Sync` (object safe), `geometry::shape` |
| `SHAPE_LINE_CHAIN_BASE` (abstract) | `trait ShapeLineChainBase: Shape` |
| `SHAPE_TYPE` | `enum ShapeType` (`#[repr(i32)]`, same numbering) |
| `Type()` + `static_cast` / `dynamic_cast` | `Shape::as_shape_ref() -> ShapeRef<'_>`: an enum of borrows (`Rect`, `Segment`, `LineChain`, `Circle`, `Simple`, `PolySet`, `Compound`, `Arc`, `Null`, `PolySetTriangle`), matched exhaustively |
| `SHAPE_RECT`, `SHAPE_ARC`, ... | `ShapeRect`, `ShapeArc`, ... (plain structs implementing `Shape`) |

- **Ownership.** `SHAPE*` owned / `std::unique_ptr<SHAPE>` becomes `Box<dyn Shape>`; `std::shared_ptr<SHAPE>`
  becomes `std::sync::Arc<dyn Shape>`; `const SHAPE*` parameters become `&dyn Shape`. `Box<dyn Shape>` is `Clone`
  (via `Shape::clone_shape`, the C++ `Clone()`). `ShapeCompound` owns `Vec<Box<dyn Shape>>`.
- **Dispatch.** The collision matrix (`shape_collisions.cpp`) is `geometry::shape_collisions::collide_shapes(ShapeRef,
  ShapeRef, ...)`; it matches on the pair of `ShapeRef`s, so adding a shape kind is a compile error everywhere it must
  be handled. `SH_ELLIPSE` exists in `ShapeType`; its `ShapeRef` variant is `ShapeRef::Ellipse` (`ShapeEllipse`, G6b).
- **`SHAPE_LINE_CHAIN_BASE` overrides.** A sub-trait cannot override super-trait methods, so the shared
  implementations live in `ShapeLineChainBase` as default methods with an `lcb_` prefix (`lcb_collide_point`,
  `lcb_collide_seg`, `lcb_squared_distance`, `lcb_point_inside`) plus `point_on_edge` / `edge_containing_point`.
  Implementors forward the corresponding `Shape` methods to them, or to their own C++ override (`ShapeLineChain`
  overrides `Collide` and `PointInside`, as in C++). Inside the default methods calls such as `PointInside()` go through
  `Shape`, i.e. they are virtual exactly where the C++ calls are virtual.
- **Out-parameters.** Collision functions keep the C++ nullable-pointer contract (`int* aActual`,
  `VECTOR2I* aLocation`, `VECTOR2I* aMTV`) as `Option<&mut i32>` / `Option<&mut Vector2I>`, because the C++ behaves
  differently depending on whether a value was requested ("any collision will do"). Overloads follow `PORTING.md`:
  `collide_point`, `collide_seg`, `collide` (shape), `collide_mtv`. `SHAPE_ARC::NearestPoints` keeps its in/out
  references (`&mut`), since the C++ reads them when nothing better is found.
- **Mutable caches.** `mutable` members written by `const` methods (`SHAPE_LINE_CHAIN::m_bbox` via
  `GenerateBBoxCache()`) take `&mut self` instead, so every shape is `Send + Sync` without interior mutability.
- **Poly-set triangles.** `TRIANGULATED_POLYGON::TRI` points into its parent's vertex list; the G4 skeleton stores the
  three vertices next to the indices (`Tri`), G4 may replace this as long as `ShapeRef::PolySetTriangle(&Tri)` stays.
- **Not-yet-ported pairs** of the collision matrix go to `shape_collisions::unsupported_g5()` (returns false like the
  C++ fallback); `TransformToPolygon` of round shapes waits for `convert_basic_shapes_to_polygon`. Both are listed in
  `crates/kicad-geom/PORT_STATUS.md`.
