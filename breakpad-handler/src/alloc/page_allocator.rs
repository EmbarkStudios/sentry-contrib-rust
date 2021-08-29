use std::{mem, ptr};

// Accessing a `static mut` is unsafe much of the time, but if we do so
// in a synchronized fashion (e.g. write once or read all) then we're
// good to go!
//
// This function will only call `expensive_computation` once, and will
// otherwise always return the value returned from the first invocation.
#[inline]
fn get_page_size() -> usize {
    static mut PAGE_SIZE: usize = 0;
    static INIT_PAGE_SIZE: parking_lot::Once = parking_lot::Once::new();

    unsafe {
        INIT_PAGE_SIZE.call_once(|| {
            PAGE_SIZE = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        });
        PAGE_SIZE
    }
}

// #[derive(Debug)]
// pub(crate) struct AllocError;

/// Intrusively linked list. Since these are the page entries for the
/// `PageAllocator` itself, they also can't be heap allocated, so each block of
/// allocated pages reserves space for this header
struct PageHeader {
    /// Pointer to the start of the next set of pages.
    next: Option<*mut Self>,
    /// The number of pages in this set
    num_pages: usize,
}

#[derive(Copy, Clone)]
struct Page {
    start: *mut u8,
    offset: usize,
}

/// This is very simple allocator which fetches pages from the kernel directly
/// so that it can be used in crash contexts where the heap may be corrupt.
///
/// There is no free operation, and the pages are only freed when the allocator
/// is dropped.
pub(crate) struct PageAllocator {
    last: Option<*mut PageHeader>,
    current_page: Option<Page>,
    total_allocated_pages: usize,
}

impl PageAllocator {
    pub(crate) fn new() -> Self {
        Self {
            last: None,
            current_page: None,
            total_allocated_pages: 0,
        }
    }

    #[inline]
    pub(crate) fn pages_allocated(&self) -> usize {
        self.total_allocated_pages
    }

    pub(crate) fn alloc_raw(&mut self, size: usize) -> Result<ptr::NonNull<u8>, super::AllocError> {
        unsafe {
            let page_size = get_page_size();

            // See if we can allocate from the current page without splitting
            if let Some(cur_page) = &mut self.current_page {
                if page_size - cur_page.offset >= size {
                    let ret = cur_page.start.offset(cur_page.offset as isize);
                    cur_page.offset += size;

                    // If we've filled the page we can remove it
                    if cur_page.offset == page_size {
                        self.current_page = None;
                    }

                    return ptr::NonNull::new(ret).ok_or(super::AllocError);
                }
            }

            let num_pages = (size + mem::size_of::<PageHeader>() + page_size - 1) / page_size;

            let ret = self.alloc_pages(page_size, num_pages)?;

            let offset = (page_size
                - (page_size * num_pages - (size + mem::size_of::<PageHeader>())))
                % page_size;

            if offset != 0 {
                self.current_page = Some(Page {
                    offset,
                    start: ret.as_ptr().offset((page_size * (num_pages - 1)) as isize),
                });
            }

            Ok(ptr::NonNull::new_unchecked(
                ret.as_ptr().offset(mem::size_of::<PageHeader>() as isize),
            ))
        }
    }

    unsafe fn alloc_pages(
        &mut self,
        page_size: usize,
        num_pages: usize,
    ) -> Result<ptr::NonNull<u8>, super::AllocError> {
        let alloced = libc::mmap(
            ptr::null_mut(),
            page_size * num_pages,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );

        if alloced == libc::MAP_FAILED {
            return Err(super::AllocError);
        }

        let last = alloced.cast::<PageHeader>();
        (*last).next = self.last;
        (*last).num_pages = num_pages;
        self.last = Some(last);

        self.total_allocated_pages += num_pages;

        Ok(ptr::NonNull::new_unchecked(alloced.cast::<u8>()))
    }

    fn free_pages(&mut self) {
        unsafe {
            let mut cur = self.last.take();
            let page_size = get_page_size();

            while let Some(cur_set) = cur {
                let next = (*cur_set).next;

                libc::munmap(
                    cur_set.cast::<libc::c_void>(),
                    (*cur_set).num_pages * page_size,
                );

                cur = next;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn owns_pointer(&self, ptr: *const libc::c_void) -> bool {
        unsafe {
            let ptr = ptr.cast::<u8>();
            let mut current = self.last;
            let page_size = get_page_size();

            while let Some(cur) = current {
                let cur_ptr = cur.cast::<u8>();
                if ptr >= cur_ptr && ptr < cur_ptr.offset(((*cur).num_pages * page_size) as isize) {
                    return true;
                }

                current = (*cur).next;
            }
        }

        false
    }
}

unsafe impl super::AllocRef for PageAllocator {
    fn alloc(&self, layout: std::alloc::Layout) -> Result<ptr::NonNull<[u8]>, super::AllocError> {
        unsafe {
            let alloced = (*(self as *const Self as *mut Self)).alloc_raw(layout.size())?;
            Ok(ptr::NonNull::new_unchecked(std::slice::from_raw_parts_mut(
                alloced.as_ptr(),
                layout.size(),
            )))
        }
    }

    unsafe fn dealloc(&self, _ptr: ptr::NonNull<u8>, _layout: std::alloc::Layout) {
        // We don't implement deallocation, so just have to wait until the entire
        // allocator is dropped to free the memory
    }
}

impl Drop for PageAllocator {
    fn drop(&mut self) {
        self.free_pages();
    }
}

#[cfg(test)]
mod test {
    use super::PageAllocator;

    #[test]
    fn setup() {
        let pa = PageAllocator::new();
        assert_eq!(0, pa.total_allocated_pages);
    }

    #[test]
    fn small_objects() {
        let mut pa = PageAllocator::new();
        for i in 1..1024 {
            let alloced = pa.alloc_raw(i).unwrap();
            unsafe {
                std::slice::from_raw_parts_mut(alloced.as_ptr(), i).fill(0);
            }
        }
    }

    #[test]
    fn large_object() {
        let mut pa = PageAllocator::new();

        pa.alloc_raw(10 * 1024).unwrap();

        let page_size = super::get_page_size();
        assert_eq!((10 * 1024 / page_size) + 1, pa.total_allocated_pages);

        for i in 1..10 {
            let alloced = pa.alloc_raw(i).unwrap();
            unsafe {
                std::slice::from_raw_parts_mut(alloced.as_ptr(), i).fill(0);
            }
        }
    }
}
