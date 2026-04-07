use bevy_ecs::resource::Resource;

/// Total number of frames successfully rendered and presented.
/// Updated by RenderingManager at the end of each successful frame.
/// Read by any ECS system that needs to know the current frame count.
#[derive(Resource, Default)]
pub struct RenderedFrameCount(pub u64);

pub const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

#[derive(Resource)]
pub struct Time {
    /// Seconds since the last frame
    pub delta: f32,
    /// Total elapsed seconds since engine start
    pub elapsed: f32,
    /// Fixed timestep size in seconds
    pub fixed_delta: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            delta: 0.0,
            elapsed: 0.0,
            fixed_delta: FIXED_TIMESTEP,
        }
    }
}
//TODO implement timestamp and timer system for animations
