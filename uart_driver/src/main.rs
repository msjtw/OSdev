#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use serverlib::{Args, Error, abi::SYS_READ, ipc, runtime, syscall};
use uart_driver::{handle, uart::Uart};

serverlib::entry!(run);

fn run(args: Args) -> ! {
    if args.len() != 2 {
        syscall::exit(runtime::EXIT_USAGE);
    }
    let address = match args.parse_usize(1) {
        Ok(address) => address,
        Err(_) => syscall::exit(runtime::EXIT_USAGE),
    };
    let uart = unsafe { Uart::new(address) };
    let mut buffer = Vec::new();
    loop {
        let request = match ipc::receive() {
            Ok(request) => request,
            Err(Error::EINTR) => continue,
            Err(_) => syscall::exit(runtime::EXIT_FAILURE),
        };
        let result = handle(&request.message, &uart, &mut buffer, |destination| {
            ipc::read_payload(request.id, 0, destination)
        });
        let data = if request.message.operation == SYS_READ && result.is_ok() {
            Some(buffer.as_slice())
        } else {
            None
        };
        if let Err(error) = ipc::reply(request.id, result, data) {
            let _ = ipc::reply(request.id, Err(error), None);
        }
    }
}
