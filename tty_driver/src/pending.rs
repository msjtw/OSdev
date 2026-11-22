use alloc::collections::VecDeque;
use serverlib::{Error, Result};

use crate::tty::Tty;

#[derive(Debug, Clone, Copy)]
struct PendingRead {
    id: usize,
    count: usize,
}

#[derive(Debug, Default)]
pub struct PendingReads {
    requests: VecDeque<PendingRead>,
}

impl PendingReads {
    /// Retain a read's ID and count without copying or dereferencing application pointers.
    pub fn push(&mut self, id: usize, count: usize) -> Result<()> {
        self.requests.try_reserve(1).map_err(|_| Error::ENOMEM)?;
        self.requests.push_back(PendingRead { id, count });
        Ok(())
    }

    /// Complete ready reads in FIFO order through a synchronous reply transport.
    pub fn complete(
        &mut self,
        tty: &mut Tty,
        mut reply: impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
    ) -> Result<usize> {
        let mut completed = 0;
        while let Some(request) = self.requests.front().copied() {
            let count = request.count.min(tty.readable());
            if count == 0 && request.count != 0 {
                break;
            }
            match reply(request.id, Ok(count), Some(tty.readable_slice(count))) {
                Ok(()) => tty.consume(count),
                Err(error) => reply(request.id, Err(error), None)?,
            }
            self.requests.pop_front();
            completed += 1;
        }
        Ok(completed)
    }

    /// Fail current waiting reads for a recoverable UART operation error.
    pub fn fail(
        &mut self,
        error: Error,
        mut reply: impl FnMut(usize, Result<usize>, Option<&[u8]>) -> Result<()>,
    ) -> Result<()> {
        while let Some(request) = self.requests.front() {
            reply(request.id, Err(error), None)?;
            self.requests.pop_front();
        }
        Ok(())
    }
}
