use super::{page_allocator::PageAllocator, raw_vec::RawVec};
use std::{
    cmp, hash, mem,
    ops::{self, Index, IndexMut},
    ptr, slice,
};

#[derive(Clone)]
pub(crate) struct Allocator {
    pub(crate) inner: std::rc::Rc<std::cell::RefCell<PageAllocator>>,
}

impl Allocator {
    pub(crate) fn new() -> Self {
        Self {
            inner: std::rc::Rc::new(std::cell::RefCell::new(PageAllocator::new())),
        }
    }
}

impl From<std::rc::Rc<std::cell::RefCell<PageAllocator>>> for Allocator {
    fn from(alloc: std::rc::Rc<std::cell::RefCell<PageAllocator>>) -> Self {
        Self { inner: alloc }
    }
}

unsafe impl super::AllocRef for Allocator {
    fn alloc(&self, layout: std::alloc::Layout) -> Result<ptr::NonNull<[u8]>, super::AllocError> {
        self.inner.borrow_mut().alloc(layout)
    }

    unsafe fn dealloc(&self, _ptr: ptr::NonNull<u8>, _layout: std::alloc::Layout) {}
}

pub(crate) struct PageVec<T> {
    buf: RawVec<T, Allocator>,
    len: usize,
}

impl<T> PageVec<T> {
    #[inline]
    pub(crate) fn new_in(alloc: Allocator) -> Self {
        Self {
            buf: RawVec::new_in(alloc),
            len: 0,
        }
    }

    #[inline]
    pub(crate) fn with_capacity_in(capacity: usize, alloc: Allocator) -> Self {
        Self {
            buf: RawVec::with_capacity_in(capacity, alloc),
            len: 0,
        }
    }

    #[inline]
    pub(crate) unsafe fn from_raw_parts_in(
        ptr: *mut T,
        length: usize,
        capacity: usize,
        alloc: Allocator,
    ) -> Self {
        Self {
            buf: RawVec::from_raw_parts_in(ptr, capacity, alloc),
            len: length,
        }
    }

    #[inline]
    pub fn into_raw_parts(self) -> (*mut T, usize, usize) {
        let mut me = mem::ManuallyDrop::new(self);
        (me.as_mut_ptr(), me.len(), me.capacity())
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(self.len, additional);
    }

    pub fn reserve_exact(&mut self, additional: usize) {
        self.buf.reserve_exact(self.len, additional);
    }

    pub fn shrink_to_fit(&mut self) {
        // The capacity is never less than the length, and there's nothing to do when
        // they are equal, so we can avoid the panic case in `RawVec::shrink_to_fit`
        // by only calling it with a greater capacity.
        if self.capacity() > self.len {
            self.buf.shrink_to_fit(self.len);
        }
    }

    pub fn truncate(&mut self, len: usize) {
        // This is safe because:
        //
        // * the slice passed to `drop_in_place` is valid; the `len > self.len`
        //   case avoids creating an invalid slice, and
        // * the `len` of the vector is shrunk before calling `drop_in_place`,
        //   such that no value will be dropped twice in case `drop_in_place`
        //   were to panic once (if it panics twice, the program aborts).
        unsafe {
            if len > self.len {
                return;
            }
            let remaining_len = self.len - len;
            let s = ptr::slice_from_raw_parts_mut(self.as_mut_ptr().add(len), remaining_len);
            self.len = len;
            ptr::drop_in_place(s);
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.buf.ptr()
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        // We shadow the slice method of the same name to avoid going through
        // `deref_mut`, which creates an intermediate reference.
        self.buf.ptr()
    }

    #[inline]
    pub fn alloc_ref(&self) -> &Allocator {
        self.buf.alloc_ref()
    }

    #[inline]
    pub unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.capacity());

