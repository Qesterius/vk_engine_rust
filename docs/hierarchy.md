# Entity Hierarchy

## Glossary

**Entity** — an integer ID in the ECS; has no data of its own, only the components attached to it.  
**Component** — a plain data struct attached to an entity and stored in ECS column storage.  
**Query** — a Bevy mechanism that iterates over all entities matching a set of component types; borrows that storage for the duration of the iteration.  
**Archetype** — type of ECS architecture that stands for distinct combination of component types; all entities with the same set of components live in the same archetype table.  
**Hierarchy** — a tree of entities linked by `Parent` / `Children` components; a root entity has no `Parent`.  
**Propagation** — the process of computing a child entity's component value from its parent's value, applied depth-layer by depth-layer each frame.  
**Aliasing** — two references pointing to the same memory location; Rust's borrow checker forbids simultaneous mutable + shared aliases at compile time.  
**`get_unchecked`** — a Bevy `Query` method that bypasses the borrow checker's aliasing restriction; returns a reference without registering a borrow. Safe only when the caller can prove the two accesses refer to different entities.  
**`SystemParam`** — a Bevy trait that lets a custom type appear as a parameter in a system function; the framework constructs and injects it automatically.  
**`Changed<T>` filter** — a Bevy query filter that matches only entities whose component `T` was written since the last time the system ran.  
**`Aabb`** — Axis-Aligned Bounding Box; a rectangular volume around a mesh whose sides are always parallel to the world X/Y/Z axes. Used for cheap broad-phase culling and collision checks before more expensive exact geometry tests.  

## Overview

The hierarchy system establishes parent-child relationships between entities and propagates component data down the tree in depth order each frame. Transform moving whole model should propagate to move also its arms and legs.

The design is constrained by two **problems**. 
* Rust's borrow checker: Bevy's `Query` system cannot simultaneously lend a mutable reference to a child's component and a read-only reference to the same component type on its parent, because both live in the same storage and the compiler cannot prove they point to different entities at compile time. 
* generality: the solution must be transparent to system authors and applicable to any propagated component (transforms, visibility, layer masks, team affiliation) — not specific to transforms. So w goal is to have an Query API

| Component | Propagation direction | Rule |
|---|---|---|
| `GlobalTransform` | root → leaves | `parent_global * local.to_matrix()` |
| `Visibility` | root → leaves | child inherits parent; hidden parent hides all descendants |
| Collision / layer mask | root → leaves | child inherits parent's layer membership |
| Team affiliation | root → leaves | child inherits parent's team |
| `Aabb` (bounding volume) | leaves → root | parent AABB = union of all children AABBs |

The design must solve both without requiring every propagation system to implement its own traversal, ordering, or aliasing workaround.


## ECS Context

Systems in Bevy if they work on entities, they take as argument type `Query`, that is possible to apply filters and iterate over results. This is not what we want because of previous problems. To not obfuscate API we need to implement `SystemParam` trait and handle manualy this hierarchical access. 

### SystemParam



## Aliasing Problem and Solution Space

Any propagation pass needs to read a component from a parent and write the same component type on a child in one pass. A `Query<&mut GlobalTransform>` is a single mutable borrow over all entities with that component — the compiler cannot verify parent ≠ child, so it rejects simultaneous read + write as potentially aliasing.

I see three approaches:

1. **Two-phase read/write (`ParamSet` or split queries)** — Query parents read-only, then children mutable, in two separate passes. Safe, but requires intermediate storage and two full iterations per system per frame.

2. **Gather → compute → scatter** — Collect all parent values into a `Vec`, release the borrow, write to children via a fresh query. Fully safe; no `unsafe`. Cost: per-frame allocation and an extra iteration pass.

