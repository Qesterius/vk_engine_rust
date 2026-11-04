use std::marker::PhantomData;

use bevy_ecs::{
    change_detection::{DetectChanges, Ref, ResMut},
    component::{Component, Mutable},
    entity::{Entity, EntityHashSet},
    lifecycle::RemovedComponents,
    query::{Changed, Or, With},
    resource::Resource,
    schedule::{IntoScheduleConfigs, Schedule},
    system::{Local, Query},
    world::World,
};

use crate::component_system::hierarchy::{Children, Parent};

/// A value every entity sets for itself (`Own`) and that resolves against its ancestors into `Inherited`.
pub trait Propagate: Send + Sync + 'static {
    /// What the entity sets about itself, relative to its parent; written by gameplay, only read here.
    type Own: Component;
    /// The resolved result after inheriting from all ancestors; written only by propagation.
    type Inherited: Component<Mutability = Mutable> + Copy;

    /// Result for an entity with no parent.
    fn root(own: &Self::Own) -> Self::Inherited;
    /// Result for a child, from its parent's result and its own setting.
    fn combine(parent: &Self::Inherited, own: &Self::Own) -> Self::Inherited;
}

/// Entities to visit this frame for `P`: dirty ones plus every ancestor of a dirty one.
#[derive(Resource)]
pub struct DirtyAncestors<P: Propagate> {
    // invariant: if an entity is marked, all of its ancestors are marked too
    marked: EntityHashSet,
    // roots that lost their parent this frame; `Changed<Parent>` can't see them
    detached: EntityHashSet,
    _marker: PhantomData<P>,
}

impl<P: Propagate> Default for DirtyAncestors<P> {
    fn default() -> Self {
        Self {
            marked: EntityHashSet::default(),
            detached: EntityHashSet::default(),
            _marker: PhantomData,
        }
    }
}

/// (entity, parent's result or `None` for roots, whether the parent was recomputed)
type Entry<P> = (Entity, Option<<P as Propagate>::Inherited>, bool);

/// The two level queues, kept between frames so they don't reallocate.
pub struct Frontier<P: Propagate> {
    current: Vec<Entry<P>>,
    next: Vec<Entry<P>>,
}

impl<P: Propagate> Default for Frontier<P> {
    fn default() -> Self {
        Self { current: Vec::new(), next: Vec::new() }
    }
}

/// Registers propagation of `P` into `schedule`; marking always runs before the down pass.
pub fn add_propagation<P: Propagate>(world: &mut World, schedule: &mut Schedule) {
    world.init_resource::<DirtyAncestors<P>>();
    schedule.add_systems((mark_dirty::<P>, propagate::<P>).chain());
}

/// Marks every entity whose own value or parent changed, plus all of its ancestors.
fn mark_dirty<P: Propagate>(
    changed: Query<Entity, (With<P::Own>, Or<(Changed<P::Own>, Changed<Parent>)>)>,
    mut removed_parents: RemovedComponents<Parent>,
    parents: Query<&Parent>,
    mut dirty: ResMut<DirtyAncestors<P>>,
) {
    let dirty = &mut *dirty;
    for entity in removed_parents.read() {
        dirty.detached.insert(entity);
    }

    for start in changed.iter().chain(dirty.detached.iter().copied()) {
        let mut current = start;
        // stop at the first already-marked entity: its ancestors are marked by the invariant
        while dirty.marked.insert(current) {
            match parents.get(current) {
                Ok(parent) => current = parent.0,
                Err(_) => break,
            }
        }
    }
}