        self.len = new_len;
    }

    #[inline]
    pub fn swap_remove(&mut self, index: usize) -> T {
        #[cold]
        #[inline(never)]
        fn assert_failed(index: usize, len: usize) -> ! {
            panic!(
                "swap_remove index (is {}) should be < len (is {})",
                index, len
            );
        }

        let len = self.len();
        if index >= len {
            assert_failed(index, len);
        }
        unsafe {
            // We replace self[index] with the last element. Note that if the
            // bounds check above succeeds there must be a last element (which
            // can be self[index] itself).
            let last = ptr::read(self.as_ptr().add(len - 1));
            let hole = self.as_mut_ptr().add(index);
            self.set_len(len - 1);
            ptr::replace(hole, last)
        }
    }

    pub fn insert(&mut self, index: usize, element: T) {
        #[cold]
        #[inline(never)]
        fn assert_failed(index: usize, len: usize) -> ! {
            panic!(
                "insertion index (is {}) should be <= len (is {})",
                index, len
            );
        }

        let len = self.len();
        if index > len {
            assert_failed(index, len);
        }

        // space for the new element
        if len == self.buf.capacity() {
            self.reserve(1);
        }

        unsafe {
            // infallible
            // The spot to put the new value
            {
                let p = self.as_mut_ptr().add(index);
                // Shift everything over to make space. (Duplicating the
                // `index`th element into two consecutive places.)
                ptr::copy(p, p.offset(1), len - index);
                // Write it in, overwriting the first copy of the `index`th
                // element.
                ptr::write(p, element);
            }
            self.set_len(len + 1);
        }
    }

    pub fn remove(&mut self, index: usize) -> T {
        #[cold]
        #[inline(never)]
        fn assert_failed(index: usize, len: usize) -> ! {
            panic!("removal index (is {}) should be < len (is {})", index, len);
        }

        let len = self.len();
        if index >= len {
            assert_failed(index, len);
        }
        unsafe {
            // infallible
            let ret;
            {
                // the place we are taking from.
                let ptr = self.as_mut_ptr().add(index);
                // copy it out, unsafely having a copy of the value on
                // the stack and in the vector at the same time.
                ret = ptr::read(ptr);

                // Shift everything down to fill in that spot.
                ptr::copy(ptr.offset(1), ptr, len - index - 1);
            }
            self.set_len(len - 1);
            ret
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let len = self.len();
        let mut del = 0;
        {
            let v = &mut **self;

            for i in 0..len {
                if !f(&v[i]) {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.truncate(len - del);
        }
    }

    #[inline]
    pub fn dedup_by_key<F, K>(&mut self, mut key: F)
    where
        F: FnMut(&mut T) -> K,
        K: PartialEq,
    {
        self.dedup_by(|a, b| key(a) == key(b))
    }

    pub fn dedup_by<F>(&mut self, mut same_bucket: F)
    where
        F: FnMut(&mut T, &mut T) -> bool,
    {
        let len = {
            let len = self.len();
            if len <= 1 {
                return;
            }

            let ptr = self.as_mut_ptr();
            let mut next_read: usize = 1;
            let mut next_write: usize = 1;

            // SAFETY: the `while` condition guarantees `next_read` and `next_write`
            // are less than `len`, thus are inside `self`. `prev_ptr_write` points to
            // one element before `ptr_write`, but `next_write` starts at 1, so
            // `prev_ptr_write` is never less than 0 and is inside the slice.
            // This fulfils the requirements for dereferencing `ptr_read`, `prev_ptr_write`
            // and `ptr_write`, and for using `ptr.add(next_read)`, `ptr.add(next_write - 1)`
            // and `prev_ptr_write.offset(1)`.
            //
            // `next_write` is also incremented at most once per loop at most meaning
            // no element is skipped when it may need to be swapped.
            //
            // `ptr_read` and `prev_ptr_write` never point to the same element. This
            // is required for `&mut *ptr_read`, `&mut *prev_ptr_write` to be safe.
            // The explanation is simply that `next_read >= next_write` is always true,
            // thus `next_read > next_write - 1` is too.
            unsafe {
                // Avoid bounds checks by using raw pointers.
                while next_read < len {
                    let ptr_read = ptr.add(next_read);
                    let prev_ptr_write = ptr.add(next_write - 1);
                    if !same_bucket(&mut *ptr_read, &mut *prev_ptr_write) {
                        if next_read != next_write {
                            let ptr_write = prev_ptr_write.offset(1);
                            mem::swap(&mut *ptr_read, &mut *ptr_write);
                        }
                        next_write += 1;
                    }
                    next_read += 1;
                }
            }

            next_write
        };

        self.truncate(len);
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        // This will panic or abort if we would allocate > isize::MAX bytes
        // or if the length increment would overflow for zero-sized types.
        if self.len == self.buf.capacity() {
            self.reserve(1);
        }
        unsafe {
            let end = self.as_mut_ptr().add(self.len);
            ptr::write(end, value);
            self.len += 1;
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            unsafe {
                self.len -= 1;
                Some(ptr::read(self.as_ptr().add(self.len())))
            }
        }
    }

    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        unsafe {
            self.append_elements(other.as_slice() as _);
            other.set_len(0);
        }
    }

    #[inline]
    unsafe fn append_elements(&mut self, other: *const [T]) {
        let count = (*other).len();
        self.reserve(count);
        let len = self.len();
        ptr::copy_nonoverlapping(other as *const T, self.as_mut_ptr().add(len), count);
        self.len += count;
    }

    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn resize_with<F>(&mut self, new_len: usize, f: F)
    where
        F: FnMut() -> T,
    {
        let len = self.len();
        if new_len > len {
            self.extend_with(new_len - len, ExtendFunc(f));
        } else {
            self.truncate(new_len);
        }
    }

    #[inline]
    fn spare_capacity_mut(&mut self) -> &mut [mem::MaybeUninit<T>] {
        unsafe {
            slice::from_raw_parts_mut(
                self.as_mut_ptr().add(self.len) as *mut mem::MaybeUninit<T>,
                self.buf.capacity() - self.len,
            )
        }
    }
}

impl<T: Clone> PageVec<T> {
    pub fn resize(&mut self, new_len: usize, value: T) {
        let len = self.len();

        if new_len > len {
            self.extend_with(new_len - len, ExtendElement(value))
        } else {
            self.truncate(new_len);
        }
    }

    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.extend_desugared(other.iter().cloned())
    }
}

