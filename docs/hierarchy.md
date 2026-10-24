# Entity Hierarchy

## Glossary

**Entity** — an integer ID in the ECS; has no data of its own, only the components attached to it.  
**Component** — a plain data struct attached to an entity and stored in ECS column storage.  
**Query** — a Bevy system parameter that gives access to all entities matching a set of component types; borrows that storage for the duration of the system.  
**Archetype** — a distinct combination of component types; all entities with the same set live in the same table. Inserting or removing a component moves the entity to another table.  
**Root** — an entity without a `Parent`; the starting point of propagation.  
**Propagation** — computing a child's derived value from its parent's derived value, e.g. `child_global = parent_global * child_local`.  
**Frontier** — the list of entities processed at the current tree level; it holds each entity together with the value already computed for its parent.  
**Dirty** — an entity whose own input changed this frame (`Changed<LocalTransform>`, or reparented).  
**Marked** — an entity with at least one dirty descendant; it may itself be unchanged but must be descended into.  
**Aliasing** — two references to the same memory; Rust forbids a mutable reference coexisting with any other reference to the same data.  
**`Aabb`** — Axis-Aligned Bounding Box; a box around a mesh with sides parallel to the world axes, used for cheap culling and collision broad-phase.

## Overview

**Solution in short.** Entities form a tree through `Parent` and `Children` components. Every propagated value (world transform, inherited visibility, team) is defined by implementing one trait, `Propagate`, which states how a root value is formed and how a parent value combines with a child's local input. One generic system pair runs per implementation:

1. **Mark up**: every dirty entity walks its ancestors and marks them, stopping at the first one already marked.
2. **Propagate down**: start from the roots and process the tree one level at a time with two swapping queues. A child enters the next level only while its parent is being processed, so parents are always computed first. Clean, unmarked subtrees are never entered.

The frontier carries the parent's computed value by value, so a child never reads its parent's component. That removes the aliasing problem for the sequential pass without `unsafe`. Each level is a set of distinct entities and can be split across threads.

**Problems this solves:**
- **Ordering**: a child's world value is only correct if its parent's value for this frame was computed first. Without enforced order, a child reads last frame's parent value.
- **Rust aliasing**: the naive implementation reads `GlobalTransform` on the parent and writes `GlobalTransform` on the child through one `Query`. The compiler can't prove the two are different entities and rejects it.
- **Cost**: most of a scene is static each frame. Recomputing every entity every frame wastes work proportional to scene size, not to what moved.
- **Generality**: several components need the same root-to-leaves traversal. Bevy writes a separate traversal for each one (`propagate_transforms`, `visibility_propagate_system`). This design writes it once.

| Propagated value | Local input | Output | Rule |
|---|---|---|---|
| World transform | `LocalTransform` | `GlobalTransform` | `parent * local.to_matrix()` |
| Visibility | `Visibility` | `InheritedVisibility` | `parent && local` |
| Team | `Team` | `InheritedTeam` | local override, else parent |

## Design Problems

### Traversal order

The tree must be walked so that every parent finishes before any of its children. There are three ways to get that order:

- **Recursive DFS from roots** *(Bevy)*: the call stack gives the order. Skipping a clean subtree is free because you don't recurse into it. It's sequential.
- **Cached flat depth layers**: a resource holding `Vec<Vec<Entity>>` by depth. It parallelizes per layer, but a skipped subtree's nodes still sit in later layers and each must be checked individually. It also has to be rebuilt on every structural change.
- **Level-by-level frontier from roots** *(used)*: two queues, `current` and `next`. Processing `current` pushes qualifying children into `next`, then the queues swap. The order comes from construction, skipping is as cheap as DFS, and each level can run in parallel.

### Skipping unchanged work

Two different questions are asked per entity, and the answers flow in opposite directions:

- **Recompute?** Yes if the entity is dirty or its parent was recomputed. This flows down.
- **Descend?** Yes if the entity was recomputed or is marked. This flows up, so it must be prepared before the down pass.

Marking costs at most the depth per dirty entity, because each entity has one parent and the walk stops at an already-marked ancestor. A frame where only the player moved touches the player's ancestors and subtree, not the rest of the scene.

### Aliasing

| Approach | Safe | Cost |
|---|---|---|
| `ParamSet` read pass then write pass | Yes | Correct only per level; one pass pair per depth |
| Gather into `Vec`, release, scatter | Yes | Extra allocation and pass |
| `unsafe get_unchecked` on one query *(Bevy)* | Relies on parent ≠ child | `unsafe` in every propagation system |
| Parent value carried in the frontier *(used)* | Yes | 64-byte copy per entity for transforms |

With the parent value in the frontier, a propagation step only writes the current entity's output component, through a normal `get_mut`. `unsafe` comes back only for the parallel variant (see Limitations under Propagation Systems).

## Core Components

### Parent / Children

`Parent(Entity)` on every non-root, and `Children(Vec<Entity>)` on every entity with children. They hold topology only. Both sides must stay consistent, so all mutation goes through `set_parent` / `remove_parent`, which also reject self-parenting and cycles.

| Approach | Pros | Cons |
|---|---|---|
| Hierarchy inside `Transform` *(baseline)* | No extra components | Tied to one component; not reusable by other propagated values |
| `Parent` only, find children by scan | No sync | O(n) to find children |
| `Parent` + `Children` *(used)* | O(1) both directions | Two sides to keep in sync |
| Built-in `ChildOf` relationship (`bevy_ecs` ≥ 0.16) | Hooks keep both sides in sync automatically | Hides the mechanism this engine wants to own; revisit later |

