#![no_std]
#![no_main]

extern crate alloc;

use serverlib::{
    Args, Error, Result,
    abi::{SYS_READ, SYS_WRITE},
    ipc, runtime, syscall, tty,
};
use tty_driver::{
    pending::PendingReads,
    readiness::ReadinessPublisher,
    tty::Tty,
    uart::{CompletionAction, UartTransfers},
};

serverlib::entry!(run);

/// Validate startup configuration and serve requests through the shared event loop.
fn run(args: Args) -> ! {
    if !matches!(args.len(), 2 | 4) {
        syscall::exit(runtime::EXIT_USAGE);
    }
    let uart_pid = match args.parse_usize(1) {
        Ok(pid) if pid != 0 => pid,
        _ => syscall::exit(runtime::EXIT_USAGE),
    };
    let mut terminal = Tty::default();
    if args.len() == 4 {
        let sid = args.parse_usize(2).unwrap_or(0);
        let pgid = args.parse_usize(3).unwrap_or(0);
        if sid == 0 || pgid == 0 {
            syscall::exit(runtime::EXIT_USAGE);
        }
        terminal.session_id = Some(sid);
        terminal.foreground_pgid = Some(pgid);
    }
    let mut reads = PendingReads::default();
    let mut uart = UartTransfers::new(uart_pid);
    let mut readiness = ReadinessPublisher::default();
    if readiness
        .publish(&terminal, tty::publish_readiness)
        .is_err()
    {
        syscall::exit(runtime::EXIT_FAILURE);
    }
    loop {
        let mut retry_output = true;
        match ipc::receive_event() {
            Ok(ipc::Event::Request(request)) => {
                handle_request(&mut terminal, &mut reads, &mut uart, request)
            }
            Ok(ipc::Event::Completion { id, result }) => {
                retry_output =
                    match uart.complete(id, result, |id, result| ipc::reply(id, result, None)) {
                        Ok(CompletionAction::PumpOutput) => true,
                        Ok(CompletionAction::WaitForEvent) => false,
                        Ok(CompletionAction::InputFailed(error)) => {
                            if reads.fail(error, ipc::reply).is_err() {
                                syscall::exit(runtime::EXIT_FAILURE);
                            }
                            // Future input activation retries on a subsequent event, not this error.
                            false
                        }
                        Err(_) => syscall::exit(runtime::EXIT_FAILURE),
                    };
            }
            Err(Error::EINTR) => continue,
            Err(_) => syscall::exit(runtime::EXIT_FAILURE),
        }
        // Input startup/rearming stays inactive until TTY-020.
        let effects =
            match tty_driver::progress::input(&mut uart, &mut terminal, &mut reads, ipc::reply) {
                Ok(effects) => effects,
                Err(_) => syscall::exit(runtime::EXIT_FAILURE),
            };
        if terminal
            .deliver_interrupts(effects.interrupts, tty::signal_group)
            .is_err()
        {
            syscall::exit(runtime::EXIT_FAILURE);
        }
        if readiness
            .publish(&terminal, tty::publish_readiness)
            .is_err()
        {
            syscall::exit(runtime::EXIT_FAILURE);
        }
        if retry_output
            && uart
                .pump(
                    |pid, message| unsafe { ipc::submit(pid, message) },
                    |id, result| ipc::reply(id, result, None),
                )
                .is_err()
        {
            syscall::exit(runtime::EXIT_FAILURE);
        }
    }
}

/// Serve local terminal operations and complete copy failures with their original error.
fn handle_request(
    terminal: &mut Tty,
    reads: &mut PendingReads,
    uart: &mut UartTransfers,
    request: ipc::Request,
) {
    if let Err(error) = dispatch(terminal, reads, uart, request) {
        if ipc::reply(request.id, Err(error), None).is_ok() {
            return;
        }
        syscall::exit(runtime::EXIT_FAILURE);
    }
    // The main loop completes reads and publishes final readiness after this event.
}

/// Dispatch client I/O and terminal ioctls without blocking the event loop.
fn dispatch(
    terminal: &mut Tty,
    reads: &mut PendingReads,
    uart: &mut UartTransfers,
    request: ipc::Request,
) -> Result<()> {
    let message = request.message;
    if matches!(message.operation, SYS_READ | SYS_WRITE) && message.args[2] == 0 {
        return ipc::reply(request.id, Ok(0), None);
    }
    if message.operation == SYS_READ {
        return reads.push(request.id, message.args[2]);
    }
    if message.operation == SYS_WRITE {
        return uart.write(request.id, message.args[2], ipc::read_payload);
    }
    if message.operation != tty::IOCTL {
        return Err(Error::ENOSYS);
    }
    match message.args[3] {
        tty::TCGETS => ipc::reply(request.id, Ok(0), Some(&terminal.termios().to_bytes())),
        tty::TCSETS => {
            if message.args[2] < tty::Termios::SIZE {
                return Err(Error::EINVAL);
            }
            let mut bytes = [0; tty::Termios::SIZE];
            ipc::read_payload(request.id, 0, &mut bytes)?;
            terminal.set_termios(tty::Termios::from_bytes(&bytes)?);
            ipc::reply(request.id, Ok(0), None)
        }
        tty::TIOCGWINSZ => {
            let mut bytes = [0; 8];
            bytes[..2].copy_from_slice(&24u16.to_ne_bytes());
            bytes[2..4].copy_from_slice(&80u16.to_ne_bytes());
            ipc::reply(request.id, Ok(0), Some(&bytes))
        }
        tty::TIOCGPGRP => ipc::reply(
            request.id,
            Ok(0),
            Some(&(terminal.foreground_pgid.unwrap_or(0) as u32).to_ne_bytes()),
        ),
        tty::TIOCSPGRP => {
            if message.args[2] < 4 {
                return Err(Error::EINVAL);
            }
            let mut bytes = [0; 4];
            ipc::read_payload(request.id, 0, &mut bytes)?;
            terminal.set_foreground(
                request.id,
                u32::from_ne_bytes(bytes) as usize,
                tty::caller_session,
                tty::validate_foreground,
            )?;
            ipc::reply(request.id, Ok(0), None)
        }
        _ => Err(Error::ENOTTY),
    }
}
