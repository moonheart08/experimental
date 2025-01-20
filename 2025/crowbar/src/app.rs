use std::{collections::HashMap, process, sync::Arc};

use winit::{
    error::OsError,
    event::WindowEvent,
    event_loop::EventLoop,
    window::{Window, WindowAttributes, WindowId},
};

pub struct WindowState {
    winit_window: Arc<Window>,
}

impl WindowState {
    pub fn new(window: Window) -> WindowState {
        WindowState {
            winit_window: Arc::new(window),
        }
    }
}

pub(crate) struct WinitApp {
    windows: HashMap<WindowId, WindowState>,
}

impl WinitApp {
    pub fn new(event_loop: &mut EventLoop<()>) -> WinitApp {
        WinitApp {
            windows: Default::default(),
        }
    }

    pub fn create_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        attribs: WindowAttributes,
    ) -> Result<WindowId, OsError> {
        let window = event_loop.create_window(attribs)?;
        let id = window.id();
        self.windows.insert(id, WindowState::new(window));
        return Ok(id);
    }

    pub fn get_window(&self, id: WindowId) -> Arc<Window> {
        self.windows
            .get(&id)
            .expect("Unknown window!")
            .winit_window
            .clone()
    }
}

impl winit::application::ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.create_window(
            event_loop,
            WindowAttributes::default()
                .with_title("Crowbar Application")
                .with_active(true),
        )
        .expect("Initial window creation MUST succeed!");
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let window = self.get_window(window_id);

        match event {
            WindowEvent::RedrawRequested => {
                // ...
            }
            WindowEvent::CloseRequested => {
                process::exit(0); // todo: sane exit handling :)
            }
            e => {
                println!("Unhandled log event {e:?}")
            }
        }
    }
}
