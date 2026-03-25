mod utils;
mod config;
mod rendering;
mod component_system;
mod engine;
mod device;

use crate::engine::Engine;
use crate::config::APPLICATION_TITLE;
use log::{error, info};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};
use anyhow::{Ok, Result};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Logger initialized");
    let event_loop = EventLoop::new()?;
    let mut app = App { engine: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    pub engine: Option<Engine>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.is_none() {
            let window_attrs = WindowAttributes::default().with_title(APPLICATION_TITLE);
            let window = event_loop.create_window(window_attrs).expect("Failed to create window");

            match Engine::new(window) {
                core::result::Result::Ok(engine) => {
                    self.engine = Some(engine);
                }
                core::result::Result::Err(e) => {
                    error!("Engine init error: {}", e);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(engine) = &mut self.engine {
                    engine.update().expect("Failed update pass");
                    engine.render().expect("Failed to render frame");
                    engine.window.request_redraw();
                }
            }
            WindowEvent::Resized(_) => {
                if let Some(engine) = &mut self.engine {
                    engine.handle_resize().expect("Failed to handle resize");
                }
            }
            _ => (),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Engine drops here automatically via Option::take, triggering correct Drop order
        self.engine.take();
        info!("Engine shut down.");
    }
}
