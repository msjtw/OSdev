use alloc::{collections::VecDeque, vec::Vec};

use crate::{errno::Errno, structures::Counter};

use super::abi::{IPC_EVENT_COMPLETION, IPC_EVENT_REQUEST, IpcEvent, KERNEL_SENDER_ID, Message};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestState {
    Queued,
    Delivered,
    Completed,
}

#[derive(Debug)]
pub struct Request {
    pub id: usize,
    pub message: Message,
    pub state: RequestState,
    pub result: Option<usize>,
    // for requests from kernel - additional data
    pub kernel_data: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentReq {
    pub destination: usize,
    pub id: usize,
}

#[derive(Debug)]
struct OutgoingRequest {
    current: CurrentReq,
    result: Option<usize>,
}

#[derive(Debug, Default)]
pub struct IpcState {
    pub inbox: Inbox,
    pub pending_call: Option<CurrentReq>,
    pub call_result: Option<usize>,
    pub outbox: Outbox,
    completion_next: bool,
}

#[derive(Debug, Default)]
pub struct Inbox {
    requests: VecDeque<Request>,
}

#[derive(Debug, Default)]
pub struct Outbox {
    requests: VecDeque<OutgoingRequest>,
}

impl IpcState {
    // Track the sole blocking call, rejecting a second unfinished call.
    pub fn begin_call(&mut self, destination: usize, id: usize) -> Result<(), Errno> {
        if self.pending_call.is_some() {
            return Err(Errno::EAGAIN);
        }
        self.pending_call = Some(CurrentReq { destination, id });
        self.call_result = None;
        Ok(())
    }

    // Clear blocking-call bookkeeping only when destination and ID match.
    pub fn finish_call(&mut self, destination: usize, id: usize) -> bool {
        if self.pending_call != Some(CurrentReq { destination, id }) {
            return false;
        }
        self.pending_call = None;
        self.call_result = None;
        true
    }
    // Check whether a blocking call or outbox entry still permits payload access.
    pub fn outgoing_active(&self, current: CurrentReq) -> bool {
        (self.pending_call == Some(current) && self.call_result.is_none())
            || self.outbox.is_active(current.destination, current.id)
    }

    // Record a matching outgoing result once and report whether a waiter should wake.
    pub fn complete_outgoing(&mut self, current: CurrentReq, result: usize) -> bool {
        if self.pending_call == Some(current) && self.call_result.is_none() {
            self.call_result = Some(result);
            true
        } else {
            self.outbox
                .complete(current.destination, current.id, result)
        }
    }

    // Select a ready event without consuming it, alternating kinds when both are ready.
    pub fn peek_event(&self) -> Option<IpcEvent> {
        let request = self.inbox.peek().map(|request| IpcEvent {
            kind: IPC_EVENT_REQUEST,
            id: request.id,
            result: 0,
            message: request.message,
        });
        let completion = self.outbox.peek_completion();
        if self.completion_next {
            completion.or(request)
        } else {
            request.or(completion)
        }
    }

    // Commit delivery only after the selected event has been copied to userspace.
    pub fn consume_event(&mut self, event: IpcEvent) {
        if event.kind == IPC_EVENT_REQUEST {
            if let Some(request) = self.inbox.get_mut(event.id) {
                request.state = RequestState::Delivered;
            }
            self.completion_next = true;
        } else if event.kind == IPC_EVENT_COMPLETION {
            self.outbox.remove(event.id);
            self.completion_next = false;
        }
    }
}

impl Inbox {
    // Enqueue a stamped request, consuming any supplied owned payload.
    pub fn push(
        &mut self,
        ids: &mut Counter,
        sender: usize,
        message: Message,
        mut data: Option<Vec<u8>>,
    ) -> Result<usize, Errno> {
        self.push_retained(ids, sender, message, &mut data)
    }

    // Enqueue a stamped request, transferring owned bytes only on success.
    pub fn push_retained(
        &mut self,
        ids: &mut Counter,
        sender: usize,
        message: Message,
        data: &mut Option<Vec<u8>>,
    ) -> Result<usize, Errno> {
        if data.is_some() && sender != KERNEL_SENDER_ID {
            return Err(Errno::EINVAL);
        }
        let id = next_request_id(ids)?;
        self.push_with_id(id, sender, message, data)?;
        Ok(id)
    }

