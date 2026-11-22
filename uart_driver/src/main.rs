#![no_std]
#![no_main]

extern crate alloc;

use serverlib::{Args, Error, ipc, runtime, syscall, uart::POLL};
use uart_driver::{service::Service, uart::Uart};

serverlib::entry!(run);

fn run(args: Args) -> ! {
    if !matches!(args.len(), 2 | 3) {
        syscall::exit(runtime::EXIT_USAGE);
    }
    let address = match args.parse_usize(1) {
        Ok(address) => address,
        Err(_) => syscall::exit(runtime::EXIT_USAGE),
    };
    let tty_pid = if args.len() == 3 {
        match args.parse_usize(2) {
            Ok(pid) if pid != 0 => Some(pid),
            _ => syscall::exit(runtime::EXIT_USAGE),
        }
    } else {
        None
    };
    let uart = unsafe { Uart::new(address) };
    let mut service = Service::new(tty_pid);
    loop {
        let request = match ipc::receive() {
            Ok(request) => request,
            Err(Error::EINTR) => continue,
            Err(_) => syscall::exit(runtime::EXIT_FAILURE),
        };
        if let Err(error) = service.accept(request, &uart, ipc::read_payload, ipc::reply) {
            if ipc::reply(request.id, Err(error), None).is_err() {
                syscall::exit(runtime::EXIT_FAILURE);
            }
        } else if request.message.operation != POLL && service.progress(&uart, ipc::reply).is_err()
        {
            syscall::exit(runtime::EXIT_FAILURE);
        }
    }
}
