#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};
use serverlib::{
    Args, Error,
    abi::{CUSTOM_BIT, Message},
    ipc,
    runtime::EXIT_USAGE,
    syscall,
};

const ECHO: usize = CUSTOM_BIT | 0x100;
const ROUNDS: usize = 6;

#[cfg(not(feature = "custom-panic"))]
serverlib::entry!(run);
#[cfg(feature = "custom-panic")]
serverlib::entry!(run, panic = custom_panic);

#[cfg(feature = "custom-panic")]
fn custom_panic(_: &core::panic::PanicInfo<'_>) -> ! {
    syscall::exit(7)
}

fn run(args: Args) -> ! {
    unsafe extern "C" {
        #[link_name = "__global_pointer$"]
        static GP: u8;
    }
    let gp: usize;
    let sp: usize;
    unsafe { core::arch::asm!("mv {}, gp", out(reg) gp, options(nomem, nostack)) };
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
    assert_eq!(gp, core::ptr::addr_of!(GP) as usize);
    assert_eq!(sp % serverlib::runtime::STACK_ALIGNMENT, 0);
    assert!(syscall::getpid().unwrap() > 1);
    assert!(!args.is_empty());
    assert!(args.get(args.len()).is_none());
    match args.get(1).map(|s| s.to_bytes()) {
        Some(b"heap") => test_heap(),
        Some(b"ipc-server") => ipc_server(),
        Some(b"ipc-client") => ipc_client(args.parse_usize(2).unwrap()),
        Some(b"oom") => {
            // Infallible allocation must exit 1 even with a custom panic handler.
            let buffer = core::hint::black_box(Vec::<u8>::with_capacity(heap_size() + 1));
            core::hint::black_box(buffer);
            syscall::exit(99)
        }
        _ => {}
    }
    if args.get(1).map(|s| s.to_bytes()) == Some(b"panic") {
        panic!("test panic dispatch");
    }
    if args.len() != 2 {
        syscall::exit(EXIT_USAGE);
    }
    match args.parse_usize(1) {
        Ok(0x10000000) => {
            syscall::write(1, b"SERVER_RUNTIME_OK\n").unwrap();
            syscall::exit(0)
        }
        Err(Error::EOVERFLOW) => syscall::exit(EXIT_USAGE),
        _ => syscall::exit(EXIT_USAGE),
    }
}

fn test_heap() -> ! {
    unsafe extern "C" {
        static __server_heap_start: u8;
        static __server_heap_end: u8;
    }
    let start = core::ptr::addr_of!(__server_heap_start) as usize;
    let end = core::ptr::addr_of!(__server_heap_end) as usize;
    {
        let number = Box::new(123usize);
        let text = String::from("fixed heap");
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(128).unwrap();
        bytes.resize(128, 0x5au8);
        assert_eq!(*number, 123);
        assert_eq!(text, "fixed heap");
        for (address, size) in [
            (
                number.as_ref() as *const usize as usize,
                core::mem::size_of::<usize>(),
            ),
            (text.as_ptr() as usize, text.len()),
            (bytes.as_ptr() as usize, bytes.capacity()),
        ] {
            assert!(address >= start && address + size <= end);
        }
    }
    #[repr(align(256))]
    struct Aligned([u8; 256]);
    let aligned = Box::new(Aligned([42; 256]));
    assert_eq!(aligned.as_ref() as *const Aligned as usize % 256, 0);
    assert_eq!(aligned.0[0], 42);
    drop(aligned);

    // Exhaust either test heap without allocating bookkeeping.
    let mut buffers: [Vec<u8>; 256] = core::array::from_fn(|_| Vec::new());
    let mut allocated = 0;
    for buffer in &mut buffers {
        if buffer.try_reserve_exact(128).is_err() {
            break;
        }
        buffer.resize(128, 0xabu8);
        allocated += 1;
    }
    assert!(allocated > 0 && allocated < buffers.len());
    assert_eq!(allocated * 128, heap_size());
    for buffer in &buffers[..allocated] {
        assert!(buffer.iter().all(|&byte| byte == 0xab));
    }
    let mut extra = Vec::<u8>::new();
    assert!(extra.try_reserve_exact(128).is_err());
    drop(buffers);
    for _ in 0..3 {
        extra.try_reserve_exact(2048).unwrap();
        extra.resize(2048, 0x55);
        assert_eq!(extra[2047], 0x55);
        extra = Vec::new();
    }
    assert!(extra.try_reserve_exact(end - start + 1).is_err());
    syscall::write(1, b"SERVER_HEAP_OK\n").unwrap();
    syscall::exit(0)
}

