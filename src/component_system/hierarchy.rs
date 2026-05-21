use bevy_ecs::{component::Component, entity::Entity, world::World};

#[derive(Component)]
pub struct Parent(pub Entity);

#[derive(Component, Default)]
pub struct Children(pub Vec<Entity>);

pub fn is_child_of(entity: Entity, ancestor: Entity, world: &World) -> bool {
    let mut current = entity;
    while let Some(p) = world.get::<Parent>(current) {
        if p.0 == ancestor {
            return true;
        }
        current = p.0;
    }
    false
}

pub fn set_parent(child: Entity, new_parent: Entity, world: &mut World) {
    if child == new_parent {
        return;
    }
    if is_child_of(new_parent, child, world) {
        return;
    }

    // remove child from old parent's Children list
    if let Some(old_parent) = world.get::<Parent>(child).map(|p| p.0) {
        if let Some(mut old_children) = world.get_mut::<Children>(old_parent) {
            old_children.0.retain(|&e| e != child);
        }
    }

    // set or insert Parent on child
    if let Some(mut parent_comp) = world.get_mut::<Parent>(child) {
        parent_comp.0 = new_parent;
    } else {
        world.entity_mut(child).insert(Parent(new_parent));
    }

    // add child to new parent's Children, inserting the component if missing
    if let Some(mut children) = world.get_mut::<Children>(new_parent) {
        children.0.push(child);
    } else {
        world.entity_mut(new_parent).insert(Children(vec![child]));
    }
}
