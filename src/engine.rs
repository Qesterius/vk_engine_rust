use std::sync::Arc;

use anyhow::Result;
use cgmath::{Quaternion, Rotation3};
use winit::window::Window;

use crate::{
    component_system::simple_component_manager::ComponentManager,
    device::device::Device,
    rendering::{
        components::mesh::Mesh,
        rendering_instance::RenderingInstance,
        rendering_manager::RenderingManager,
    },
};
use crate::component_system::transform::Transform;

/// Field declaration order determines Drop order (first declared = first dropped).
/// This order ensures correct Vulkan teardown:
/// 1. renderer   - device_wait_idle, destroy pipeline/swapchain/canvas
/// 2. component_manager - destroy mesh buffers (device still valid via Arc)
/// 3. device     - Arc refcount -> 0, destroy logical device
/// 4. rendering_instance - destroy debug messenger + vulkan instance
/// 5. window     - close OS window (after surface already destroyed in canvas)
pub struct Engine {
    pub renderer: RenderingManager,
    pub component_manager: ComponentManager,
    pub device: Arc<Device>,
    pub rendering_instance: Arc<RenderingInstance>,
    pub window: Window,
}

impl Engine {
    pub fn new(window: Window) -> Result<Self> {
        let rendering_instance = Arc::new(RenderingInstance::new(&window)?);

        let (surface, surface_loader) = rendering_instance.create_surface(&window)?;

        let device = Device::new(&rendering_instance, surface, &surface_loader)?;

        let renderer = RenderingManager::new(
            Arc::clone(&device),
            rendering_instance.instance.clone(),
            surface,
            surface_loader,
            window.inner_size(),
        )?;

        let mut component_manager = ComponentManager::new()?;

        for i in 0..10 {
            let cube = Mesh::new_cube(
                &device.memory_properties,
                &device.logical_device,
                &mut component_manager.deletion_queue,
            )?;
            let entity = component_manager.create_entity();
            component_manager.add_mesh(entity, cube);

            let mut transform = Transform::new([0.0, 0.0, 0.0]);
            transform.translate(cgmath::Vector3 {
                x: i as f32,
                y: (i as f32) % 2.0,
                z: (i as f32) % 5.0,
            });
            component_manager.add_transform(entity, transform);
        }

        Ok(Self {
            renderer,
            component_manager,
            device,
            rendering_instance,
            window,
        })
    }

    pub fn render(&mut self) -> Result<()> {
        self.renderer.render(&self.component_manager)
    }

    pub fn update(&mut self) -> Result<()> {
        let rotation_speed = cgmath::Deg(0.001);
        for transform_opt in self.component_manager.transforms.iter_mut() {
            if let Some(transform) = transform_opt {
                let step = Quaternion::from_angle_y(rotation_speed);
                transform.rotation = transform.rotation * step;
            }
        }
        Ok(())
    }

    pub fn handle_resize(&mut self) -> Result<()> {
        self.renderer.handle_resize(self.window.inner_size())
    }
}
