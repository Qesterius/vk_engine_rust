use bevy_ecs::resource::Resource;
use winit::dpi::PhysicalSize;

/// Stores window-level events for the current frame.
/// Populated by the winit event loop before `engine.tick()`.
/// Cleared at the start of each tick by the `pre_update` schedule.
#[derive(Resource, Default)]
pub struct WindowEvents {
    pub resized: Option<PhysicalSize<u32>>,
    //TODO: can add focus/minimize events
}

/// Set to true by any system (or main.rs) that wants the engine to exit.
#[derive(Resource, Default)]
pub struct AppExit(pub bool);

pub fn clear_window_events(mut events: bevy_ecs::system::ResMut<WindowEvents>) {
    *events = WindowEvents::default();
}
