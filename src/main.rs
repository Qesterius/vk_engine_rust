mod utils;
mod config;
mod rendering;

use crate::rendering::rendering_state::RenderingState;

use crate::config::{APPLICATION_TITLE};
use log::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};
use anyhow::{Ok, Result};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Logger initialized");
    let event_loop = EventLoop::new()?;
    let mut app = App{ state: None};
    event_loop.run_app(&mut app)?;
    Ok(())
}
struct App
{
    state: Option<RenderingState>
}

impl ApplicationHandler for App{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none(){
            let window_attrs = WindowAttributes::default().with_title(APPLICATION_TITLE);
            let window = event_loop.create_window(window_attrs).expect("Failed to create window");

            match unsafe{ RenderingState::new(window)}{
                core::result::Result::Ok(state) => {
                    self.state = Some(state);
                }
                core::result::Result::Err(e) => {
                    error!("Failed to initialize Vulkan: {}", e);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        let _ = window_id;
        match event{
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state{
                    unsafe{ state.render().expect("Failed to render frame");}
                    state.window.request_redraw();
                }
            }
            _ => (),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut state) = self.state.take(){
            unsafe{state.destroy();}
        }
    }
}


