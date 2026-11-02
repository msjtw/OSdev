//! Host-side data-structure tests; no kernel runtime hooks.
extern crate alloc;

#[path = "../../../serverlib/src/abi.rs"]
mod abi;
#[path = "../../src/errno.rs"]
mod errno;
#[path = "../../src/process/ipc/state.rs"]
mod state;
#[path = "../../src/structures.rs"]
mod structures;

use abi::Message;
use state::{CurrentReq, IpcQueue, RequestState};
use structures::Counter;

#[test]
fn fifo_grows_beyond_the_original_suggested_capacity() {
    let mut inbox = IpcQueue::default();
    let mut ids = Counter::default();
    for id in 1..=1000 {
        assert_eq!(inbox.push(&mut ids, 1, Message::default(), None).unwrap(), id);
    }
    for id in 1..=1000 {
        assert_eq!(inbox.receive().unwrap(), id);
        assert_eq!(inbox.remove(id).unwrap().id, id);
    }
    assert!(inbox.receive().is_none());
}

#[test]
fn removing_cancelled_entries_preserves_fifo_order() {
    let mut inbox = IpcQueue::default();
    let mut ids = Counter::default();
    for _ in 1..=5 {
        inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    }
    inbox.remove(3);
    inbox.remove(99);
    assert_eq!(
        (0..4).map(|_| inbox.receive().unwrap()).collect::<Vec<_>>(),
        [1, 2, 4, 5]
    );
}

#[test]
fn metadata_stamps_sender_without_changing_operation_arguments() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let message = Message {
        sender: 999,
        operation: 64,
        args: [1, 2, 3, 4, 5, 6],
    };
    let id = inbox.push(&mut ids, 7, message, None).unwrap();
    let request = inbox.get(id).unwrap();
    assert_eq!(request.id, id);
    assert_eq!(request.message.sender, 7);
    assert_eq!(request.message.operation, 64);
    assert_eq!(request.message.args, message.args);
    assert_eq!(request.state, RequestState::Queued);
    assert_eq!(request.result, None);
    assert_eq!(inbox.receive().unwrap(), id);
    assert_eq!(inbox.get(id).unwrap().state, RequestState::Delivered);
}

#[test]
fn ids_are_not_reused_across_inboxes_or_process_slot_reuse() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let mut other = IpcQueue::default();
    let first = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    let second = other.push(&mut ids, 1, Message::default(), None).unwrap();
    inbox.remove(first).unwrap();
    assert!(inbox.get(first).is_none());
    assert_eq!(other.get(second).unwrap().id, second);
    inbox = IpcQueue::default();
    let third = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    assert!(first > 0 && third > second && second > first);
    assert!(third <= isize::MAX as usize);
    assert!(inbox.remove(first).is_none());
}

#[test]
fn incoming_requests_do_not_replace_pending_outgoing_call() {
    let mut ipc = IpcQueue::default();
    let mut ids = Counter::default();
    ipc.begin_call(8, 1).unwrap();
    assert!(matches!(ipc.begin_call(8, 2), Err(errno::Errno::EAGAIN)));
    let incoming = ipc.push(&mut ids, 7, Message::default(), None).unwrap();
    assert_eq!(ipc.receive().unwrap(), incoming);
    assert_eq!(
        ipc.pending_call,
        Some(CurrentReq {
            destination: 8,
            id: 1
        })
    );
    assert!(!ipc.finish_call(9, 1));
    assert!(!ipc.finish_call(8, 2));
    assert!(ipc.finish_call(8, 1));
    ipc.begin_call(8, 2).unwrap();
    assert!(!ipc.finish_call(8, 1));
    assert_eq!(
        ipc.pending_call,
        Some(CurrentReq {
            destination: 8,
            id: 2
        })
    );
}

#[test]
fn delivered_request_stays_in_inbox_until_completion_or_cancellation() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let id = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    assert_eq!(inbox.receive().unwrap(), id);
    assert!(inbox.receive().is_none());
    assert_eq!(inbox.get(id).unwrap().state, RequestState::Delivered);
    let request = inbox.get_mut(id).unwrap();
    request.state = RequestState::Completed;
    request.result = Some(42);
    assert_eq!(inbox.get(id).unwrap().result, Some(42));
    inbox.get_mut(id).unwrap().state = RequestState::Cancelled;
    assert_eq!(inbox.remove(id).unwrap().state, RequestState::Cancelled);
    assert!(inbox.get(id).is_none());
}

#[test]
fn delivered_front_does_not_block_later_queued_requests() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let first = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    let second = inbox.push(&mut ids, 2, Message::default(), None).unwrap();
    assert_eq!(inbox.receive().unwrap(), first);
    assert_eq!(inbox.receive().unwrap(), second);
    assert!(inbox.get(first).is_some());
    assert!(inbox.get(second).is_some());
    inbox.remove(second);
    assert!(inbox.get(first).is_some());
}

#[test]
fn kernel_payload_is_owned_until_request_is_removed() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let data = vec![1u8, 2, 3, 4];
    let address = data.as_ptr() as usize;
    let id = inbox
        .push(&mut ids, abi::KERNEL_SENDER_ID, Message::default(), Some(data))
        .unwrap();
    let request = inbox.get(id).unwrap();
    assert_eq!(request.message.sender, abi::KERNEL_SENDER_ID);
    assert_eq!(request.message.args[1], address);
    assert_eq!(request.message.args[2], 4);
    assert_eq!(request.kernel_data.as_deref(), Some(&[1, 2, 3, 4][..]));
    assert_eq!(inbox.receive(), Some(id));
    let request = inbox.remove(id).unwrap();
    assert_eq!(request.kernel_data.unwrap(), [1, 2, 3, 4]);
    assert!(inbox.get(id).is_none());
}

#[test]
fn non_kernel_sender_cannot_attach_owned_data() {
    let mut ids = Counter::default();
    let mut inbox = IpcQueue::default();
    let message = Message { sender: abi::KERNEL_SENDER_ID, ..Message::default() };
    for data in [vec![], vec![1, 2, 3]] {
        assert!(matches!(inbox.push(&mut ids, 7, message, Some(data)), Err(errno::Errno::EINVAL)));
    }
    assert!(inbox.peek().is_none());
    // Rejection must not allocate an ID or enqueue a request.
    assert_eq!(inbox.push(&mut ids, 7, message, None).unwrap(), 1);
}