3. **Depth-layered traversal + `unsafe get_unchecked`** *(used; same mechanism as `bevy_transform`)* — Pre-sort entities into BFS layers. Within each layer, every parent is in a prior (already-processed) layer. Call `unsafe query.get_unchecked(entity)` to borrow parent and child from the same query simultaneously. Safety rests on two invariants: no-self-parenting guarantees parent entity ≠ child entity (no memory alias); depth ordering guarantees the parent's value is stable. **Depth ordering alone does not solve the borrow problem** — it ensures correctness; `get_unchecked` is what bypasses the compiler restriction. `bevy_transform` uses DFS for the same reason; this design uses BFS to enable per-layer parallelism via `propagate_bfs_par`.

## Core Components

### Parent / Children Links

**What it is** — `Parent(Entity)` and `Children(SmallVec<[Entity; 8]>)` are components that establish the tree structure. Every entity in the hierarchy holds a `Parent` pointing at its immediate ancestor; every non-leaf entity holds `Children` listing its immediate descendants. Roots have no `Parent`. These components carry no computed data — they are the graph topology, not the values that flow through it. Mutations must keep both sides consistent; a safe insertion wrapper enforces this and also rejects self-parenting and cycles (see Invariants).

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Hierarchy embedded in `Transform` component *(baseline)* | No separate components | Couples hierarchy structure to one component type; cannot query topology independently; Not universal between systems |
| `Parent(Entity)` only, derive children by full scan | Half the storage; no sync requirement | Deriving children requires scanning all entities each frame; O(n) per query |
| `Parent(Entity)` + `Children(SmallVec)` *(used)* | Both traversal directions available cheaply; matches Bevy and Unity DOTS convention | Redundant data; mutations must update both sides atomically |

### HierarchyOrder Resource

**What it is** — a resource holding `Vec<Vec<Entity>>`, a BFS partition of all hierarchical entities by depth. Index 0 contains roots, index 1 their children, and so on. A maintenance system rebuilds it using `Changed<Parent>` detection and runs in `pre_update` before any propagation system. Systems that need depth-ordered access read this resource rather than computing order themselves.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Each propagation system does its own DFS inline *(baseline)* | No shared resource; no rebuild overhead | Traversal logic duplicated per system; O(n) repeated work per system per frame |
| Rebuild every frame unconditionally | Always correct; simple | O(n) over all hierarchical entities every frame regardless of mutation rate |
| Rebuild on `Changed<Parent>` detection *(used)* | Pays only when hierarchy mutates; free for static scenes after init | One-frame lag possible if mutation and rebuild run in same schedule pass |
| Event-based rebuild via `HierarchyChanged` event | Zero overhead when static; explicit trigger | All mutation sites must emit the event; easy to miss |
| Unity DOTS approach: depth-tagged chunks | Enables parallel jobs per chunk; cache-efficient layout | Requires chunk-based ECS storage not available in Bevy |

**Limitations:** `Changed<Parent>` detects component writes, not semantic hierarchy changes — a no-op write triggers a rebuild. For highly dynamic hierarchies with many structural changes per frame, rebuilds happen frequently; the O(n) cost is paid each time. Static scenes pay only at startup.

### HierarchyQuery

**What it is** — a custom `SystemParam` wrapping `Query<Q>` and `Res<HierarchyOrder>`. Exposes `propagate_bfs(closure)`, `propagate_bfs_reversed(closure)`, and `propagate_bfs_par(closure)`. Each closure receives `(parent: readonly, child: mutable)`. The `unsafe get_unchecked` is encapsulated here once; callers write a normal system signature and call one method — ordering, aliasing, and layering are invisible to them.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Caller iterates `HierarchyOrder` directly with `Query` *(baseline)* | No abstraction layer | Aliasing problem reappears at every call site; ordering logic duplicated |
| `ParamSet` two-phase read/write | Safe; no unsafe | Two separate passes per system; cannot express "parent data informs child write" in one closure; not composable |
| Gather-compute-scatter (collect to `Vec<(Entity, T)>`, write back) | Fully safe; no unsafe; no aliasing | Per-frame allocation; extra iteration pass; cache pressure |
| `HierarchyQuery<Q>` with internal `unsafe get_unchecked` *(used)* | Transparent to callers; single closure API; no allocation; mirrors Bevy `bevy_hierarchy` internals | `unsafe` block inside; safety argument must hold at insertion site |
| Flecs query traversal (`up(ChildOf)`) | Framework handles all ordering and aliasing natively | Not available in Bevy ECS without fundamental changes |
| Depth-layer parallel via rayon *(used as variant)* | Free parallelism within each layer; scales with entity count | Closure must be `Send + Sync`; parallel scheduling adds coordination overhead for small hierarchies |

