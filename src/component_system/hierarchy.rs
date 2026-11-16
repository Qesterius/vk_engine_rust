use bevy_ecs::{
    component::Component,
    entity::Entity,
    lifecycle::HookContext,
    world::{DeferredWorld, World},
};

/// Immediate parent of this entity; absent on roots.
/// The field is private so only `set_parent` / `remove_parent` can write it.
/// Removing the component any other way, despawn included, still unlinks the parent through `leave_parent`.
#[derive(Component)]
#[component(on_remove = leave_parent)]
pub struct Parent(Entity);

impl Parent {
    pub fn get(&self) -> Entity {
        self.0
    }
}

/// Remove hook of `Parent`: runs whenever `Parent` leaves an entity (`remove_parent`, a direct
/// `remove::<Parent>()`, or a despawn). The component is still readable here, so the hook knows
/// which parent's `Children` to clean. Overwriting `Parent` in place, as `set_parent` does, doesn't run it.
fn leave_parent(mut world: DeferredWorld, ctx: HookContext) {
    let Some(parent) = world.get::<Parent>(ctx.entity).map(Parent::get) else {
        return;
    };
    // `None` when the parent is already despawned; then there is no list to clean.
    if let Some(mut children) = world.get_mut::<Children>(parent) {
        children.0.retain(|&e| e != ctx.entity);
    }
}

/// Immediate children of this entity; kept in sync with `Parent` by `set_parent` / `remove_parent`.
/// The field is private so only those two functions can write it.
/// Despawning the entity despawns its children (`despawn_children`); removing the component detaches them (`detach_children`).
#[derive(Component)]
#[component(on_despawn = despawn_children, on_remove = detach_children)]
pub struct Children(Vec<Entity>);

/// Despawn hook of `Children`: despawns every child, and through the same hook on them, the whole subtree.
/// A hook can't add or remove entities, so the despawns are queued. The ECS applies them after this
/// entity is gone, so the subtree goes top-down and each child's `leave_parent` finds no parent to clean.
fn despawn_children(mut world: DeferredWorld, ctx: HookContext) {
    let children = children_of(&world, ctx.entity);
    let mut commands = world.commands();
    for child in children {
        commands.entity(child).try_despawn();
    }
}

/// Remove hook of `Children`: detaches every child when `Children` is removed directly, so none is left
/// pointing at a parent whose walk no longer reaches it. On despawn it runs after `despawn_children`,
/// so its queued detaches find the children already gone and do nothing (`try_` skips missing entities).
fn detach_children(mut world: DeferredWorld, ctx: HookContext) {
    let children = children_of(&world, ctx.entity);
    let mut commands = world.commands();
    for child in children {
        commands.entity(child).try_remove::<Parent>();
    }
}

/// Copies the list out, so the hook can then borrow the world mutably for its commands.
fn children_of(world: &DeferredWorld, entity: Entity) -> Vec<Entity> {
    world.get::<Children>(entity).map(|c| c.0.clone()).unwrap_or_default()
}

impl Children {
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

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

/// Attaches `child` under `new_parent`; ignored if either entity doesn't exist, or if it would create a self-loop or cycle.
pub fn set_parent(child: Entity, new_parent: Entity, world: &mut World) {
    // Gameplay can hold ids of despawned entities. Checking up front keeps a dead id from
    // failing halfway, after the child was already taken out of its old parent's `Children`.
    if world.get_entity(child).is_err() || world.get_entity(new_parent).is_err() {
        return;
    }
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

/// Detaches `child` from its parent, making it a root. `leave_parent` updates the parent's `Children`.
pub fn remove_parent(child: Entity, world: &mut World) {
    if let Ok(mut child) = world.get_entity_mut(child) {
        child.remove::<Parent>();
    }
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

    #[test]
    fn removing_parent_component_unlinks_both_sides() {
        // before: a
        //         └── b
        // try:    remove Parent from b directly, bypassing remove_parent
        // after:  a   b   (the remove hook still cleans a's Children)
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        world.entity_mut(b).remove::<Parent>();

        assert_eq!(get_parent(b, &world), None);
        assert_eq!(get_children(a, &world), Some(&[][..]));
    }

    #[test]
    fn despawning_child_unlinks_parent() {
        // before: a
        //         └── b
        // after:  a   (b despawned, no dead entry left in a's Children)
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        world.despawn(b);

        assert_eq!(get_children(a, &world), Some(&[][..]));
    }

    #[test]
    fn set_parent_with_despawned_entity_is_ignored() {
        // before: a
        //         └── b        x (despawned)
        // try:    b under x, then x under a
        // after:  unchanged, no panic
        let mut world = World::new();
        let [a, b, x] = spawn(&mut world);
        set_parent(b, a, &mut world);
        world.despawn(x);

        set_parent(b, x, &mut world);
        set_parent(x, a, &mut world);

        assert_eq!(get_parent(b, &world), Some(a));
        assert_eq!(get_children(a, &world), Some(&[b][..]));
    }

    #[test]
    fn despawning_parent_despawns_subtree() {
        // before: a
        //         └── b
        //             └── c
        // after:  nothing left
        let mut world = World::new();
        let [a, b, c] = spawn(&mut world);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);

        world.despawn(a);

        assert!(world.get_entity(b).is_err());
        assert!(world.get_entity(c).is_err());
    }

    #[test]
    fn despawning_inner_entity_keeps_rest_of_tree_linked() {
        // before: a
        //         ├── b
        //         │   └── c
        //         └── d
        // despawn b
        // after:  a
        //         └── d   (c went with b; a lists only d)
        let mut world = World::new();
        let [a, b, c, d] = spawn(&mut world);
        set_parent(b, a, &mut world);
        set_parent(c, b, &mut world);
        set_parent(d, a, &mut world);

        world.despawn(b);

        assert!(world.get_entity(c).is_err());
        assert_eq!(get_children(a, &world), Some(&[d][..]));
        assert_eq!(get_parent(d, &world), Some(a));
    }

    #[test]
    fn despawning_through_commands_despawns_subtree() {
        // gameplay systems despawn through Commands; the hooks run when the command is applied
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        world.commands().entity(a).despawn();
        world.flush();

        assert!(world.get_entity(b).is_err());
    }

    #[test]
    fn removing_children_component_detaches_children() {
        // before: a
        //         └── b
        // try:    remove Children from a directly
        // after:  a   b   (b is a root and still alive)
        let mut world = World::new();
        let [a, b] = spawn(&mut world);
        set_parent(b, a, &mut world);

        world.entity_mut(a).remove::<Children>();

        assert!(world.get_entity(b).is_ok());
        assert_eq!(get_parent(b, &world), None);
    }
}
