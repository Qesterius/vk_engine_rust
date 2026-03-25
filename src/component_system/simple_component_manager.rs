use crate::rendering::{cleanup::DeletionQueue, components::mesh::Mesh};
use super::transform::Transform;
use anyhow::{Ok, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entity(usize);

pub struct ComponentManager {
    pub transforms: Vec<Option<Transform>>,
    pub meshes: Vec<Option<Mesh>>,
    pub deletion_queue: DeletionQueue,
}

impl ComponentManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            transforms: Vec::new(),
            meshes: Vec::new(),
            deletion_queue: DeletionQueue::new(),
        })
    }

    pub fn create_entity(&mut self) -> Entity {
        let id = self.transforms.len();
        self.transforms.push(None);
        self.meshes.push(None);
        Entity(id)
    }

    pub fn add_mesh(&mut self, entity: Entity, mesh: Mesh) {
        self.meshes[entity.0] = Some(mesh);
    }

    pub fn add_transform(&mut self, entity: Entity, transform: Transform) {
        self.transforms[entity.0] = Some(transform);
    }
}

impl Drop for ComponentManager {
    fn drop(&mut self) {
        self.deletion_queue.flush();
    }
}