**Limitations:** the `propagate_bfs_par` variant requires closures to be `Send + Sync` and cannot capture `&mut` state outside the closure. Parallel execution within a layer is only beneficial above roughly 64 entities per layer; below that, coordination overhead dominates. The different-entity invariant must be maintained permanently — if cycle detection is ever relaxed or the insertion wrapper is bypassed, the internal `unsafe` becomes undefined behaviour.

### LocalTransform / GlobalTransform

**What it is** — the canonical two-component pair for propagated spatial data. `LocalTransform` stores position, rotation, and scale relative to the parent (or world origin for roots); it is what users write. `GlobalTransform` stores the computed world-space 4×4 matrix, derived each frame by `propagate_transforms` as `parent_global * local.to_matrix()`. All other systems (rendering, audio, physics, bounding volumes) read `GlobalTransform` only. For components where propagation copies or inherits a value of the same type (visibility, team, collision layer), one component suffices because depth-layer ordering ensures the parent's value is stable before the child is processed.

Two-component is not a framework requirement — it is a semantic consequence of needing to store computed world-space state separately from local user input. `HierarchyQuery` works identically for one-component and two-component cases.

**Alternatives:**

| Approach | Pros | Cons |
|---|---|---|
| Single `Transform` in world space, user composes manually *(baseline)* | One component per entity | User must decompose and recompose on every parent change; breaks silently when hierarchy changes at runtime |
| Single `Transform` (local), framework side-table `HashMap<Entity, Mat4>` for world | One user-facing component | Framework allocates and maintains a parallel map; extra indirection for every reader; non-ECS data |
| `LocalTransform` + `GlobalTransform` *(used)* | Clear ownership; `GlobalTransform` is the canonical read source; matches Unity DOTS `LocalToParent` / `LocalToWorld` and Bevy `Transform` / `GlobalTransform` convention | Two components per entity; authors must know which to write |

**Limitations:** writing directly to `GlobalTransform` bypasses propagation and is overwritten next frame. Any system that needs world-space position must query `GlobalTransform` — querying `LocalTransform` is incorrect except for systems that specifically operate in parent-local space.

## Cross-Cutting Concerns

### Scheduling / Ordering

`rebuild_hierarchy_order` runs first in `pre_update`; all propagation systems follow it. Systems that only read `GlobalTransform` (renderer, audio) carry no explicit hierarchy dependency — the existing `pre_update` → `update` → render ordering already guarantees it is stable. Both traversal directions iterate the same `HierarchyOrder` Vec; reversed iteration costs nothing extra.

Systems affected by or dependent on the hierarchy, in scheduling order:

| System | Direction | Reads | Writes | When |
|---|---|---|---|---|
| `rebuild_hierarchy_order` | — | `Changed<Parent>` | `HierarchyOrder` | `pre_update` first |
| `propagate_transforms` | root → leaves | `LocalTransform`, parent `GlobalTransform` | `GlobalTransform` | `pre_update` after rebuild |
| `propagate_visibility` *(planned)* | root → leaves | parent `Visibility` | `Visibility` | `pre_update` after transforms |
| `update_bounding_volumes` *(planned)* | leaves → root | child `Aabb` | parent `Aabb` | `pre_update` after transforms |
| `despawn_cascade` | root → leaves | `Children` | despawn queue | `post_update` |
| Renderer | — | `GlobalTransform`, `Mesh` | — | after `pre_update` |

