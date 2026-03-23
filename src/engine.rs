use crate::{component_system::{simple_component_manager::{self, ComponentManager}, transform::Transform}, rendering::{components::mesh::Mesh, rendering_state::{self, RenderingState}}};
use anyhow::{Ok, Result};
use log::info;
use winit::window::Window;
use cgmath::{Quaternion, Rotation3, Vector3};



pub struct Engine {
    pub rendering_state: RenderingState,
    pub component_manager: ComponentManager
}

impl Engine{
    pub fn new(window:Window) -> Result<Self>{
        let mut rendering_state = unsafe { RenderingState::new(window)? };
        let mut component_manager = ComponentManager::new()?;
        

        for i in 0..10{
            let simple_cube_mesh = Mesh::new_cube( //TODO: create asset manager and move this there
                &rendering_state.vulkan_context.phys_memory_properties, 
                &rendering_state.logical_device,
                &mut rendering_state.deletion_queue 
            )?;

            let simple_cube_entity = component_manager.create_entity();
            component_manager.add_mesh(simple_cube_entity, simple_cube_mesh);
            
            let mut transform = Transform::new([0.0, 0.0, 0.0]);
            transform.translate(Vector3 { x: i as f32, y: (i as f32) % 2.0, z: (i as f32) % 5.0 });
            component_manager.add_transform(simple_cube_entity, transform);
        }
        Ok(Self{
                 rendering_state: rendering_state,
                 component_manager: component_manager
        })
    }

    pub fn render(&mut self) -> Result<()>{
        unsafe { self.rendering_state.render(&self.component_manager) }
    }

    pub fn update(&mut self) -> Result<()>{
        let rotation_speed = cgmath::Deg(0.001);
    
        // 2. Loop through all entities that have a transform
        for transform_opt in self.component_manager.transforms.iter_mut() {
            if let Some(transform) = transform_opt {
                // 3. Create a small rotation step around the Y axis
                let rotation_step = cgmath::Quaternion::from_angle_y(rotation_speed);
                
                // 4. Combine existing rotation with the new step
                // Note: Multiplication order matters with quaternions!
                transform.rotation = transform.rotation * rotation_step;
            }
        }
        Ok(())
    }

    pub unsafe fn destroy(&mut self){
        self.component_manager.destroy();
        info!("Engine shut down successfully.");
    }
}