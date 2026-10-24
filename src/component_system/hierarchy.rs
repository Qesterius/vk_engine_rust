use bevy_ecs::{component::Component, entity::Entity, world::World};

/// Immediate parent of this entity; absent on roots.
#[derive(Component)]
pub struct Parent(pub Entity);

/// Immediate children of this entity; kept in sync with `Parent` by `set_parent` / `remove_parent`.
#[derive(Component, Default)]
pub struct Children(pub Vec<Entity>);

/// Returns the immediate parent, or `None` for a root.
pub fn get_parent(child: Entity, world: &World) -> Option<Entity> {
    world.get::<Parent>(child).map(|p| p.0)
}

/// Returns the immediate children, or `None` if the entity has never had any.
pub fn get_children(parent: Entity, world: &World) -> Option<&[Entity]> {
    world.get::<Children>(parent).map(|c| c.0.as_slice())
}

/// Returns true if `parent` is anywhere above `child` in the tree.
pub fn is_child_of(child: Entity, parent: Entity, world: &World) -> bool {
    let mut current = child;
    while let Some(p) = get_parent(current, world) {
        if p == parent {
            return true;
        }
        current = p;
    }
    false
}

/// Attaches `child` under `new_parent`; ignored if it would create a self-loop or cycle.
pub fn set_parent(child: Entity, new_parent: Entity, world: &mut World) {
    if child == new_parent || is_child_of(new_parent, child, world) {
        return;
    }

    if let Some(old_parent) = get_parent(child, world) {
        if let Some(mut old_children) = world.get_mut::<Children>(old_parent) {
            old_children.0.retain(|&e| e != child);
        }
    }

    // overwrite in place when possible to avoid an archetype move
    if let Some(mut parent) = world.get_mut::<Parent>(child) {
        parent.0 = new_parent;
    } else {
        world.entity_mut(child).insert(Parent(new_parent));
    }

    if let Some(mut children) = world.get_mut::<Children>(new_parent) {
        children.0.push(child);
    } else {
        world.entity_mut(new_parent).insert(Children(vec![child]));
    }
}

/// Detaches `child` from its parent, making it a root.
pub fn remove_parent(child: Entity, world: &mut World) {
    let Some(parent) = get_parent(child, world) else {
        return;
    };
    if let Some(mut children) = world.get_mut::<Children>(parent) {
        children.0.retain(|&e| e != child);
    }
    world.entity_mut(child).remove::<Parent>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn<const N: usize>(world: &mut World) -> [Entity; N] {
        std::array::from_fn(|_| world.spawn_empty().id())
    }

    #[test]
    fn set_parent_links_both_sides() {
        // before: a   b
        // after:  a
        //         └── b
        let mut world = World::new();
        let [a, b] = spawn(&mut world);

        set_parent(b, a, &mut world);

        assert_eq!(get_parent(b, &world), Some(a));
        assert_eq!(get_children(a, &world), Some(&[b][..]));
    }

    #[test]
    fn is_child_of_walks_whole_chain() {
        // a
        // └── b
        //     └── c
        // c is under b and a; nothing is under itself; a is not under c
        let mut world = World::new();
        let [a, b, c] = spawn(&mut world);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);

        assert!(is_child_of(c, a, &world));
        assert!(is_child_of(c, b, &world));
        assert!(!is_child_of(a, c, &world));
        assert!(!is_child_of(a, a, &world));
    }

    #[test]
    fn self_parent_rejected() {
        // before: a
        // try:    a -> a
        // after:  a   (unchanged, no Parent or Children inserted)
        let mut world = World::new();
        let [a] = spawn(&mut world);

        set_parent(a, a, &mut world);

        assert_eq!(get_parent(a, &world), None);
        assert_eq!(get_children(a, &world), None);
    }

    #[test]
    fn cycle_rejected() {
        // before: a
        //         └── b
        //             └── c
        // try:    a under c  (would close the loop a -> b -> c -> a)
        // after:  unchanged
        let mut world = World::new();
        let [a, b, c] = spawn(&mut world);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);

        set_parent(a, c, &mut world);

        assert_eq!(get_parent(a, &world), None);
        assert_eq!(get_children(c, &world), None);
    }

    #[test]
    fn reparent_cleans_old_parent() {
        // before: a       b
        //         └── c
        // after:  a       b
        //                 └── c
        // a keeps an empty Children list rather than losing the component
        let mut world = World::new();
        let [a, b, c] = spawn(&mut world);
        set_parent(c, a, &mut world);

        set_parent(c, b, &mut world);

        assert_eq!(get_parent(c, &world), Some(b));
        assert_eq!(get_children(a, &world), Some(&[][..]));
        assert_eq!(get_children(b, &world), Some(&[c][..]));
    }

    #[test]
    fn reparent_to_same_parent_keeps_single_entry() {
        // before: a
        //         └── b
        // try:    b under a again
        // after:  a
        //         └── b   (b listed once, not twice)
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        set_parent(b, a, &mut world);

        assert_eq!(get_children(a, &world), Some(&[b][..]));
    }

    #[test]
    fn remove_parent_unlinks_both_sides() {
        // before: a
        //         └── b
        // after:  a   b   (both roots; a keeps an empty Children list)
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        remove_parent(b, &mut world);

        assert_eq!(get_parent(b, &world), None);
        assert_eq!(get_children(a, &world), Some(&[][..]));
    }

    #[test]
    fn remove_parent_on_root_is_noop() {
        // before: a   (root)
        // after:  a   (unchanged)
        let mut world = World::new();
        let [a] = spawn(&mut world);

        remove_parent(a, &mut world);

        assert_eq!(get_parent(a, &world), None);
    }
}
