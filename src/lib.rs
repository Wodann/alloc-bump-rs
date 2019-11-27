#![no_std]

use core::cell::Cell;
use core::mem;
use core::num::NonZeroUsize;
use core::ptr::{self, NonNull};

use alloc_wg::alloc::{AllocRef, BuildAllocRef, DeallocRef, Global, NonZeroLayout, ReallocRef};

pub enum BumpAllocErr<A: AllocRef> {
    ZeroCapacity,

    AllocError {
        layout: NonZeroLayout,
        inner: A::Error,
    },
}

#[derive(Clone, Debug)]
pub struct BumpAlloc<A: DeallocRef = Global> {
    data: NonNull<u8>,
    layout: NonZeroLayout,
    ptr: Cell<NonNull<u8>>,
    build_alloc: A::BuildAlloc,
}

impl<A: AllocRef> BumpAlloc<A> {
    pub fn with_capacity_in(capacity: usize, a: A) -> Self {
        match Self::try_with_capacity_in(capacity, a) {
            Ok(bump) => bump,
            Err(BumpAllocErr::ZeroCapacity) => panic!("zero capacity"),
            Err(BumpAllocErr::AllocError { .. }) => unreachable!("Infallible allocation"),
        }
    }

    pub fn try_with_capacity_in(capacity: usize, a: A) -> Result<Self, BumpAllocErr<A>> {
        if capacity == 0 {
            return Err(BumpAllocErr::ZeroCapacity);
        }

        let layout = unsafe {
            NonZeroLayout::from_size_align_unchecked(
                NonZeroUsize::new_unchecked(capacity),
                NonZeroUsize::new_unchecked(1),
            )
        };

        let data = a
            .alloc(layout)
            .map_err(|inner| BumpAllocErr::AllocError { layout, inner })?;

        let new_ptr = data.clone().as_ptr() as usize;
        let new_ptr = new_ptr + layout.size().get();

        Ok(Self {
            data,
            layout,
            ptr: Cell::new(unsafe { NonNull::new_unchecked(new_ptr as *mut u8) }),
            build_alloc: a.get_build_alloc(),
        })
    }

    #[allow(clippy::mut_from_ref)]
    pub fn alloc_t<T>(&self, val: T) -> Result<&mut T, <&Self as AllocRef>::Error> {
        assert!(mem::size_of::<T>() > 0);

        unsafe {
            let layout = NonZeroLayout::new_unchecked::<T>();

            let ptr = self.alloc(layout)?;
            let ptr = ptr.cast::<T>().as_ptr();

            ptr::write(ptr, val);
            Ok(&mut *ptr)
        }
    }

    pub fn reset(&mut self) {
        unsafe {
            self.reset_unchecked();
        }
    }

    pub unsafe fn reset_unchecked(&self) {
        let new_ptr = self.data.as_ptr() as usize;
        let new_ptr = new_ptr + self.layout.size().get();
        self.ptr.set(NonNull::new_unchecked(new_ptr as *mut u8));
    }
}

impl<A: DeallocRef> BuildAllocRef for &BumpAlloc<A> {
    type Ref = Self;

    unsafe fn build_alloc_ref(
        &self,
        _ptr: NonNull<u8>,
        _layout: Option<NonZeroLayout>,
    ) -> Self::Ref {
        self
    }
}

impl<A: DeallocRef> DeallocRef for &BumpAlloc<A> {
    type BuildAlloc = Self;

    fn get_build_alloc(&self) -> Self::BuildAlloc {
        self
    }

    unsafe fn dealloc(&self, _ptr: NonNull<u8>, _layout: NonZeroLayout) {}
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocErr;

impl<A: DeallocRef> AllocRef for &BumpAlloc<A> {
    type Error = AllocErr;

    fn alloc(&self, layout: NonZeroLayout) -> Result<NonNull<u8>, Self::Error> {
        let ptr = self.ptr.get().as_ptr() as usize;
        let new_ptr = ptr.checked_sub(layout.size().get()).ok_or(AllocErr)?;

        // Round down to the requested alignment.
        let new_ptr = new_ptr & !(layout.align().get() - 1);

        let start = self.data.as_ptr() as usize;
        if new_ptr < start {
            // Not enough capacity
            return Err(AllocErr);
        }

        self.ptr
            .set(unsafe { NonNull::new_unchecked(new_ptr as *mut u8) });
        Ok(self.ptr.get())
    }
}

impl<A: DeallocRef> ReallocRef for &BumpAlloc<A> {
    unsafe fn realloc(
        &self,
        _ptr: NonNull<u8>,
        _old_layout: NonZeroLayout,
        new_layout: NonZeroLayout,
    ) -> Result<NonNull<u8>, Self::Error> {
        self.alloc(new_layout)
    }
}

#[cfg(test)]
mod tests {
    use super::BumpAlloc;
    use alloc_wg::alloc::Global;
    use core::mem;

    #[test]
    fn bump_alloc() {
        let bump = BumpAlloc::<Global>::with_capacity_in(mem::size_of::<f32>() * 2, Global);

        let stack_a = 1.2f32;
        let alloc_a = bump.alloc_t(stack_a.clone()).unwrap();
        assert_eq!(stack_a, *alloc_a);

        let stack_b = 2.4f32;
        let alloc_b = bump.alloc_t(stack_b.clone()).unwrap();
        assert_eq!(stack_b, *alloc_b);
    }

    #[test]
    fn bump_reset() {
        let mut bump = BumpAlloc::<Global>::with_capacity_in(mem::size_of::<f32>(), Global);

        for idx in 0..=2 {
            bump.reset();

            let new_stack: f32 = idx as f32;
            let new_alloc = bump.alloc_t(new_stack.clone()).unwrap();
            assert_eq!(new_stack, *new_alloc);
        }
    }

    #[test]
    fn bump_reset_unchecked() {
        let bump = BumpAlloc::<Global>::with_capacity_in(mem::size_of::<f32>(), Global);

        let mut prev: Option<(f32, &mut f32)> = None;

        for idx in 0..=2 {
            unsafe { bump.reset_unchecked() };

            let new_stack: f32 = idx as f32;
            let new_alloc = bump.alloc_t(new_stack.clone()).unwrap();
            assert_eq!(new_stack, *new_alloc);

            if let Some((prev_stack, prev_alloc)) = prev {
                assert_eq!(new_stack, *prev_alloc);
                assert_ne!(*new_alloc, prev_stack);
            }

            prev = Some((new_stack, new_alloc));
        }
    }

    #[test]
    fn bump_string_realloc() {
        use alloc_wg::string::String;
        let bump = BumpAlloc::<Global>::with_capacity_in(256, Global);

        let test = "test";
        let mut string = String::new_in(&bump);
        string.push_str(test);
        assert_eq!(string, test);
    }

    #[test]
    #[should_panic]
    fn bump_invalid_alloc_nomemory() {
        let bump = BumpAlloc::<Global>::with_capacity_in(mem::size_of::<f32>(), Global);

        for idx in 0..=1 {
            let stack: f32 = idx as f32;
            let _alloc = bump.alloc_t(stack).unwrap();
        }
    }

    #[test]
    fn bump_invalid_code() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/compiler/*.rs");
    }
}