/// Walks marked trees level by level from their roots, recomputing only what changed.
/// Entities without `P::Own` stop the walk, so their subtree is not propagated.
fn propagate<P: Propagate>(
    nodes: Query<(Ref<P::Own>, Option<Ref<Parent>>, Option<&Children>)>,
    mut inherited: Query<&mut P::Inherited>,
    mut dirty: ResMut<DirtyAncestors<P>>,
    mut frontier: Local<Frontier<P>>,
) {
    if dirty.marked.is_empty() {
        return;
    }
    let Frontier { current, next } = &mut *frontier;

    // marked entities without a parent are exactly the roots of trees that contain changes
    for &entity in &dirty.marked {
        if let Ok((_, None, _)) = nodes.get(entity) {
            current.push((entity, None, false));
        }
    }

    while !current.is_empty() {
        for (entity, parent_value, parent_recomputed) in current.drain(..) {
            let Ok((own, parent, children)) = nodes.get(entity) else {
                continue;
            };
            let Ok(mut out) = inherited.get_mut(entity) else {
                continue;
            };

            let recompute = parent_recomputed
                || own.is_changed()
                || parent.is_some_and(|p| p.is_changed())
                || dirty.detached.contains(&entity);

            let value = if recompute {
                let value = match &parent_value {
                    Some(parent_value) => P::combine(parent_value, &own),
                    None => P::root(&own),
                };
                *out = value;
                value
            } else {
                // unchanged but marked: a descendant still needs this entity's current result
                *out
            };

            let Some(children) = children else {
                continue;
            };
            for &child in &children.0 {
                if recompute || dirty.marked.contains(&child) {
                    next.push((child, Some(value), recompute));
                }
            }
        }
        std::mem::swap(current, next);
    }

    dirty.marked.clear();
    dirty.detached.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_system::hierarchy::{remove_parent, set_parent};

    /// 1-D stand-in for a transform: a position relative to the parent.
    #[derive(Component)]
    struct Offset(f32);

    /// Resolved world position: sum of offsets from the root down.
    #[derive(Component, Clone, Copy, Default, Debug, PartialEq)]
    struct WorldOffset(f32);

    struct OffsetPropagation;

    impl Propagate for OffsetPropagation {
        type Own = Offset;
        type Inherited = WorldOffset;

        fn root(own: &Offset) -> WorldOffset {
            WorldOffset(own.0)
        }

        fn combine(parent: &WorldOffset, own: &Offset) -> WorldOffset {
            WorldOffset(parent.0 + own.0)
        }
    }

    fn setup() -> (World, Schedule) {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        add_propagation::<OffsetPropagation>(&mut world, &mut schedule);
        (world, schedule)
    }

    fn node(world: &mut World, offset: f32) -> Entity {
        world.spawn((Offset(offset), WorldOffset::default())).id()
    }

    /// One frame; clearing trackers afterwards makes `is_changed` mean "changed this frame".
    fn frame(world: &mut World, schedule: &mut Schedule) {
        schedule.run(world);
        world.clear_trackers();
    }

    fn world_offset(world: &World, entity: Entity) -> f32 {
        world.get::<WorldOffset>(entity).unwrap().0
    }

    fn written_this_frame(world: &World, entity: Entity) -> bool {
        world.entity(entity).get_ref::<WorldOffset>().unwrap().is_changed()
    }

    fn move_to(world: &mut World, entity: Entity, offset: f32) {
        world.get_mut::<Offset>(entity).unwrap().0 = offset;
    }

    #[test]
    fn chain_sums_offsets() {
        // a(10)
        // └── b(1)
        //     └── c(2)
        // result: a = 10, b = 11, c = 13
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let b = node(&mut world, 1.0);
        let c = node(&mut world, 2.0);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);

        frame(&mut world, &mut schedule);

        assert_eq!(world_offset(&world, a), 10.0);
        assert_eq!(world_offset(&world, b), 11.0);
        assert_eq!(world_offset(&world, c), 13.0);
    }

    #[test]
    fn moving_parent_moves_descendants() {
        // a(10 → 12)
        // └── b(1)
        //     └── c(2)
        // result: b = 13, c = 15 although only a changed
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let b = node(&mut world, 1.0);
        let c = node(&mut world, 2.0);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);
        frame(&mut world, &mut schedule);

        move_to(&mut world, a, 12.0);
        frame(&mut world, &mut schedule);

        assert_eq!(world_offset(&world, b), 13.0);
        assert_eq!(world_offset(&world, c), 15.0);
    }

    #[test]
    fn static_sibling_is_not_written() {
        // a(10)
        // ├── b(1 → 2)   moved
        // └── x(5)       static
        // result: b written (12), x and a untouched
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let b = node(&mut world, 1.0);
        let x = node(&mut world, 5.0);
        set_parent(b, a, &mut world);
        set_parent(x, a, &mut world);
        frame(&mut world, &mut schedule);

        move_to(&mut world, b, 2.0);
        schedule.run(&mut world);

        assert_eq!(world_offset(&world, b), 12.0);
        assert!(written_this_frame(&world, b));
        assert!(!written_this_frame(&world, a));
        assert!(!written_this_frame(&world, x));
    }

    #[test]
    fn unchanged_ancestors_are_read_not_recomputed() {
        // a(10)
        // └── b(1)
        //     └── c(2 → 5)   only c moved
        // result: c = 16 using a's and b's stored results; a and b untouched
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let b = node(&mut world, 1.0);
        let c = node(&mut world, 2.0);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);
        frame(&mut world, &mut schedule);

        move_to(&mut world, c, 5.0);
        schedule.run(&mut world);

        assert_eq!(world_offset(&world, c), 16.0);
        assert!(!written_this_frame(&world, a));
        assert!(!written_this_frame(&world, b));
    }

    #[test]
    fn untouched_tree_is_skipped() {
        // a(10)          b(100)
        // └── c(1)       └── d(1 → 2)   only d moved
        // result: a and c untouched, d = 102
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let c = node(&mut world, 1.0);
        let b = node(&mut world, 100.0);
        let d = node(&mut world, 1.0);
        set_parent(c, a, &mut world);
        set_parent(d, b, &mut world);
        frame(&mut world, &mut schedule);

        move_to(&mut world, d, 2.0);
        schedule.run(&mut world);

        assert_eq!(world_offset(&world, d), 102.0);
        assert!(!written_this_frame(&world, a));
        assert!(!written_this_frame(&world, c));
    }

    #[test]
    fn reparent_follows_new_parent() {
        // before: a(10)          b(100)
        //         └── c(1)
        // after:  a(10)          b(100)
        //                        └── c(1)
        // result: c = 101 although its own offset did not change
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let b = node(&mut world, 100.0);
        let c = node(&mut world, 1.0);
        set_parent(c, a, &mut world);
        frame(&mut world, &mut schedule);

        set_parent(c, b, &mut world);
        frame(&mut world, &mut schedule);

        assert_eq!(world_offset(&world, c), 101.0);
    }

    #[test]
    fn detached_child_becomes_root() {
        // before: a(10)
        //         └── c(1)
        // after:  a(10)   c(1)
        // result: c = 1, its own offset only
        let (mut world, mut schedule) = setup();
        let a = node(&mut world, 10.0);
        let c = node(&mut world, 1.0);
        set_parent(c, a, &mut world);
        frame(&mut world, &mut schedule);
        assert_eq!(world_offset(&world, c), 11.0);

        remove_parent(c, &mut world);
        frame(&mut world, &mut schedule);

        assert_eq!(world_offset(&world, c), 1.0);
    }
}
