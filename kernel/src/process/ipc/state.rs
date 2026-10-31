use alloc::{collections::VecDeque, vec::Vec};

use crate::{errno::Errno, structures::Counter};

use super::abi::{KERNEL_SENDER_ID, Message};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestState {
    Queued,
    Delivered,
    Completed,
    Cancelled,
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

#[derive(Debug, Default)]
pub struct IpcQueue {
    inbox: VecDeque<Request>,
    pub pending_call: Option<CurrentReq>,
    pub call_result: Option<usize>,
}

impl IpcQueue {

    pub fn push(
        &mut self,
        ids: &mut Counter,
        sender: usize,
        mut message: Message,
        mut data: Option<Vec<u8>>,
    ) -> Result<usize, Errno> {
        if data.is_some() && sender != KERNEL_SENDER_ID {
            return Err(Errno::EINVAL);
        }
        if let Some(data) = data.as_mut() {
            message.args[1] = data.as_mut_ptr() as usize;
            message.args[2] = data.len();
        }
        self.inbox.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
        let id = next_request_id(ids)?;
        message.sender = sender;
        self.inbox.push_back(Request {
            id,
            message,
            state: RequestState::Queued,
            result: None,
            kernel_data: data,
        });
        Ok(id)
    }

    pub fn peek(&self) -> Option<&Request> {
        self.inbox
            .iter()
            .find(|request| request.state == RequestState::Queued)
    }

    pub fn receive(&mut self) -> Option<usize> {
        let request = self
            .inbox
            .iter_mut()
            .find(|request| request.state == RequestState::Queued)?;
        request.state = RequestState::Delivered;
        Some(request.id)
    }

    pub fn get(&self, id: usize) -> Option<&Request> {
        self.inbox.iter().find(|request| request.id == id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Request> {
        self.inbox.iter_mut().find(|request| request.id == id)
    }

    pub fn remove(&mut self, id: usize) -> Option<Request> {
        let index = self.inbox.iter().position(|request| request.id == id)?;
        self.inbox.remove(index)
    }

    pub fn begin_call(&mut self, destination: usize, id: usize) -> Result<(), Errno> {
        if self.pending_call.is_some() {
            return Err(Errno::EAGAIN);
        }
        self.pending_call = Some(CurrentReq { destination, id });
        self.call_result = None;
        Ok(())
    }

    pub fn finish_call(&mut self, destination: usize, id: usize) -> bool {
        if self.pending_call != Some(CurrentReq { destination, id }) {
            return false;
        }
        self.pending_call = None;
        self.call_result = None;
        true
    }
}

pub(super) fn next_request_id(ids: &mut Counter) -> Result<usize, Errno> {
    ids.next()
        .filter(|id| *id <= isize::MAX as usize)
        .ok_or(Errno::EOVERFLOW)
}
