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
use state::{CurrentReq, Inbox, IpcState, RequestState};
use structures::Counter;

fn track(queue: &mut IpcState, destination: usize, id: usize) {
    queue.outbox.push(destination, id).unwrap();
}

#[test]
fn preallocated_id_correlates_outbox_and_destination_request() {
    let mut caller = IpcState::default();
    let mut server = Inbox::default();
    let mut ids = Counter::new();
    let id = state::next_request_id(&mut ids).unwrap();
    caller.outbox.push(7, id).unwrap();
    server
        .push_with_id(id, 5, Message::default(), &mut None)
        .unwrap();
    assert_eq!(server.peek().unwrap().id, id);
    assert_eq!(server.peek().unwrap().message.sender, 5);
    assert!(caller.outbox.is_active(7, id));
    assert_eq!(state::next_request_id(&mut ids).unwrap(), id + 1);
}

#[test]
fn failed_enqueue_can_roll_back_preinserted_outbox_entry() {
    let mut caller = IpcState::default();
    let mut server = Inbox::default();
    caller.outbox.push(7, 10).unwrap();
    let mut data = Some(vec![1, 2, 3]);
    assert!(matches!(
        server.push_with_id(10, 5, Message::default(), &mut data),
        Err(errno::Errno::EINVAL)
    ));
    assert!(server.peek().is_none());
    assert_eq!(data.as_deref(), Some(&[1, 2, 3][..]));
    caller.outbox.remove(10).unwrap();
    assert!(!caller.outbox.is_active(7, 10));
    assert!(!caller.outbox.complete(7, 10, 0));
}

#[test]
fn removing_outbox_entries_revokes_active_and_unconsumed_results_only_by_id() {
    let mut queue = IpcState::default();
    queue.begin_call(9, 99).unwrap();
    track(&mut queue, 7, 10);
    track(&mut queue, 8, 11);
    track(&mut queue, 7, 12);
    assert!(queue.outbox.complete(8, 11, 4));
    assert_eq!(
        queue.outbox.remove(10),
        Some(CurrentReq {
            destination: 7,
            id: 10
        })
    );
    assert!(!queue.outbox.complete(7, 10, 9));
    assert_eq!(queue.outbox.remove(10), None);
    assert_eq!(
        queue.outbox.remove(11),
        Some(CurrentReq {
            destination: 8,
            id: 11
        })
    );
    assert!(queue.peek_event().is_none());
    assert!(queue.outbox.is_active(7, 12));
    assert_eq!(
        queue.pending_call,
        Some(CurrentReq {
            destination: 9,
            id: 99
        })
    );
    assert!(!queue.outbox.is_empty());
}

#[test]
fn draining_outbox_includes_completed_records_and_leaves_inbox_intact() {
    let mut queue = IpcState::default();
    let mut ids = Counter::new();
    let incoming = queue
        .inbox
        .push(&mut ids, 5, Message::default(), None)
        .unwrap();
    track(&mut queue, 7, 10);
    track(&mut queue, 8, 11);
    assert!(queue.outbox.complete(8, 11, 4));
    assert_eq!(
        queue.outbox.pop(),
        Some(CurrentReq {
            destination: 7,
            id: 10
        })
    );
    assert_eq!(
        queue.outbox.pop(),
        Some(CurrentReq {
            destination: 8,
            id: 11
        })
    );
    assert_eq!(queue.outbox.pop(), None);
    assert!(queue.outbox.is_empty());
    assert!(!queue.outbox.complete(8, 11, 8));
    assert_eq!(queue.inbox.peek().unwrap().id, incoming);
}

#[test]
fn outbox_entries_are_independent_of_blocking_call() {
    let mut queue = IpcState::default();
    queue.begin_call(7, 11).unwrap();
    track(&mut queue, 8, 12);
    track(&mut queue, 9, 13);
    assert!(queue.outbox.is_active(8, 12));
    assert!(!queue.outbox.is_active(9, 12));
    assert!(queue.outbox.complete(9, 13, 4));
    assert!(!queue.outbox.is_active(9, 13));
    assert_eq!(
        queue.pending_call,
        Some(CurrentReq {
            destination: 7,
            id: 11
        })
    );
    assert!(queue.outbox.is_active(8, 12));
}

