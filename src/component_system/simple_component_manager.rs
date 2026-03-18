use crate::rendering::components::mesh::Mesh;
use super::transform::Transform;
use anyhow::{Ok, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entity(usize);

pub struct ComponentManager{
    pub transforms: Vec<Option<Transform>>,
    pub meshes: Vec<Option<Mesh>>
    //holds all entities
}
impl ComponentManager{
    pub fn new() -> Result<Self>{
        Ok(Self { transforms: Vec::new(), meshes: Vec::new() })
    }

    //get all entities
    //create entity
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

    //get all component meshes
    //get component with id
    // do we need seperate deletion queue for component manager? what about cleanup here
    pub fn destroy(&mut self){}
}