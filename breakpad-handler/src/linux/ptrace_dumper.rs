use crate::alloc::PageAllocator;

struct PTraceDumper {
    allocator: PageAllocator,
    threads_suspended: bool,
}