### Invariants

Two invariants must be enforced at hierarchy mutation sites, not at query time:

**No self-parenting.** Setting `Parent(e)` on entity `e` makes parent == child inside `HierarchyQuery::propagate_bfs`, causing `get_unchecked` to produce two aliasing mutable references — undefined behaviour. Rejected at insertion.

**No cycles.** A cycle (A parent of B, B parent of A) produces an infinite loop in traversal or an incorrect `HierarchyOrder`. Cycle detection on insertion is O(depth) — acceptable since hierarchy mutations are rare relative to frame rate. Both checks belong in the safe `set_parent(entity, parent, world)` wrapper; direct component insertion bypasses them.

### Performance

`HierarchyOrder` rebuild is O(n) over all entities with `Parent` or `Children` components. Static scenes pay this once at init; dynamic scenes with few structural changes pay it only on change frames. `propagate_bfs` iterates each hierarchical entity once per registered propagation system per frame — equivalent cost to a normal query iteration over the same set. `propagate_bfs_par` distributes each depth layer across rayon threads; the scheduling overhead makes this a net loss for layers under ~64 entities but scales well for large flat hierarchies (many entities at the same depth).

Pre-filling `HierarchyOrder` as `SmallVec` per layer avoids allocation for typical shallow hierarchies (depth < 8, branching < 8). Deep or wide hierarchies spill to heap.

## Key Types

| Type | Owns / Represents | Lifetime |
|---|---|---|
| `Parent(Entity)` | Reference to immediate parent entity | Component on entity; removed when entity leaves hierarchy |
| `Children(SmallVec<[Entity; 8]>)` | Ordered list of immediate child entities | Component on entity; updated by insertion wrapper |
| `HierarchyOrder` | `Vec<Vec<Entity>>` BFS depth layers over all hierarchical entities | ECS `Resource`; rebuilt on `Changed<Parent>` |
| `HierarchyQuery<Q>` | `Query<Q>` + `Res<HierarchyOrder>` + traversal methods | `SystemParam`; per-system-run lifetime |
| `LocalTransform` | Position, rotation, scale relative to parent | Component on entity; written by user or animation systems |
| `GlobalTransform` | World-space 4×4 matrix; derived by propagation | Component on entity; written only by `propagate_transforms` |

## Implementation

**New module** `src/component_system/hierarchy/` with:
- `Parent` and `Children` components
- `set_parent(entity, parent, &mut World)` safe wrapper enforcing no-self-parent and no-cycle; `remove_parent` keeping both sides consistent
- `HierarchyOrder` resource and `rebuild_hierarchy_order` system using `Changed<Parent>`
- `HierarchyQuery<Q>` `SystemParam` impl with `propagate_bfs`, `propagate_bfs_reversed`, `propagate_bfs_par`

**Transform changes** — `src/component_system/transform.rs`: `Transform` renamed to `LocalTransform`; `GlobalTransform` component added. For entities without `Parent`, `GlobalTransform` is initialised from `LocalTransform` directly. The render loop query switches from `Transform` to `GlobalTransform`. `propagate_transforms` registered in `pre_update` after `rebuild_hierarchy_order`.

**Despawn cascading** — new system in `post_update` that, when an entity with `Children` is despawned, recursively queues all descendants using `HierarchyOrder` in reverse (leaves first) to avoid dangling references during the same frame's despawn pass.

**Schedule wiring** — `pre_update` ordering: `rebuild_hierarchy_order` → `propagate_transforms` → (future: visibility, bounding volumes). `post_update`: despawn cascade. No changes to `update` or the render pass schedule.

**Unchanged** — mesh loading, buffer management, descriptor system, swapchain, frame synchronisation, all ECS component definitions outside `transform.rs`, rendering pipeline.
