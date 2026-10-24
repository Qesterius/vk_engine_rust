
//1. in update local transfors change, this adds them to dirty set
//2. dirty set is processed by marking whole parent chain as dirty untill reaching already marked parent or root. Not really. lets say we have a with 2 children: b and c. b has child d. If we modify b, then d should be recomputed, but c should not. We need from child to root traversal to mark the branches that need recomputation, but we need as well to mark children or!! we mark parents with different value. has dirty descendant. Not is dirty. 
// ok instead of Dirty we use Changed in query
//3. in propagate we go from root over dirty nodes

// Dirtyness: has dirty descendant, and is dirty. 
// should it be implemented as a component? or as a resource? or as a system local state?
// My intuition is that we have component, but if we keep this info on world, whereas its only Transform related value. 
// what if we implemented a trait and add it to Transform. 

// A) in 2nd poiint we either mark all parents as has dirty descendant and mark all children as dirty or when we reach a dirty node in propagate we just propagate until the end of the branch. I like the 2nd option more as its faster and simpler.
// B) isnt there any alternative to dirty descendant? can it be done with just dirty?

//in propagate we start at root. 

use bevy_ecs::{
    entity::{Entity, EntityHashSet},
    lifecycle::RemovedComponents,
    query::{Changed, Or},
    system::Query,
};

use crate::component_system::{
    hierarchy::{Children, Parent},
    transform::{GlobalTransform, LocalTransform},
};

// TODO: generalize over a `Propagate` trait (own value + inherited result + combine) so visibility,
// team and other inherited values reuse this traversal. Currently it only propagates transforms.

/// Recomputes `GlobalTransform` for every dirty entity and its whole subtree; parents always before children.
///
/// An entity is dirty when one of the two inputs of `global = parent_global * local` changed for it:
/// its own `LocalTransform`, or which entity its parent is (reparented, or detached and now a root).
pub fn propagate(
    // `Or<(A, B)>` matches entities passing either filter. `Changed<Parent>` also fires when
    // `Parent` is first inserted, so a fresh `set_parent` counts as well as a reparent.
    changed: Query<Entity, Or<(Changed<LocalTransform>, Changed<Parent>)>>,
    // `remove_parent` deletes the `Parent` component, and a missing component can't be `Changed`.
    // `RemovedComponents` is Bevy's log of removals since this system last ran.
    mut removed_parents: RemovedComponents<Parent>,
    parents: Query<&Parent>,
    nodes: Query<(&LocalTransform, Option<&Children>)>,
    mut globals: Query<&mut GlobalTransform>,
) {
    // Read the removal log once into a set. `read()` consumes the log, so it can't be iterated twice,
    // and a set gives O(1) `contains` for the ancestor checks below.
    let detached: EntityHashSet = removed_parents.read().collect();

    // One closure answering "is this entity dirty?" for both sources.
    // It borrows `changed` and `detached` (read-only) and is passed to the ancestor check.
    let is_dirty = |entity: Entity| changed.contains(entity) || detached.contains(&entity);

    // filter out all changed entities that have a changed ancestor. Later when we run from all changed roots we will recompute all children once. 
    // example tree
    // a
    // └── b
    //     └── c
    // if b and c are changed, we keep in changed_roots only b, because c will be recomputed when we process b.
    //
    // Each stack entry is a pair: (entity to recompute, its parent's global for this frame).
    // Carrying the parent's value means a child never has to look its parent up in the ECS,
    // which is a random memory access per entity. Only the seeds below need that lookup.
    let mut stack: Vec<(Entity, GlobalTransform)> = changed
        .iter()
        // Add detached entities as candidates too. Skip ones already in `changed`: an entity
        // detached and reattached in the same frame would otherwise be seeded twice.
        .chain(detached.iter().copied().filter(|&entity| !changed.contains(entity)))
        // The ancestor check must use the same notion of dirty. Otherwise a child of a detached
        // entity could be seeded on its own and computed from the detached entity's stale global.
        .filter(|&entity| !has_dirty_ancestor(entity, &parents, &is_dirty))
        // A seed's parent did not change (otherwise the seed would have been filtered out),
        // so the parent's stored global is already correct for this frame and can be read.
        .map(|seed| (seed, copy_parent_global(seed, &parents, &globals)))
        .collect();

    // Recompute every seed and everything below it.
    // `pop` takes from the end, so this is depth-first. Order is still correct:
    // a child is pushed only while its parent is being processed, so it can't come first.
    while let Some((entity, parent_global)) = stack.pop() {
        let Ok((local, children)) = nodes.get(entity) else {
            continue;
        };
        let Ok(mut global) = globals.get_mut(entity) else {
            continue;
        };

        // Compute once, then use the value twice: store it, and hand it to the children.
        // `GlobalTransform` is `Copy`, so `new_global` stays usable after the assignment.
        let new_global = parent_global * local;
        *global = new_global;

        if let Some(children) = children {
            // `map` turns each child id into a (child, this entity's new global) pair.
            stack.extend(children.0.iter().map(|&child| (child, new_global)));
        }
    }
}

