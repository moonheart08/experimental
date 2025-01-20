use winit::event_loop::EventLoop;

pub mod app;
pub mod render;

fn main() {
    println!("Hello, world!");

    let mut event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = app::WinitApp::new(&mut event_loop);
    event_loop
        .run_app(&mut app)
        .expect("Event loop should return successfully or not return.");
}
