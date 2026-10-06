use crate::{
    lock::IntMutex,
    process::{
        Process,
        fd::{Errno, FileOps},
    }, uart::{UART_DRIVER, UartDriver}, virtmemory::{copy_in_cont, copy_out_cont},
};

// Linux RISC-V's kernel termios ABI, used by the TCGETS/TCSETS ioctls.
// musl's public struct termios has additional userspace-only fields after
// this prefix, so only this 36-byte kernel portion is copied by the ioctl.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 19],
}

#[derive(Debug)]
pub(super) struct TTY {
    termios: IntMutex<Termios>,
    transport: &'static UartDriver,
}

impl TTY {
    pub(super) fn new() -> Self {
        Self {
            termios: IntMutex::new(Termios::default()),
            transport: &UART_DRIVER,
        }
    }
}

impl FileOps for TTY {
    fn read(
        &self,
        proc: &mut super::Process,
        addr: usize,
        len: usize,
    ) -> Result<usize, super::fd::Errno> {
        if len == 0 {
            return Ok(0);
        }

        let bytes = self.transport.read_blocking(proc, len);
        const ECHO: u32 = 0o10;
        if self.termios.lock().c_lflag & ECHO != 0 {
            self.transport.write(&bytes);
        }
        copy_out_cont(&mut proc.pagetable, addr, &bytes)
            .map(|()| bytes.len())
            .map_err(|()| Errno::Fault)
    }

    fn write(
        &self,
        proc: &mut super::Process,
        addr: usize,
        len: usize,
    ) -> Result<usize, super::fd::Errno> {
        let bytes = copy_in_cont(&mut proc.pagetable, addr, len).map_err(|()| Errno::Fault)?;
        self.transport.write(&bytes);
        Ok(bytes.len())
    }

    fn ioctl(&self, proc: &mut Process, op: usize, arg: usize) -> Result<usize, Errno> {
        const TCGETS: usize = 0x5401;
        const TCSETS: usize = 0x5402;
        const TIOCGWINSZ: usize = 0x5413;

        match op {
            TCGETS => {
                let termios = *self.termios.lock();
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&termios as *const Termios).cast::<u8>(),
                        core::mem::size_of::<Termios>(),
                    )
                };
                copy_out_cont(&mut proc.pagetable, arg, bytes)
                    .map(|()| 0)
                    .map_err(|()| Errno::Fault)
            }
            TCSETS => {
                let bytes: alloc::vec::Vec<u8> = copy_in_cont(
                    &mut proc.pagetable,
                    arg,
                    core::mem::size_of::<Termios>(),
                )
                    .map_err(|()| Errno::Fault)?;
                let termios = unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<Termios>()) };
                *self.termios.lock() = termios;
                Ok(0)
            }
            TIOCGWINSZ => {
                // struct winsize { unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel; }
                let mut winsize = [0u8; 8];
                winsize[0..2].copy_from_slice(&24u16.to_ne_bytes());
                winsize[2..4].copy_from_slice(&80u16.to_ne_bytes());
                copy_out_cont(&mut proc.pagetable, arg, &winsize)
                    .map(|()| 0)
                    .map_err(|()| Errno::Fault)
            }
            _ => Err(Errno::NotATerminal),
        }
    }
}