/// The parent's current global, or the identity for a root (so root global = root local).
fn copy_parent_global(
    entity: Entity,
    parents: &Query<&Parent>,
    globals: &Query<&mut GlobalTransform>,
) -> GlobalTransform {
    match parents.get(entity) {
        // `.copied()` copies the value out of the query, so no borrow is held afterwards.
        Ok(parent) => globals.get(parent.0).copied().unwrap_or_default(),
        Err(_) => GlobalTransform::default(),
    }
}

/// Walks up the parent chain and reports whether any ancestor is dirty.
/// `is_dirty: &impl Fn(Entity) -> bool` accepts any function or closure taking an entity and returning bool.
fn has_dirty_ancestor(
    mut entity: Entity,
    parents: &Query<&Parent>,
    is_dirty: &impl Fn(Entity) -> bool,
) -> bool {
    while let Ok(parent) = parents.get(entity) {
        if is_dirty(parent.0) {
            return true;
        }
        entity = parent.0;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_system::hierarchy::{remove_parent, set_parent};
    use bevy_ecs::{change_detection::DetectChanges, schedule::Schedule, world::World};
    use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, Vector3};

    fn setup() -> (World, Schedule) {
        let mut schedule = Schedule::default();
        schedule.add_systems(propagate);
        (World::new(), schedule)
    }

    fn node(world: &mut World, position: [f32; 3]) -> Entity {
        world.spawn(LocalTransform::new(position)).id()
    }

    /// One frame; clearing trackers afterwards makes `is_changed` mean "changed this frame".
    fn frame(world: &mut World, schedule: &mut Schedule) {
        schedule.run(world);
        world.clear_trackers();
    }

    fn world_position(world: &World, entity: Entity) -> Vector3<f32> {
        world.get::<GlobalTransform>(entity).unwrap().translation()
    }

    fn assert_near(actual: Vector3<f32>, expected: [f32; 3]) {
        let expected = Vector3::from(expected);
        assert!(
            (actual - expected).magnitude2() < 1e-8,
            "expected {expected:?}, got {actual:?}"
        );
    }

    fn written_this_frame(world: &World, entity: Entity) -> bool {
        world.entity(entity).get_ref::<GlobalTransform>().unwrap().is_changed()
    }

    #[test]
    fn chain_adds_offsets() {
        // a(10,0,0)
        // └── b(1,0,0)
        //     └── c(0,2,0)
        // result: a = (10,0,0), b = (11,0,0), c = (11,2,0)
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [1.0, 0.0, 0.0]);
        let c = node(&mut world, [0.0, 2.0, 0.0]);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);

        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, a), [10.0, 0.0, 0.0]);
        assert_near(world_position(&world, b), [11.0, 0.0, 0.0]);
        assert_near(world_position(&world, c), [11.0, 2.0, 0.0]);
    }

    #[test]
    fn parent_rotation_rotates_child_offset() {
        // a(10,0,0) rotated 90° around y
        // └── b(1,0,0)   "1 to a's right"
        // result: b = (10,0,-1), because a's right now points along -z
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        world.get_mut::<LocalTransform>(a).unwrap().rotation = Quaternion::from_angle_y(Deg(90.0));
        let b = node(&mut world, [1.0, 0.0, 0.0]);
        set_parent(b, a, &mut world);

        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, b), [10.0, 0.0, -1.0]);
    }

    #[test]
    fn moving_parent_moves_unchanged_child() {
        // a(10 → 20,0,0)
        // └── b(1,0,0)
        // result: b = (21,0,0) although b itself did not change
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [1.0, 0.0, 0.0]);
        set_parent(b, a, &mut world);
        frame(&mut world, &mut schedule);

        world.get_mut::<LocalTransform>(a).unwrap().position.x = 20.0;
        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, b), [21.0, 0.0, 0.0]);
    }

    #[test]
    fn moving_child_does_not_touch_parent_or_sibling() {
        // a(10,0,0)
        // ├── b(1 → 2,0,0)   moved
        // └── x(5,0,0)       static
        // result: b = (12,0,0) and written; a and x not written
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [1.0, 0.0, 0.0]);
        let x = node(&mut world, [5.0, 0.0, 0.0]);
        set_parent(b, a, &mut world);
        set_parent(x, a, &mut world);
        frame(&mut world, &mut schedule);

        world.get_mut::<LocalTransform>(b).unwrap().position.x = 2.0;
        schedule.run(&mut world);

        assert_near(world_position(&world, b), [12.0, 0.0, 0.0]);
        assert!(written_this_frame(&world, b));
        assert!(!written_this_frame(&world, a));
        assert!(!written_this_frame(&world, x));
    }

    #[test]
    fn parent_and_child_both_changed_uses_new_parent_value() {
        // a(10 → 20,0,0)   moved
        // └── b(1 → 2,0,0) moved too
        // result: b = (22,0,0); b must not be computed from a's old value
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [1.0, 0.0, 0.0]);
        set_parent(b, a, &mut world);
        frame(&mut world, &mut schedule);

        world.get_mut::<LocalTransform>(a).unwrap().position.x = 20.0;
        world.get_mut::<LocalTransform>(b).unwrap().position.x = 2.0;
        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, b), [22.0, 0.0, 0.0]);
    }

    #[test]
    fn reparented_child_follows_new_parent() {
        // before: a(10,0,0)        b(100,0,0)
        //         └── c(1,0,0)
        // after:  a(10,0,0)        b(100,0,0)
        //                          └── c(1,0,0)
        // result: c = (101,0,0) although c's own LocalTransform was not touched
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [100.0, 0.0, 0.0]);
        let c = node(&mut world, [1.0, 0.0, 0.0]);
        set_parent(c, a, &mut world);
        frame(&mut world, &mut schedule);

        set_parent(c, b, &mut world);
        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, c), [101.0, 0.0, 0.0]);
    }

    #[test]
    fn reparent_moves_whole_subtree() {
        // before: a(10,0,0)        b(100,0,0)
        //         └── c(1,0,0)
        //             └── d(0,2,0)
        // after:  c (with d) moved under b
        // result: c = (101,0,0), d = (101,2,0); d itself changed nothing
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let b = node(&mut world, [100.0, 0.0, 0.0]);
        let c = node(&mut world, [1.0, 0.0, 0.0]);
        let d = node(&mut world, [0.0, 2.0, 0.0]);
        set_parent(c, a, &mut world);
        set_parent(d, c, &mut world);
        frame(&mut world, &mut schedule);

        set_parent(c, b, &mut world);
        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, d), [101.0, 2.0, 0.0]);
    }

    #[test]
    fn detached_child_becomes_root() {
        // before: a(10,0,0)
        //         └── c(1,0,0)
        // after:  a(10,0,0)   c(1,0,0)
        // result: c = (1,0,0), its own local only
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, [10.0, 0.0, 0.0]);
        let c = node(&mut world, [1.0, 0.0, 0.0]);
        set_parent(c, a, &mut world);
        frame(&mut world, &mut schedule);

        remove_parent(c, &mut world);
        frame(&mut world, &mut schedule);

        assert_near(world_position(&world, c), [1.0, 0.0, 0.0]);
    }
}