trait ExtendWith<T> {
    fn next(&mut self) -> T;
    fn last(self) -> T;
}

struct ExtendElement<T>(T);
impl<T: Clone> ExtendWith<T> for ExtendElement<T> {
    fn next(&mut self) -> T {
        self.0.clone()
    }
    fn last(self) -> T {
        self.0
    }
}

struct ExtendDefault;
impl<T: Default> ExtendWith<T> for ExtendDefault {
    fn next(&mut self) -> T {
        Default::default()
    }
    fn last(self) -> T {
        Default::default()
    }
}

struct ExtendFunc<F>(F);
impl<T, F: FnMut() -> T> ExtendWith<T> for ExtendFunc<F> {
    fn next(&mut self) -> T {
        (self.0)()
    }
    fn last(mut self) -> T {
        (self.0)()
    }
}

impl<T> PageVec<T> {
    /// Extend the vector by `n` values, using the given generator.
    fn extend_with<E: ExtendWith<T>>(&mut self, n: usize, mut value: E) {
        self.reserve(n);

        unsafe {
            let mut ptr = self.as_mut_ptr().add(self.len());
            // Use SetLenOnDrop to work around bug where compiler
            // may not realize the store through `ptr` through self.set_len()
            // don't alias.
            let mut local_len = SetLenOnDrop::new(&mut self.len);

            // Write all elements except the last one
            for _ in 1..n {
                ptr::write(ptr, value.next());
                ptr = ptr.offset(1);
                // Increment the length in every step in case next() panics
                local_len.increment_len(1);
            }

            if n > 0 {
                // We can write the last element directly without cloning needlessly
                ptr::write(ptr, value.last());
                local_len.increment_len(1);
            }

            // len set by scope guard
        }
    }
}

