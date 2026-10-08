use alloc::{boxed::Box, sync::Arc};

use crate::{
    KERNEL,
    lock::IntMutex,
    process::{
        ProcState, Process,
        fd::{Errno, FileDescriptor, FileOps},
    },
    structures::RingBuffer,
    virtmemory::{copy_in_cont, copy_out_cont},
};

pub const PIPE_CAPACITY: usize = 4096;
const PIPE_IO_CHUNK: usize = 256;

#[derive(Debug)]
pub struct PipeState {
    buffer: Box<RingBuffer<PIPE_CAPACITY>>,
    readers: usize,
    writers: usize,
}

#[derive(Debug)]
pub struct PipeReadEnd {
    state: Arc<IntMutex<PipeState>>,
}

#[derive(Debug)]
pub struct PipeWriteEnd {
    state: Arc<IntMutex<PipeState>>,
}

fn read_channel(state: &Arc<IntMutex<PipeState>>) -> usize {
    Arc::as_ptr(state) as usize
}

fn write_channel(state: &Arc<IntMutex<PipeState>>) -> usize {
    (Arc::as_ptr(state) as usize) | 1
}

fn wake(channel: usize, wake_all: bool) {
    KERNEL.get().unwrap().lock().wakeup(Some(channel), wake_all);
}

impl FileOps for PipeReadEnd {
    fn read(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }

        let mut bytes = [0u8; PIPE_IO_CHUNK];

        loop {
            let mut count = 0;
            let mut sleeping = false;

            {
                let mut state = self.state.lock();
                let requested = len.min(bytes.len());
                while count < requested {
                    let Some(byte) = state.buffer.pop() else {
                        break;
                    };
                    bytes[count] = byte;
                    count += 1;
                }

                if count == 0 && state.writers != 0 {
                    // Publish the sleeping state while holding the pipe lock,
                    // so a writer cannot add input between the empty check and
                    // this process going to sleep.
                    unsafe { proc.lock.lock_manual() };
                    proc.sleep_channel = Some(read_channel(&self.state));
                    proc.state = ProcState::Sleeping;
                    sleeping = true;
                }
            }

            if count != 0 {
                wake(write_channel(&self.state), false);
                return copy_out_cont(&mut proc.pagetable, addr, &bytes[..count])
                    .map(|()| count)
                    .map_err(|()| Errno::EFAULT);
            }

            if sleeping {
                unsafe { proc.sleep_locked() };
            } else {
                // An empty pipe with no writers is EOF.
                return Ok(0);
            }
        }
    }

    fn write(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EBADF)
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }
}

impl FileOps for PipeWriteEnd {
    fn read(&self, _proc: &mut Process, _addr: usize, _len: usize) -> Result<usize, Errno> {
        Err(Errno::EBADF)
    }

    fn write(&self, proc: &mut Process, addr: usize, len: usize) -> Result<usize, Errno> {
        if len == 0 {
            return Ok(0);
        }

        let requested = len.min(PIPE_IO_CHUNK);
        let bytes =
            copy_in_cont(&mut proc.pagetable, addr, requested).map_err(|()| Errno::EFAULT)?;

        loop {
            let mut written = 0;
            let mut sleeping = false;

            {
                let mut state = self.state.lock();
                if state.readers == 0 {
                    return Err(Errno::EPIPE);
                }

                while written < bytes.len() && state.buffer.push(bytes[written]) {
                    written += 1;
                }

                if written == 0 {
                    // The pipe is full. Publish sleep under the same lock that
                    // protects buffer space so a reader cannot lose the wakeup.
                    unsafe { proc.lock.lock_manual() };
                    proc.sleep_channel = Some(write_channel(&self.state));
                    proc.state = ProcState::Sleeping;
                    sleeping = true;
                }
            }

            if written != 0 {
                wake(read_channel(&self.state), false);
                return Ok(written);
            }

            if sleeping {
                unsafe { proc.sleep_locked() };
            }
        }
    }

    fn ioctl(&self, _proc: &mut Process, _op: usize, _arg: usize) -> Result<usize, Errno> {
        Err(Errno::ENOTTY)
    }
}

impl Drop for PipeReadEnd {
    fn drop(&mut self) {
        {
            let mut state = self.state.lock();
            debug_assert!(state.readers > 0);
            state.readers -= 1;
        }
        // Every blocked writer must observe that there are no more readers.
        wake(write_channel(&self.state), true);
    }
}

impl Drop for PipeWriteEnd {
    fn drop(&mut self) {
        {
            let mut state = self.state.lock();
            debug_assert!(state.writers > 0);
            state.writers -= 1;
        }
        // Every blocked reader must observe EOF once all writers are gone.
        wake(read_channel(&self.state), true);
    }
}

pub fn new_pipe() -> (Arc<FileDescriptor>, Arc<FileDescriptor>) {
    let mut buffer = Box::<RingBuffer<PIPE_CAPACITY>>::new_uninit();
    unsafe {
        core::ptr::write_bytes(buffer.as_mut_ptr(), 0, 1);
    }
    let buffer = unsafe { buffer.assume_init() };

    let state = Arc::new(IntMutex::new(PipeState {
        buffer,
        readers: 1,
        writers: 1,
    }));

    let read_target: Arc<dyn FileOps> = Arc::new(PipeReadEnd {
        state: state.clone(),
    });
    let write_target: Arc<dyn FileOps> = Arc::new(PipeWriteEnd { state });

    (
        Arc::new(FileDescriptor {
            target: read_target,
            readable: true,
            writable: false,
        }),
        Arc::new(FileDescriptor {
            target: write_target,
            readable: false,
            writable: true,
        }),
    )
}
