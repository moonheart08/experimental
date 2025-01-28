use std::{
    alloc::{Allocator, Global, Layout},
    ffi::c_void,
    marker::PhantomData,
    ptr::{self, NonNull},
    sync::{
        LazyLock,
        atomic::{self, AtomicUsize},
    },
};

use ash::vk::{AllocationCallbacks, SystemAllocationScope};

/// A vulkan allocator wrapping the global allocation context.
pub static VK_ALLOCATOR: LazyLock<&'static CrowbarVkAllocator<Global>> =
    LazyLock::new(|| Box::leak(Box::new(CrowbarVkAllocator::<Global>::new(Global))));

pub static VK_ALLOCATOR_CALLBACKS: LazyLock<AllocationCallbacks<'static>> =
    LazyLock::new(|| AllocationCallbacks {
        // SAFETY: We never create a mutable ref to the allocator.
        p_user_data: VK_ALLOCATOR.to_owned() as *const CrowbarVkAllocator<Global> as *mut c_void,
        pfn_allocation: Some(vk_alloc::<Global>),
        pfn_reallocation: Some(vk_realloc::<Global>),
        pfn_free: Some(vk_free::<Global>),
        pfn_internal_allocation: None,
        pfn_internal_free: None,
        _marker: PhantomData,
    });

pub struct CrowbarVkAllocator<TAlloc: Allocator + Send + Sync> {
    pub allocator: TAlloc,
    /// Memory allocated through us by the vulkan instance.
    pub allocated: AtomicUsize,
    /// Memory the driver claims to have allocated itself.
    pub driver_allocated: AtomicUsize,
}

impl<TAlloc: Allocator + Send + Sync> CrowbarVkAllocator<TAlloc> {
    /// Safely construct a pinned crowbar vk allocator.
    pub fn new(allocator: TAlloc) -> CrowbarVkAllocator<TAlloc> {
        let b = CrowbarVkAllocator::<TAlloc> {
            allocator,
            allocated: AtomicUsize::new(0),
            driver_allocated: AtomicUsize::new(0),
        };

        return b;
    }
}

