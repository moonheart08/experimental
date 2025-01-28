#![feature(allocator_api)]
#![feature(slice_ptr_get)]
#![feature(pointer_is_aligned_to)]
use winit::event_loop::EventLoop;

pub mod app;
pub mod consts;
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
