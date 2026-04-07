use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use bevy_ecs::{schedule::Schedule, system::Query, world::Mut};
use cgmath::{Quaternion, Rotation3};
use winit::window::Window;

use crate::{
    device::device::Device,
    rendering::{
        components::mesh::Mesh,
        rendering_instance::RenderingInstance,
        rendering_manager::RenderingManager,
    },
    time::{Time, RenderedFrameCount},
    window_events::{AppExit, WindowEvents, clear_window_events},
};
use crate::component_system::transform::Transform;

// Main engine sctruct. It is responsible for scheduling and running ecs systems
pub struct Engine {
    pub world: bevy_ecs::world::World,

    /// Runs at the start of each tick: pre-frame setup (input staging, etc.).
    pub pre_update: Schedule,
    /// Runs at a fixed timestep: game logic, physics, AI.
    pub fixed_update: Schedule,
    /// Runs once per frame: animations, camera, anything needing delta time.
    pub update: Schedule,
    /// Runs at the end of each tick: ex. clears per-frame events so systems see them during one tick
    pub post_update: Schedule,

    // Acummulator of how much time elapsed since last fixed frame.
    fixed_delta_accumulator: f32,
    last_frame: Instant,

    pub device: Arc<Device>,
    pub rendering_instance: Arc<RenderingInstance>,
    pub window: Window,
}

impl Engine {
    pub fn new(window: Window) -> Result<Self> {
        let rendering_instance = Arc::new(RenderingInstance::new(&window)?);

        let (surface, surface_loader) = rendering_instance.create_surface(&window)?;

        let device = Device::new(&rendering_instance, surface, &surface_loader)?;

        let mut world = bevy_ecs::world::World::new();

        // --- Resources ---
        world.insert_resource(Time::default());
        world.insert_resource(RenderedFrameCount::default());
        world.insert_resource(WindowEvents::default());
        world.insert_resource(AppExit::default());

        let renderer = RenderingManager::new(
            Arc::clone(&device),
            Arc::clone(&rendering_instance),
            surface,
            surface_loader,
            window.inner_size(),
        )?;
        world.insert_resource(renderer);

        // --- Entities ---
        for i in 0..10 {
            let cube = Mesh::new_cube(
                &device.memory_properties,
                Arc::clone(&device),
            )?;
            let mut transform = Transform::new([0.0, 0.0, 0.0]);
            transform.translate(cgmath::Vector3 {
                x: i as f32,
                y: (i as f32) % 2.0,
                z: (i as f32) % 5.0,
            });
            world.spawn((cube, transform));
        }

        // --- Schedules ---
        let pre_update = Schedule::default();

        let mut fixed_update = Schedule::default();
        fixed_update.add_systems(Engine::rotate_all);

        let update = Schedule::default();

        let mut post_update = Schedule::default();
        post_update.add_systems(clear_window_events);

        Ok(Self {
            world,
            pre_update,
            fixed_update,
            update,
            post_update,
            fixed_delta_accumulator: 0.0,
            last_frame: Instant::now(),
            device,
            rendering_instance,
            window,
        })
    }

    // Application loop, running all schedulers
    pub fn tick(&mut self) -> Result<()> {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        // Update Time resource
        let mut time = self.world.resource_mut::<Time>();
        time.delta = delta;
        time.elapsed += delta;

        // Pre-update: setup systems (input staging, etc.)
        self.pre_update.run(&mut self.world);

        // Fixed update: 
        // Makes sure it executes fixedupdate exactly that many times 
        // how much steps fit since last loop 
        self.fixed_delta_accumulator += delta;
        let fixed_step = self.world.resource::<Time>().fixed_delta;
        while self.fixed_delta_accumulator >= fixed_step {
            self.fixed_update.run(&mut self.world);
            self.fixed_delta_accumulator -= fixed_step;
        }

        // Per-frame update
        // ex. rendering
        self.update.run(&mut self.world);

        // Post-update: 
        // clear per-frame one of data like events
        self.post_update.run(&mut self.world);

        Ok(())
    }

    // Idle the GPU before any Vulkan resources are freed by the world drop,
    // otherwise GPU might still try to finish work on freed buffers
    pub fn shutdown(&mut self) {
        unsafe { let _ = self.device.logical_device.device_wait_idle(); }
    }

    // Render calling outside of Bevy ECS scheduling. 
    // It does not gain anything from concurent schedulers as its on GPU and is 
    // cleanly seperated from any updates on component resources
    pub fn render(&mut self) -> Result<()> {
        self.world.resource_scope(|world, mut renderer: Mut<RenderingManager>| {
            renderer.render(world)
        })
    }

    pub fn handle_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) -> Result<()> {
        self.world.resource_scope(|_world, mut renderer: Mut<RenderingManager>| {
            renderer.handle_resize(size)
        })
    }

    // simple system to validate rendering 
    fn rotate_all(mut query: Query<&mut Transform>) {
        let rotation_speed = cgmath::Deg(0.5);
        let step = Quaternion::from_angle_y(rotation_speed);
        for mut transform in &mut query {
            transform.rotation = transform.rotation * step;
        }
    }
}