fn heap_size() -> usize {
    unsafe extern "C" {
        static __server_heap_start: u8;
        static __server_heap_end: u8;
    }
    core::ptr::addr_of!(__server_heap_end) as usize
        - core::ptr::addr_of!(__server_heap_start) as usize
}

fn ipc_length(round: usize) -> usize {
    [0, 17, 1024, heap_size() * 2, 1024, 17][round]
}

fn pattern(index: usize, round: usize) -> u8 {
    (index as u8).wrapping_add(round as u8)
}

fn ipc_server() -> ! {
    let mut previous_id = None;
    for round in 0..ROUNDS {
        let request = ipc::receive().unwrap();
        assert_ne!(Some(request.id), previous_id);
        previous_id = Some(request.id);
        let message = request.message;
        assert_eq!(message.operation, ECHO);
        assert!(message.sender > 1);
        assert_eq!(message.sender, message.args[0]);
        assert_eq!(message.args[2], ipc_length(round));
        assert_eq!(message.args[3..], [round, 123, 456]);
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(message.args[2]).is_err() {
            assert_eq!(round, 3);
            ipc::reply(request.id, Err(Error::ENOMEM), None).unwrap();
            continue;
        }
        bytes.resize(message.args[2], 0u8);
        let split = bytes.len() / 2;
        assert_eq!(
            ipc::read_payload(request.id, 0, &mut bytes[..split]),
            Ok(split)
        );
        assert_eq!(
            ipc::read_payload(request.id, split, &mut bytes[split..]),
            Ok(bytes.len() - split)
        );
        for (index, byte) in bytes.iter_mut().enumerate() {
            assert_eq!(*byte, pattern(index, round));
            *byte ^= 0x5a;
        }
        ipc::reply(request.id, Ok(bytes.len()), Some(&bytes)).unwrap();
        assert_eq!(ipc::read_payload(request.id, 0, &mut []), Err(Error::ESRCH));
        // Vec drops each iteration; the next request must be able to reuse it.
    }
    syscall::write(1, b"SERVER_RUNTIME_SERVER_OK\n").unwrap();
    syscall::exit(0)
}

fn ipc_client(destination: usize) -> ! {
    assert_eq!(syscall::read(0, &mut []), Ok(0));
    for round in 0..ROUNDS {
        let length = ipc_length(round);
        let mut bytes = Vec::new();
        if round != 3 {
            bytes.try_reserve_exact(length).unwrap();
            bytes.extend((0..length).map(|index| pattern(index, round)));
        }
        let message = Message {
            sender: 0, // Kernel must replace this claimed sender.
            operation: ECHO,
            args: [
                syscall::getpid().unwrap(),
                bytes.as_mut_ptr() as usize,
                length,
                round,
                123,
                456,
            ],
        };
        // The live buffer stays allocated and untouched until call returns.
        // Round 3 must fail reservation before the server reads any payload.
        let result = unsafe { ipc::call(destination, &message) };
        if round == 3 {
            assert_eq!(result, Err(Error::ENOMEM));
        } else {
            assert_eq!(result, Ok(length));
            for (index, &byte) in bytes.iter().enumerate() {
                assert_eq!(byte, pattern(index, round) ^ 0x5a);
            }
        }
    }
    syscall::write(1, b"SERVERLIB_IPC_OK\n").unwrap();
    syscall::exit(0)
}
