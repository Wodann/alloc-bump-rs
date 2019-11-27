use alloc_bump::BumpAlloc;
use alloc_wg::alloc::Global;
use core::mem;

fn main() {
    let stack = 2.0f32;
    let heap = {
        let bump = BumpAlloc::<Global>::with_capacity_in(mem::size_of::<f32>(), Global);

        bump.alloc_t(stack).unwrap()
    };
    assert_eq!(stack, *heap);
}
