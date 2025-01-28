use std::sync::LazyLock;
use std::{ffi::CString, ptr};

use ash::{Entry, vk};

use crate::consts::{APPLICATION_VERSION, ENGINE_VERSION};
mod alloc;

pub static VK_ENTRY: LazyLock<Option<Entry>> = LazyLock::new(|| unsafe { Entry::load().ok() });

pub fn render_setup() {
    if let Some(vk) = VK_ENTRY.as_ref() {
        let app_name = CString::new("Crowbar").unwrap();
        let engine_name = CString::new("Crowbar").unwrap();

        let app_info = vk::ApplicationInfo {
            s_type: vk::StructureType::APPLICATION_INFO,
            p_next: ptr::null(),
            p_application_name: app_name.as_ptr(),
            application_version: APPLICATION_VERSION,
            p_engine_name: engine_name.as_ptr(),
            engine_version: ENGINE_VERSION,
            api_version: ash::vk::API_VERSION_1_3,
            ..Default::default()
        };

        vk.create_instance(&info, Some(alloc::VK_ALLOCATOR_CALLBACKS.to_owned()))
    }
}