**Limitations:** inserting `Parent` or `Children` directly bypasses the consistency and cycle checks.

### Propagate trait

One implementation per propagated value. It declares the local input component, the output component, the value carried down the frontier (`Mat4`, `bool`, `TeamId`), and three functions: form a root value from its local input, combine a parent value with a child's local input, and write a value into the output component.

| Approach | Pros | Cons |
|---|---|---|
| Hand-written traversal per system *(baseline, Bevy)* | Each tuned to its case | Traversal, skipping and parallelism duplicated |
| Cached order resource + closure `SystemParam` | Shared order | Rebuild cost; flat layers defeat skipping |
| Generic trait + generic systems *(used)* | Traversal written once; new value = one trait impl | Every value must fit the parent-combines-with-local shape |

**Limitations:** it only covers root → leaves propagation. Leaves → root aggregation (a parent `Aabb` as the union of its children) needs reverse level order. The down pass could record its levels and replay them reversed, but that's not designed yet.

### Propagation Systems

Two generic systems are registered per `Propagate` implementation:

- **`mark_dirty::<P>`** reads entities whose local input changed or whose `Parent` changed. It walks ancestors and inserts them into the `DirtyAncestors<P>` resource set. A resource set is used instead of a marker component so that marking never moves entities between archetypes.
- **`propagate::<P>`** seeds `current` with the roots. For each entry it recomputes if needed and writes the output, then pushes children into `next`: all of them if the entry was recomputed, otherwise only those that are dirty or marked. It swaps the queues and repeats until `current` is empty, then clears `DirtyAncestors<P>`.

Entities with no hierarchy at all are roots with no children, so they need no special case.

**Limitations:** the parallel variant writes many entities' outputs through one query from several threads. That needs `unsafe get_unchecked` or Bevy's unique-entity batch APIs. Its safety argument is that a tree never places an entity in a level twice. Rayon overhead outweighs the work for small levels, so the plan is to run levels under about 64 entities sequentially.

### LocalTransform / GlobalTransform

`LocalTransform` (position, rotation, scale relative to the parent) is what users and gameplay write. `GlobalTransform` (world-space `Mat4`) is written only by propagation and read by the renderer, physics and audio.

| Approach | Pros | Cons |
|---|---|---|
| Single world-space `Transform` *(baseline)* | One component | User must recompose on every parent move |
| Local + side-table `HashMap` for world | One user-facing component | Non-ECS data; extra lookup for every reader |
| `LocalTransform` + `GlobalTransform` *(used)* | Clear ownership; matches Bevy and Unity DOTS | Writers must pick the right one |

**Limitations:** writes to `GlobalTransform` are overwritten on the next recompute.

## Cross-Cutting Concerns

### Scheduling

In `post_update`: `mark_dirty::<P>` → `propagate::<P>` for each implementation. The frame runs `pre_update` → `fixed_update` → `update` → `post_update` → render; gameplay writes `LocalTransform` in `fixed_update` and `update`, so propagating any earlier would render children one frame behind their parents. Transforms run first, because visibility and bounding volumes may depend on them. `despawn_cascade` also runs in `post_update`. The renderer runs after `post_update` and needs no explicit dependency.

### Invariants

- **No self-parenting and no cycles.** A cycle makes propagation loop forever and makes the parallel safety argument false. Both are checked in `set_parent`, which costs O(depth).
- **Each entity appears in at most one `Children` list.** `set_parent` removes the child from its old parent first.

### Performance

`n` = hierarchy entities, `d` = depth, `k` = siblings, `c` = entities recomputed this frame, `m` = marked ancestors.

| Operation | Cost |
|---|---|
| `set_parent` | O(d + k): cycle check plus removal from old siblings |
| `is_child_of` | O(d) |
| `mark_dirty` | O(m) total, at most O(dirty × d) |
| `propagate` | O(c + m) entries visited; static scene ≈ O(roots) |
| Full rebuild (everything moved) | O(n), split across threads per level |

## Key Types

| Type | Owns / Represents | Lifetime |
|---|---|---|
| `Parent(Entity)` | Immediate parent | Component; removed when detached |
| `Children(Vec<Entity>)` | Immediate children | Component; maintained by `set_parent` |
| `Propagate` | Trait: local input, output, carried value, root/combine/write | One impl per propagated value |
| `DirtyAncestors<P>` | `HashSet<Entity>` of marked entities | Resource; filled by `mark_dirty`, cleared by `propagate` |
| `LocalTransform` | Position, rotation, scale relative to parent | Component; written by gameplay |
| `GlobalTransform` | World-space `Mat4` | Component; written only by propagation |

## Implementation

**Done** in `src/component_system/hierarchy.rs`: `Parent`, `Children`, `is_child_of` (walks ancestors through `&World`), and `set_parent` (self-parent and cycle rejection, old-parent cleanup, inserts missing components).

**Plan, in order:**
1. `remove_parent`, keeping both sides consistent.
2. Rename `Transform` to `LocalTransform` and add `GlobalTransform`. The render query switches to `GlobalTransform`.
3. `Propagate` trait and its transform implementation.
4. Sequential `mark_dirty::<P>` and `propagate::<P>` registered in `post_update`, verified on a parent cube with child cubes.
5. `despawn_cascade` in `post_update`.
6. Parallel levels behind the ~64 entity threshold.
7. Visibility implementation as the second user of the trait, to validate the generalization.

**Unchanged:** rendering pipeline, asset manager, descriptor system, swapchain, frame synchronisation.