unsafe fn userdata_as_allocator<TAlloc: Allocator + Send + Sync>(
    userdata: *mut c_void,
) -> &'static CrowbarVkAllocator<TAlloc> {
    return unsafe {
        (userdata as *mut CrowbarVkAllocator<TAlloc>)
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

impl MemoryTag {
    pub fn layout(&self) -> Layout {
        // SAFETY: We got this from a layout before, we know it's valid.
        unsafe { Layout::from_size_align_unchecked(self.size, self.align) }
    }
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

unsafe fn validate_alloc(alloc: *mut c_void) -> bool {
    let (tag, _) = unsafe { as_tag_and_block(alloc) };

    #[cfg(debug_assertions)]
    {
        return tag.magic == MT_MAGIC;
    }

    #[cfg(not(debug_assertions))]
    return true;
}

fn make_layout(size: usize, align: usize) -> Option<(Layout, usize)> {
    Some(
        MT_LAYOUT
            .extend(Layout::from_size_align(size, align.max(align_of::<MemoryTag>())).ok()?)
            .ok()?,
    )
}

unsafe extern "system" fn vk_alloc<TAlloc: Allocator + Send + Sync + 'static>(
    userdata: *mut c_void,
    size: usize,
    align: usize,
    scope: SystemAllocationScope,
) -> *mut c_void {
    let Some(layout) = make_layout(size, align) else {
        return ptr::null::<u8>() as *mut c_void;
    };

    let data = unsafe { userdata_as_allocator::<TAlloc>(userdata) };

    data.allocated
        .fetch_add(layout.0.size(), atomic::Ordering::Relaxed);

    // SAFETY: Simple allocation using the provided layout, we're just a shim.
    let Ok(allocated) = data.allocator.allocate(layout.0) else {
        return ptr::null::<u8>() as *mut c_void;
    };

    let allocated = allocated.as_mut_ptr() as *mut c_void;

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

unsafe extern "system" fn vk_realloc<TAlloc: Allocator + Send + Sync + 'static>(
    userdata: *mut c_void,
    original: *mut c_void,
    size: usize,
    align: usize,
    scope: SystemAllocationScope,
) -> *mut c_void {
    #[derive(PartialEq, Eq)]
    enum GrowOrShrink {
        Grow,
        Shrink,
    }

    let Some(layout) = make_layout(size, align) else {
        return ptr::null::<u8>() as *mut c_void;
    };

    let data = unsafe { userdata_as_allocator::<TAlloc>(userdata) };
    let allocator = &data.allocator;

    assert!(unsafe { validate_alloc(original) });

    let (base_ptr, grow_or_shrink, old_layout) = 
    // Safety scope, as we're going to do a reallocation and the old tag would be UB to hang on to.
    {
        let (tag, _) = unsafe { as_tag_and_block(original) };

        // SAFETY: We got this from a layout before, we know it's valid.
        let old_layout = tag.layout();

        (tag.base, 
            // If new layout larger, grow, else shrink.
            (old_layout.size() < layout.0.size())
            .then_some(GrowOrShrink::Grow).unwrap_or(GrowOrShrink::Shrink),
            old_layout
        )
    };

    let new_alloc;
    unsafe {
        if grow_or_shrink == GrowOrShrink::Grow {
            new_alloc = allocator.grow(
                NonNull::new_unchecked(base_ptr).cast(),
                old_layout,
                layout.0,
            );
            data.allocated.fetch_add(
                layout.0.size() - old_layout.size(),
                atomic::Ordering::Relaxed,
            );
        } else {
            new_alloc = allocator.shrink(
                NonNull::new_unchecked(base_ptr).cast(),
                old_layout,
                layout.0,
            );
            data.allocated.fetch_sub(
                old_layout.size() - layout.0.size(),
                atomic::Ordering::Relaxed,
            );
        }
    };

    if let Err(_) = new_alloc {
        // Return null as per spec, due to allocation failure.
        return ptr::null::<u8>() as *mut c_void;
    }

    let allocated = new_alloc.unwrap().as_ptr() as *mut c_void;

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

unsafe extern "system" fn vk_free<TAlloc: Allocator + Send + Sync + 'static>(
    userdata: *mut c_void,
    original: *mut c_void,
) {
    let data = unsafe { userdata_as_allocator::<TAlloc>(userdata) };
    let allocator = &data.allocator;

    assert!(unsafe { validate_alloc(original) });

    let size;
    {
        let (tag, _) = unsafe { as_tag_and_block(original) };
        size = tag.layout().size();

        // SAFETY: Man I hope the driver doesn't ask us to dealloc invalid memory.
        unsafe { 
            allocator.deallocate(NonNull::new_unchecked(tag.base).cast(), tag.layout())
        };
    }

    data.allocated.fetch_sub(size, atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod test {
    use std::{ffi::c_void, ptr::slice_from_raw_parts_mut, usize};

    use ash::vk::SystemAllocationScope;

    use crate::render::alloc::validate_alloc;

    use super::VK_ALLOCATOR_CALLBACKS;

    unsafe fn vk_global_alloc(
        size: usize,
        align: usize,
        scope: SystemAllocationScope,
    ) -> *mut c_void {
        let cb = VK_ALLOCATOR_CALLBACKS.to_owned();
        return unsafe { cb.pfn_allocation.unwrap()(cb.p_user_data, size, align, scope) };
    }

    unsafe fn vk_global_realloc(
        original: *mut c_void,
        size: usize,
        align: usize,
        scope: SystemAllocationScope,
    ) -> *mut c_void {
        let cb = VK_ALLOCATOR_CALLBACKS.to_owned();
        return unsafe {
            cb.pfn_reallocation.unwrap()(cb.p_user_data, original, size, align, scope)
        };
    }

    unsafe fn vk_global_free(
        original: *mut c_void,
    ) {
        let cb = VK_ALLOCATOR_CALLBACKS.to_owned();
        unsafe {
            cb.pfn_free.unwrap()(cb.p_user_data, original)
        };
    }


    #[test]
    pub fn allocate() {
        unsafe {
            const SIZE: usize = 320;
            const ALIGN: usize = 128;

            let alloc = vk_global_alloc(SIZE, ALIGN, SystemAllocationScope::INSTANCE);

            assert!(!alloc.is_null(), "Allocation in test must succeed.");
            assert!(validate_alloc(alloc), "Allocation validation failed.");
            assert!(
                alloc.is_aligned_to(ALIGN),
                "Allocation alignment is incorrect."
            );

            {
                let slice = slice_from_raw_parts_mut(alloc as *mut u8, SIZE)
                    .as_mut()
                    .unwrap();

                for i in slice {
                    *i = 37;
                }
            }

            let alloc = vk_global_realloc(alloc, SIZE * 2, ALIGN, SystemAllocationScope::INSTANCE);

            assert!(!alloc.is_null(), "Allocation in test must succeed.");
            assert!(validate_alloc(alloc), "Allocation validation failed.");
            assert!(
                alloc.is_aligned_to(ALIGN),
                "Allocation alignment is incorrect."
            );

            {
                let slice = slice_from_raw_parts_mut(alloc as *mut u8, SIZE)
                    .as_mut()
                    .unwrap();

                for i in 0..SIZE {
                    assert_eq!(slice[i], 37, "Reallocation grow garbled memory.");
                }
            }

            vk_global_free(alloc);
        }
    }

    #[test]
    pub fn reasonable_failure() {
        unsafe {
            let alloc = vk_global_alloc(usize::MAX, 1, SystemAllocationScope::INSTANCE);
            assert!(alloc.is_null(), "Allocation should fail gracefully.");

            let alloc = vk_global_alloc(4, usize::MAX, SystemAllocationScope::INSTANCE);
            assert!(alloc.is_null(), "Allocation should fail gracefully.");
        }
    }
}