// Set the length of the vec when the `SetLenOnDrop` value goes out of scope.
//
// The idea is: The length field in SetLenOnDrop is a local variable
// that the optimizer will see does not alias with any stores through the Vec's data
// pointer. This is a workaround for alias analysis issue #32155
struct SetLenOnDrop<'a> {
    len: &'a mut usize,
    local_len: usize,
}

impl<'a> SetLenOnDrop<'a> {
    #[inline]
    fn new(len: &'a mut usize) -> Self {
        SetLenOnDrop {
            local_len: *len,
            len,
        }
    }

    #[inline]
    fn increment_len(&mut self, increment: usize) {
        self.local_len += increment;
    }
}

impl Drop for SetLenOnDrop<'_> {
    #[inline]
    fn drop(&mut self) {
        *self.len = self.local_len;
    }
}

impl<T: PartialEq> PageVec<T> {
    #[inline]
    pub fn dedup(&mut self) {
        self.dedup_by(|a, b| a == b)
    }
}

pub(crate) fn from_elem_in<T: Clone>(elem: T, n: usize, alloc: Allocator) -> PageVec<T> {
    <T as SpecFromElem>::from_elem(elem, n, alloc)
}

trait SpecFromElem: Sized {
    fn from_elem(elem: Self, n: usize, alloc: Allocator) -> PageVec<Self>;
}

impl<T: Clone> SpecFromElem for T {
    fn from_elem(elem: Self, n: usize, alloc: Allocator) -> PageVec<Self> {
        let mut v = PageVec::with_capacity_in(n, alloc);
        v.extend_with(n, ExtendElement(elem));
        v
    }
}

impl<T> ops::Deref for PageVec<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len) }
    }
}

impl<T> ops::DerefMut for PageVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len) }
    }
}

mod convert {
    use super::PageVec;

    #[inline]
    pub(crate) fn to_vec_clone<T: Clone>(s: &[T], alloc: super::Allocator) -> PageVec<T> {
        struct DropGuard<'a, T> {
            vec: &'a mut PageVec<T>,
            num_init: usize,
        }
        impl<'a, T> Drop for DropGuard<'a, T> {
            #[inline]
            fn drop(&mut self) {
                // SAFETY:
                // items were marked initialized in the loop below
                unsafe {
                    self.vec.set_len(self.num_init);
                }
            }
        }
        let mut vec = PageVec::with_capacity_in(s.len(), alloc);
        let mut guard = DropGuard {
            vec: &mut vec,
            num_init: 0,
        };
        let slots: &mut [std::mem::MaybeUninit<T>] = guard.vec.spare_capacity_mut();
        // .take(slots.len()) is necessary for LLVM to remove bounds checks
        // and has better codegen than zip.
        for (i, b) in s.iter().enumerate().take(slots.len()) {
            guard.num_init = i;
            unsafe { slots[i].as_mut_ptr().write(b.clone()) };
        }
        #[allow(clippy::mem_forget)]
        {
            core::mem::forget(guard);
        }
        // SAFETY:
        // the vec was allocated and initialized above to at least this length.
        unsafe {
            vec.set_len(s.len());
        }
        vec
    }

    #[inline]
    pub(crate) fn to_vec_copy<T: Copy>(s: &[T], alloc: super::Allocator) -> PageVec<T> {
        let mut v = PageVec::with_capacity_in(s.len(), alloc);
        // SAFETY:
        // allocated above with the capacity of `s`, and initialize to `s.len()` in
        // ptr::copy_to_non_overlapping below.
        unsafe {
            s.as_ptr().copy_to_nonoverlapping(v.as_mut_ptr(), s.len());
            v.set_len(s.len());
        }
        v
    }
}

impl<T: Clone> Clone for PageVec<T> {
    fn clone(&self) -> PageVec<T> {
        let alloc = self.alloc_ref();
        convert::to_vec_clone(self, alloc.clone())
    }

