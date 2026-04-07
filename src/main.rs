mod component_system;
mod config;
mod device;
mod engine;
mod rendering;
mod time;
mod utils;
mod window_events;

use crate::config::APPLICATION_TITLE;
use crate::engine::Engine;
use crate::window_events::{AppExit, WindowEvents};
use anyhow::Result;
use log::{error, info};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let event_loop = EventLoop::new()?;
    let mut app = App { engine: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    engine: Option<Engine>,
}

impl App {
    fn init_engine(&self, event_loop: &ActiveEventLoop) -> Result<Engine> {
        let window =
            event_loop.create_window(WindowAttributes::default().with_title(APPLICATION_TITLE))?;
        Engine::new(window)
    }

    fn on_redraw(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let engine = match &mut self.engine {
            Some(e) => e,
            None => return Ok(()),
        };

        if let Some(size) = engine.world.resource::<WindowEvents>().resized {
            engine.handle_resize(size)?;
        }

        engine.tick()?;

        if engine.world.resource::<AppExit>().0 {
            event_loop.exit();
            return Ok(());
        }

        engine.render()?;
        engine.window.request_redraw();
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.engine.is_some() {
            return;
        }
        match self.init_engine(event_loop) {
            Ok(engine) => self.engine = Some(engine),
            Err(e) => {
                error!("Engine init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.on_redraw(event_loop) {
                    error!("Fatal render error: {e:#}");
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                // Resize data was pushed to WindowEvents above.
                // Request a redraw so on_redraw picks it up this frame.
                if let Some(engine) = &mut self.engine {
                    engine.world.resource_mut::<WindowEvents>().resized = Some(size);
                    engine.window.request_redraw();
                }
            }
            _ => {}
        }
    }
    // Gracefully shut down the engine on exit, allowing it to clean up resources.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(engine) = &mut self.engine {
            engine.shutdown();
        }
        self.engine.take();
        info!("Engine shut down.");
    }
}