#[test]
fn event_peek_is_nonconsuming_and_completions_can_arrive_out_of_order() {
    let mut queue = IpcState::default();
    track(&mut queue, 7, 11);
    track(&mut queue, 7, 12);
    assert!(queue.outbox.complete(7, 12, (-12isize) as usize));
    let event = queue.peek_event().unwrap();
    assert_eq!(event.kind, abi::IPC_EVENT_COMPLETION);
    assert_eq!(event.id, 12);
    assert_eq!(event.result, (-12isize) as usize);
    assert_eq!(event.message, Message::default());
    assert_eq!(queue.peek_event(), Some(event)); // Failed copy-out leaves it intact.
    queue.consume_event(event);
    assert!(queue.peek_event().is_none());
    assert!(queue.outbox.is_active(7, 11));
    assert!(!queue.outbox.complete(7, 12, 99));
}

#[test]
fn event_receive_preserves_request_fifo_and_alternates_ready_kinds() {
    let mut queue = IpcState::default();
    let mut ids = Counter::new();
    let first = queue
        .inbox
        .push(&mut ids, 10, Message::default(), None)
        .unwrap();
    let second = queue
        .inbox
        .push(&mut ids, 11, Message::default(), None)
        .unwrap();
    track(&mut queue, 7, 100);
    track(&mut queue, 7, 101);
    assert!(queue.outbox.complete(7, 100, 1));
    assert!(queue.outbox.complete(7, 101, 2));
    for (kind, id) in [
        (abi::IPC_EVENT_REQUEST, first),
        (abi::IPC_EVENT_COMPLETION, 100),
        (abi::IPC_EVENT_REQUEST, second),
        (abi::IPC_EVENT_COMPLETION, 101),
    ] {
        let event = queue.peek_event().unwrap();
        assert_eq!((event.kind, event.id), (kind, id));
        queue.consume_event(event);
    }
    assert!(queue.peek_event().is_none());
    assert_eq!(
        queue.inbox.get(first).unwrap().state,
        RequestState::Delivered
    );
    assert_eq!(
        queue.inbox.get(second).unwrap().state,
        RequestState::Delivered
    );
}

#[test]
fn repeated_completion_does_not_overwrite_the_first_result() {
    let mut queue = IpcState::default();
    track(&mut queue, 7, 100);
    assert!(!queue.outbox.complete(8, 100, 3));
    assert!(queue.outbox.complete(7, 100, 4));
    assert!(!queue.outbox.complete(7, 100, 5));
    assert_eq!(queue.peek_event().unwrap().result, 4);
}

#[test]
fn retained_payload_transfers_only_on_success() {
    let mut inbox = Inbox::default();
    let mut ids = Counter::default();
    let mut data = Some(vec![1, 2, 3]);
    assert!(
        inbox
            .push_retained(&mut ids, 7, Message::default(), &mut data)
            .is_err()
    );
    assert_eq!(data.as_deref(), Some([1, 2, 3].as_slice()));
    let id = inbox
        .push_retained(&mut ids, 0, Message::default(), &mut data)
        .unwrap();
    assert!(data.is_none());
    assert_eq!(
        inbox.get(id).unwrap().kernel_data.as_deref(),
        Some([1, 2, 3].as_slice())
    );
}

#[test]
fn fifo_grows_beyond_the_original_suggested_capacity() {
    let mut inbox = Inbox::default();
    let mut ids = Counter::default();
    for id in 1..=1000 {
        assert_eq!(
            inbox.push(&mut ids, 1, Message::default(), None).unwrap(),
            id
        );
    }
    for id in 1..=1000 {
        assert_eq!(inbox.receive().unwrap(), id);
        assert_eq!(inbox.remove(id).unwrap().id, id);
    }
    assert!(inbox.receive().is_none());
}

