use alloc::string::String;

use crate::{
    process::Process,
    uart::uart_write,
    virtmemory::{copy_in_bytes, copy_in_cont, copy_out_cont},
};

pub fn sys_write(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let addr = proc.trapframe.a1;
    let size = proc.trapframe.a2;

    if fd != 1 {
        panic!("Write to fd {fd}");
    }

    let bytes = copy_in_cont(&mut proc.pagetable, addr, size).unwrap();
    let msg = String::from_utf8(bytes).unwrap();

    uart_write(msg.as_bytes());
    proc.trapframe.a0 = 0;
}

/// Linux RISC-V writev for the UART-backed stdout descriptor.
pub fn sys_writev(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let mut iov_addr = proc.trapframe.a1;
    let iov_count = proc.trapframe.a2;

    if fd != 1 {
        proc.trapframe.a0 = (-9isize) as usize; // EBADF
        return;
    }
    if iov_count > 1024 {
        proc.trapframe.a0 = (-22isize) as usize; // EINVAL
        return;
    }

    let word_size = core::mem::size_of::<usize>();
    let iovec_size = word_size * 2;
    let mut written = 0usize;

    for _ in 0..iov_count {
        let record = match copy_in_bytes(&mut proc.pagetable, iov_addr, iovec_size) {
            Ok(record) => record,
            Err(()) => {
                proc.trapframe.a0 = if written == 0 {
                    (-14isize) as usize
                } else {
                    written
                };
                return;
            }
        };
        let base = usize::from_ne_bytes(record[..word_size].try_into().unwrap());
        let len = usize::from_ne_bytes(record[word_size..].try_into().unwrap());
        let mut offset = 0;
        while offset < len {
            let chunk_len = (len - offset).min(256);
            let bytes = match base
                .checked_add(offset)
                .and_then(|addr| copy_in_bytes(&mut proc.pagetable, addr, chunk_len).ok())
            {
                Some(bytes) => bytes,
                None => {
                    proc.trapframe.a0 = if written == 0 {
                        (-14isize) as usize
                    } else {
                        written
                    };
                    return;
                }
            };
            let Some(total) = written.checked_add(chunk_len) else {
                proc.trapframe.a0 = (-22isize) as usize; // EINVAL
                return;
            };
            uart_write(&bytes);
            written = total;
            offset += chunk_len;
        }
        iov_addr = match iov_addr.checked_add(iovec_size) {
            Some(addr) => addr,
            None => {
                proc.trapframe.a0 = if written == 0 {
                    (-14isize) as usize
                } else {
                    written
                };
                return;
            }
        };
    }

    proc.trapframe.a0 = written;
}

/// Return a default terminal size for the UART console's TIOCGWINSZ request.
pub fn sys_ioctl(proc: &mut Process) {
    let fd = proc.trapframe.a0;
    let op = proc.trapframe.a1;
    let arg = proc.trapframe.a2;

    if fd != 1 {
        proc.trapframe.a0 = (-9isize) as usize; // EBADF
        return;
    }

    const TIOCGWINSZ: usize = 0x5413;
    if op != TIOCGWINSZ {
        proc.trapframe.a0 = (-25isize) as usize; // ENOTTY
        return;
    }

    // struct winsize { unsigned short ws_row, ws_col, ws_xpixel, ws_ypixel; }
    // Use a conventional 24x80 terminal; pixel dimensions are unknown.
    let mut winsize = [0u8; 8];
    winsize[0..2].copy_from_slice(&24u16.to_ne_bytes());
    winsize[2..4].copy_from_slice(&80u16.to_ne_bytes());
    proc.trapframe.a0 = match copy_out_cont(&mut proc.pagetable, arg, &winsize) {
        Ok(()) => 0,
        Err(()) => (-14isize) as usize, // EFAULT
    };
}
