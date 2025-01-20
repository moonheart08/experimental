use std::ptr;
use std::sync::LazyLock;

use ash::Entry;
mod alloc;

pub static VK_ENTRY: LazyLock<Option<Entry>> = LazyLock::new(|| unsafe { Entry::load().ok() });

pub fn render_setup() {
    if let Some(vk) = VK_ENTRY.as_ref() {
        //vk.create_instance(create_info, Some(alloc::VK_ALLOCATOR_CALLBACKS))
    }
}