#[test]
fn removing_cancelled_entries_preserves_fifo_order() {
    let mut inbox = Inbox::default();
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
    let mut inbox = Inbox::default();
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
    let mut inbox = Inbox::default();
    let mut other = Inbox::default();
    let first = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    let second = other.push(&mut ids, 1, Message::default(), None).unwrap();
    inbox.remove(first).unwrap();
    assert!(inbox.get(first).is_none());
    assert_eq!(other.get(second).unwrap().id, second);
    inbox = Inbox::default();
    let third = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    assert!(first > 0 && third > second && second > first);
    assert!(third <= isize::MAX as usize);
    assert!(inbox.remove(first).is_none());
}

#[test]
fn incoming_requests_do_not_replace_pending_outgoing_call() {
    let mut ipc = IpcState::default();
    let mut ids = Counter::default();
    ipc.begin_call(8, 1).unwrap();
    assert!(matches!(ipc.begin_call(8, 2), Err(errno::Errno::EAGAIN)));
    let incoming = ipc
        .inbox
        .push(&mut ids, 7, Message::default(), None)
        .unwrap();
    assert_eq!(ipc.inbox.receive().unwrap(), incoming);
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
    let mut inbox = Inbox::default();
    let id = inbox.push(&mut ids, 1, Message::default(), None).unwrap();
    assert_eq!(inbox.receive().unwrap(), id);
    assert!(inbox.receive().is_none());
    assert_eq!(inbox.get(id).unwrap().state, RequestState::Delivered);
    let request = inbox.get_mut(id).unwrap();
    request.state = RequestState::Completed;
    request.result = Some(42);
    assert_eq!(inbox.get(id).unwrap().result, Some(42));
    assert_eq!(inbox.remove(id).unwrap().state, RequestState::Completed);
    assert!(inbox.get(id).is_none());
}

#[test]
fn outgoing_helpers_correlate_calls_and_outbox_without_overwriting_results() {
    let mut queue = IpcState::default();
    let call = CurrentReq {
        destination: 7,
        id: 10,
    };
    let outgoing = CurrentReq {
        destination: 8,
        id: 11,
    };
    queue.begin_call(call.destination, call.id).unwrap();
    track(&mut queue, outgoing.destination, outgoing.id);
    assert!(queue.outgoing_active(call));
    assert!(queue.outgoing_active(outgoing));
    let wrong = CurrentReq {
        destination: 9,
        id: 10,
    };
    assert!(!queue.outgoing_active(wrong));
    assert!(!queue.complete_outgoing(wrong, 1));
    assert!(queue.complete_outgoing(call, 2));
    assert!(!queue.outgoing_active(call));
    assert!(!queue.complete_outgoing(call, 3));
    assert_eq!(queue.call_result, Some(2));
    assert!(queue.outgoing_active(outgoing));
    assert!(queue.complete_outgoing(outgoing, 4));
    assert!(!queue.complete_outgoing(outgoing, 5));
    assert!(!queue.outgoing_active(outgoing));
    assert_eq!(queue.peek_event().unwrap().result, 4);
}

#[test]
fn delivered_front_does_not_block_later_queued_requests() {
    let mut ids = Counter::default();
    let mut inbox = Inbox::default();
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
    let mut inbox = Inbox::default();
    let data = vec![1u8, 2, 3, 4];
    let address = data.as_ptr() as usize;
    let id = inbox
        .push(
            &mut ids,
            abi::KERNEL_SENDER_ID,
            Message::default(),
            Some(data),
        )
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
    let mut inbox = Inbox::default();
    let message = Message {
        sender: abi::KERNEL_SENDER_ID,
        ..Message::default()
    };
    for data in [vec![], vec![1, 2, 3]] {
        assert!(matches!(
            inbox.push(&mut ids, 7, message, Some(data)),
            Err(errno::Errno::EINVAL)
        ));
    }
    assert!(inbox.peek().is_none());
    // Rejection must not allocate an ID or enqueue a request.
    assert_eq!(inbox.push(&mut ids, 7, message, None).unwrap(), 1);
}