    fn clone_from(&mut self, other: &Self) {
        // drop anything that will not be overwritten
        self.truncate(other.len());

        // self.len <= other.len due to the truncate above, so the
        // slices here are always in-bounds.
        let (init, tail) = other.split_at(self.len());

        // reuse the contained values' allocations/resources.
        self.clone_from_slice(init);
        self.extend_from_slice(tail);
    }
}

impl<T: hash::Hash> hash::Hash for PageVec<T> {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        hash::Hash::hash(&**self, state)
    }
}

impl<T, I: slice::SliceIndex<[T]>> Index<I> for PageVec<T> {
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        Index::index(&**self, index)
    }
}

impl<T, I: slice::SliceIndex<[T]>> IndexMut<I> for PageVec<T> {
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        IndexMut::index_mut(&mut **self, index)
    }
}

impl<T> Extend<T> for PageVec<T> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.extend_desugared(iter.into_iter())
    }
}

impl<T> PageVec<T> {
    // leaf method to which various SpecFrom/SpecExtend implementations delegate when
    // they have no further optimizations to apply
    fn extend_desugared<I: Iterator<Item = T>>(&mut self, mut iterator: I) {
        // This is the case for a general iterator.
        //
        // This function should be the moral equivalent of:
        //
        //      for item in iterator {
        //          self.push(item);
        //      }
        while let Some(element) = iterator.next() {
            let len = self.len();
            if len == self.capacity() {
                let (lower, _) = iterator.size_hint();
                self.reserve(lower.saturating_add(1));
            }
            unsafe {
                ptr::write(self.as_mut_ptr().add(len), element);
                // NB can't overflow since we would have had to alloc the address space
                self.set_len(len + 1);
            }
        }
    }
}

macro_rules! __impl_slice_eq1 {
    ($lhs:ty, $rhs:ty $(where $ty:ty: $bound:ident)?) => {
        impl<T, U> PartialEq<$rhs> for $lhs
        where
            T: PartialEq<U>,
            $($ty: $bound)?
        {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool { self[..] == other[..] }
        }
    }
}

__impl_slice_eq1! { PageVec<T>, PageVec<U> }
__impl_slice_eq1! { PageVec<T>, &[U] }
__impl_slice_eq1! { PageVec<T>, &mut [U] }
__impl_slice_eq1! { &[T], PageVec<U> }
__impl_slice_eq1! { &mut [T], PageVec<U> }
__impl_slice_eq1! { PageVec<T>, [U] }
__impl_slice_eq1! { [T], PageVec<U> }

impl<T: PartialOrd> cmp::PartialOrd for PageVec<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<T: cmp::Eq> cmp::Eq for PageVec<T> {}

impl<T: Ord> cmp::Ord for PageVec<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        Ord::cmp(&**self, &**other)
    }
}

#[cfg(test)]
mod test {
    use super::{Allocator, PageVec};

    #[test]
    fn setup() {
        let empty = PageVec::<i32>::new_in(Allocator::new());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.capacity(), 0);
    }

    #[test]
    fn simple() {
        let allocator = Allocator::new();
        assert_eq!(allocator.inner.borrow().pages_allocated(), 0);

        let mut v = PageVec::new_in(allocator.clone());
        for i in 0..256 {
            v.push(i);
            assert_eq!(Some(&i), v.last());
            assert_eq!(v.last(), v.get(i));
            assert_eq!(i, v[i]);
        }

        assert!(!v.is_empty());
        assert_eq!(v.len(), 256);
        assert_eq!(1, allocator.inner.borrow().pages_allocated());

        for (i, v) in v.iter().enumerate() {
            assert_eq!(i, *v);
        }
    }

    #[test]
    fn sanity_check() {
        let allocator = Allocator::new();
        let mut v = PageVec::new_in(allocator.clone());
        assert_eq!(0, allocator.inner.borrow().pages_allocated());

        v.push(1);
        assert_eq!(1, allocator.inner.borrow().pages_allocated());
        assert!(allocator
            .inner
            .borrow()
            .owns_pointer((&v[0] as *const i32).cast::<libc::c_void>()));
    }
}