    // Enqueue with an allocated ID, transferring owned bytes only on success.
    pub fn push_with_id(
        &mut self,
        id: usize,
        sender: usize,
        mut message: Message,
        data: &mut Option<Vec<u8>>,
    ) -> Result<(), Errno> {
        if data.is_some() && sender != KERNEL_SENDER_ID {
            return Err(Errno::EINVAL);
        }
        if let Some(data) = data.as_mut() {
            message.args[1] = data.as_mut_ptr() as usize;
            message.args[2] = data.len();
        }
        self.requests.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
        message.sender = sender;
        self.requests.push_back(Request {
            id,
            message,
            state: RequestState::Queued,
            result: None,
            kernel_data: data.take(),
        });
        Ok(())
    }

    // Borrow the oldest queued request without marking it delivered.
    pub fn peek(&self) -> Option<&Request> {
        self.requests
            .iter()
            .find(|request| request.state == RequestState::Queued)
    }

    // Mark the oldest queued request delivered and return its ID.
    pub fn receive(&mut self) -> Option<usize> {
        let request = self
            .requests
            .iter_mut()
            .find(|request| request.state == RequestState::Queued)?;
        request.state = RequestState::Delivered;
        Some(request.id)
    }

    // Borrow an inbox request by ID regardless of its delivery state.
    pub fn get(&self, id: usize) -> Option<&Request> {
        self.requests.iter().find(|request| request.id == id)
    }

    // Mutably borrow an inbox request by ID regardless of its delivery state.
    pub fn get_mut(&mut self, id: usize) -> Option<&mut Request> {
        self.requests.iter_mut().find(|request| request.id == id)
    }

    // Remove an inbox request and return its owned payload with it.
    pub fn remove(&mut self, id: usize) -> Option<Request> {
        let index = self.requests.iter().position(|request| request.id == id)?;
        self.requests.remove(index)
    }
}

impl Outbox {
    // Report whether no outgoing requests or unconsumed completions remain.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    // Remove an outgoing request and discard its unconsumed result, if any.
    pub fn remove(&mut self, id: usize) -> Option<CurrentReq> {
        let index = self
            .requests
            .iter()
            .position(|request| request.current.id == id)?;
        self.requests.remove(index).map(|request| request.current)
    }

    // Remove the oldest outbox entry without allocating during lifecycle cleanup.
    pub fn pop(&mut self) -> Option<CurrentReq> {
        self.requests.pop_front().map(|request| request.current)
    }

    // Reserve storage and insert an outbox entry before publishing its request.
    pub fn push(&mut self, destination: usize, id: usize) -> Result<(), Errno> {
        self.requests.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
        self.requests.push_back(OutgoingRequest {
            current: CurrentReq { destination, id },
            result: None,
        });
        Ok(())
    }

    // Check whether a matching outbox entry still awaits a reply.
    pub fn is_active(&self, destination: usize, id: usize) -> bool {
        self.requests
            .iter()
            .any(|s| s.current == CurrentReq { destination, id } && s.result.is_none())
    }

    // Store an async result once without allocating or replacing a completion.
    pub fn complete(&mut self, destination: usize, id: usize, result: usize) -> bool {
        let Some(request) = self
            .requests
            .iter_mut()
            .find(|s| s.current == CurrentReq { destination, id } && s.result.is_none())
        else {
            return false;
        };
        request.result = Some(result);
        true
    }

    // Borrow the first ready completion without removing its outbox entry.
    pub fn peek_completion(&self) -> Option<IpcEvent> {
        self.requests.iter().find_map(|request| {
            request.result.map(|result| IpcEvent {
                kind: IPC_EVENT_COMPLETION,
                id: request.current.id,
                result,
                message: Message::default(),
            })
        })
    }
}

// Allocate a non-reused request ID that fits the syscall success range.
pub(super) fn next_request_id(ids: &mut Counter) -> Result<usize, Errno> {
    ids.next()
        .filter(|id| *id <= isize::MAX as usize)
        .ok_or(Errno::EOVERFLOW)
}
