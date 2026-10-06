use crate::{
    process::{
        Process,
        fd::{Errno, FileOps},
    }, uart::{UART_DRIVER, UartDriver}, virtmemory::{copy_in_cont, copy_out_cont},
};

#[allow(non_camel_case_types)]
type tcflag_t = usize;
#[allow(non_camel_case_types)]
type cc_t = usize;

const NCCS: usize = 16;

#[derive(Debug, Default)]
struct Termios {
    // At least these members, in no particular order
    c_iflag: tcflag_t,
    c_oflag: tcflag_t,
    c_cflag: tcflag_t,
    c_lflag: tcflag_t,
    c_cc: [cc_t; NCCS],
}

#[derive(Debug)]
pub(super) struct TTY {
    termios: Termios,
    transport: &'static UartDriver,
}

impl TTY {
    pub(super) fn new() -> Self {
        Self {
            termios: Termios::default(),
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
        const TIOCGWINSZ: usize = 0x5413;
        if op != TIOCGWINSZ {
            return Err(Errno::NotATerminal);
        }

        // struct winsize { unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel; }
        let mut winsize = [0u8; 8];
        winsize[0..2].copy_from_slice(&24u16.to_ne_bytes());
        winsize[2..4].copy_from_slice(&80u16.to_ne_bytes());
        copy_out_cont(&mut proc.pagetable, arg, &winsize)
            .map(|()| 0)
            .map_err(|()| Errno::Fault)
    }
}
