#![cfg(target_os = "macos")]
extern crate block;

use block::ConcreteBlock;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn stack_block_copies_calls_and_releases_captured_state() {
    let calls = Rc::new(Cell::new(0));
    let captured = Rc::clone(&calls);
    let stack = ConcreteBlock::new(move |value: i32| {
        captured.set(captured.get() + 1);
        value + 7
    });
    let heap = stack.copy();
    let retained = heap.clone();
    assert_eq!(unsafe { heap.call((5,)) }, 12);
    drop(heap);
    assert_eq!(unsafe { retained.call((10,)) }, 17);
    assert_eq!(calls.get(), 2);
    drop(retained);
    assert_eq!(Rc::strong_count(&calls), 1);
}
