use std::{
    alloc::{self, GlobalAlloc, Layout},
    ffi::c_void,
    marker::PhantomData,
    ptr,
    sync::{
        LazyLock,
        atomic::{self, AtomicUsize},
    },
};

use ash::vk::{AllocationCallbacks, SystemAllocationScope};

pub static VK_ALLOCATOR: LazyLock<&'static CrowbarVkAllocator> =
    LazyLock::new(|| Box::leak(Box::new(CrowbarVkAllocator::new())));
pub static VK_ALLOCATOR_CALLBACKS: LazyLock<AllocationCallbacks<'static>> =
    LazyLock::new(|| unsafe {
        AllocationCallbacks {
            // SAFETY: We never perform actual mutation of the allocator data.
            p_user_data: VK_ALLOCATOR.into() as *const CrowbarVkAllocator as *mut c_void,
            pfn_allocation: Some(vk_alloc),
            pfn_reallocation: Some(vk_realloc),
            _marker: PhantomData,
        }
    });

pub struct CrowbarVkAllocator {
    /// Memory allocated through us by the vulkan instance.
    pub allocated: AtomicUsize,
    /// Memory the driver claims to have allocated itself.
    pub driver_allocated: AtomicUsize,
}

impl CrowbarVkAllocator {
    /// Safely construct a pinned crowbar vk allocator.
    pub fn new() -> CrowbarVkAllocator {
        let b = CrowbarVkAllocator {
            allocated: AtomicUsize::new(0),
            driver_allocated: AtomicUsize::new(0),
        };

        return b;
    }
}

unsafe fn userdata_as_allocator(userdata: *mut c_void) -> &'static CrowbarVkAllocator {
    return unsafe {
        (userdata as *mut CrowbarVkAllocator)
            .as_ref()
            .expect("Null allocator data. Driver bug?")
    };
}

/// A small "tag" structure put ahead of any vulkan managed allocations for tracking purposes.
#[repr(align(8))]
struct MemoryTag {
    #[cfg(debug_assertions)]
    magic: usize,
    size: usize,
    align: usize,
    scope: SystemAllocationScope,
    base: *mut c_void,
}

const MT_LAYOUT: Layout = Layout::new::<MemoryTag>();
const MT_MAGIC: usize = 0x7E_E7_AB_BA_CA_FE_B0_0B;

unsafe fn as_tag_and_block<'a>(p: *mut c_void) -> (&'a mut MemoryTag, *mut c_void) {
    // SAFETY: Given a block allocated with padding, and a minimum alignment matching MemoryTag's, we can place the tag information directly before
    // the block itself. This MAY result in unused 'start' padding, but it ensures we can locate the tag without knowing how much padding there is.
    // MemoryTag then contains a pointer to the actual base of the allocation.
    unsafe {
        let tag = p.byte_offset(-(size_of::<MemoryTag>() as isize)) as *mut MemoryTag;

        return (tag.as_mut().unwrap_unchecked(), p);
    }
}

unsafe extern "system" fn vk_alloc(
    userdata: *mut c_void,
    size: usize,
    align: usize,
    scope: SystemAllocationScope,
) -> *mut c_void {
    let layout = MT_LAYOUT
        .extend(
            Layout::from_size_align(size, align.min(align_of::<MemoryTag>()))
                .expect("Driver bug: Invalid alignment given for allocation."),
        )
        .expect("Allocation overflowed.");

    let data = unsafe { userdata_as_allocator(userdata) };

    data.allocated
        .fetch_add(layout.0.size(), atomic::Ordering::Relaxed);

    // SAFETY: Simple allocation using the provided layout, we're just a shim.
    let allocated = unsafe { std::alloc::alloc(layout.0) as *mut c_void };
    // SAFETY: Offset to account for the tag, we accounted for this when allocating.
    let block = unsafe { allocated.byte_offset(layout.1 as isize) };

    // SAFETY: This is a correctly aligned tag/block pair.
    let (tag, block) = unsafe { as_tag_and_block(block) };

    // Set up the tag for validity. Necessary for this to be safe.
    tag.base = allocated;
    tag.align = layout.0.align();
    tag.size = layout.0.size();
    tag.scope = scope;
    #[cfg(debug_assertions)]
    {
        tag.magic = MT_MAGIC;
    }

    return block; // and return the untagged allocation.
}

unsafe extern "system" fn vk_realloc(
    userdata: *mut c_void,
    original: *mut c_void,
    size: usize,
    align: usize,
    _scope: SystemAllocationScope,
) -> *mut c_void {
    let layout = MT_LAYOUT
        .extend(
            Layout::from_size_align(size, align.min(align_of::<MemoryTag>()))
                .expect("Driver bug: Invalid alignment given for allocation."),
        )
        .expect("Allocation overflowed.");

    let data = unsafe { userdata_as_allocator(userdata) };

    unsafe { std::alloc::realloc(ptr, layout, new_size) }
}
