//! External tests of the production coalescing state and IPC inbox storage.
extern crate alloc;
#[path = "../../../serverlib/src/abi.rs"]
mod abi;
#[path = "../../src/errno.rs"]
mod errno;
#[path = "../../src/process/ipc/state.rs"]
mod ipc;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! { static FAIL: Cell<bool> = const { Cell::new(false) }; }
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if FAIL.try_with(|flag| flag.get()).unwrap_or(false) {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
#[path = "../../src/process/uart_poll/state.rs"]
mod poll;
#[path = "../../src/structures.rs"]
mod structures;

#[test]
fn ticks_coalesce_and_work_during_an_outstanding_request_is_retained() {
    let mut poll = poll::PollState::default();
    assert_eq!(poll.ready(), None);
    poll.configure(7);
    assert_eq!(poll.ready(), None);
    for _ in 0..100 {
        poll.tick();
    }
    assert_eq!(poll.ready(), Some(7));
    poll.sent(10);
    assert_eq!(poll.ready(), None);
    for _ in 0..100 {
        poll.tick();
    }
    assert_eq!(poll.ready(), None);
    poll.acknowledge(10);
    assert_eq!(poll.ready(), Some(7));
    poll.sent(11);
    poll.acknowledge(11);
    assert_eq!(poll.ready(), None);
}

#[test]
fn failure_leaves_pending_work_and_dead_destination_can_be_disabled() {
    let mut poll = poll::PollState::default();
    poll.configure(7);
    poll.tick();
    // An unsuccessful enqueue does not call sent or clear pending work.
    assert_eq!(poll.ready(), Some(7));
    assert_eq!(poll.ready(), Some(7));
    poll.sent(10);
    assert_eq!(poll.disable(), Some(10));
    assert_eq!(poll.destination(), None);
    assert_eq!(poll.outstanding(), None);
    poll.tick();
    assert_eq!(poll.ready(), None);
}

#[test]
fn collecting_acknowledgements_does_not_accumulate_completed_inbox_entries() {
    let mut poll = poll::PollState::default();
    let mut inbox = ipc::Inbox::default();
    let mut ids = structures::Counter::new();
    poll.configure(7);
    for _ in 0..1000 {
        poll.tick();
        let id = inbox
            .push(&mut ids, 0, abi::Message::default(), None)
            .unwrap();
        poll.sent(id);
        assert_eq!(inbox.receive(), Some(id));
        let request = inbox.get_mut(id).unwrap();
        request.state = ipc::RequestState::Completed;
        request.result = Some(0);
        let reply = inbox.remove(id).unwrap();
        assert_eq!(reply.result, Some(0));
        poll.acknowledge(id);
        assert!(inbox.get(id).is_none());
        assert_eq!(poll.outstanding(), None);
    }
}

#[test]
fn inbox_allocation_failure_can_retry_without_another_tick() {
    let mut poll = poll::PollState::default();
    let mut inbox = ipc::Inbox::default();
    let mut ids = structures::Counter::new();
    poll.configure(7);
    poll.tick();
    FAIL.with(|flag| flag.set(true));
    let result = inbox.push(&mut ids, 0, abi::Message::default(), None);
    FAIL.with(|flag| flag.set(false));
    assert!(matches!(result, Err(errno::Errno::ENOMEM)));
    assert_eq!(poll.ready(), Some(7));
    assert!(inbox.peek().is_none());
    let id = inbox
        .push(&mut ids, 0, abi::Message::default(), None)
        .unwrap();
    poll.sent(id);
    assert_eq!(poll.outstanding(), Some(id));
    assert_eq!(poll.ready(), None);
}
