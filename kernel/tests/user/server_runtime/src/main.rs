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
        Some(b"async") => test_async(),
        Some(b"async-exec") => test_async_exec(),
        Some(b"async-cross") => test_async_cross(),
        Some(b"after-cross-exec") => {
            control(args.parse_usize(2).unwrap());
            syscall::exit(0)
        }
        Some(b"after-async-exec") => {
            assert_eq!(ipc::cancel(args.parse_usize(2).unwrap()), Err(Error::ESRCH));
            test_async()
        }
        Some(b"async-interrupt") => test_async_interrupt(),
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

fn self_request(id: usize) {
    match ipc::receive_event().unwrap() {
        ipc::Event::Request(request) => assert_eq!(request.id, id),
        _ => panic!("expected request"),
    }
}

fn test_async() -> ! {
    let pid = syscall::getpid().unwrap();
    let mut bytes = *b"abcdefgh";
    let message = Message { operation: ECHO, args: [0, bytes.as_mut_ptr() as usize, bytes.len(), 0, 0, 0], ..Message::default() };
    let cancelled = unsafe { ipc::submit(pid, &message) }.unwrap();
    ipc::cancel(cancelled).unwrap();
    let id = unsafe { ipc::submit(pid, &message) }.unwrap();
    self_request(id);
    let mut copy = [0; 8];
    assert_eq!(ipc::read_payload(id, 0, &mut copy), Ok(8));
    assert_eq!(&copy, b"abcdefgh");
    ipc::cancel(id).unwrap();
    assert_eq!(ipc::read_payload(id, 0, &mut copy), Err(Error::ESRCH));
    ipc::reply(id, Ok(4), Some(b"late")).unwrap();
    let fresh = unsafe { ipc::submit(pid, &message) }.unwrap();
    self_request(fresh);
    ipc::reply(id, Ok(4), Some(b"late")).unwrap();
    ipc::reply(fresh, Ok(4), Some(b"done")).unwrap();
    let raw = unsafe { syscall::syscall(serverlib::abi::SYS_IPC_RECEIVE_EVENT, [usize::MAX, 0, 0, 0, 0, 0]) };
    assert_eq!(raw, Error::EFAULT.into_raw());
    assert_eq!(ipc::receive_event(), Ok(ipc::Event::Completion { id: fresh, result: Ok(4) }));
    assert_eq!(&bytes, b"doneefgh"); // Neither stale reply wrote "late".
    assert_eq!(ipc::cancel(fresh), Err(Error::ESRCH));
    let id = unsafe { ipc::submit(pid, &message) }.unwrap();
    self_request(id);
    ipc::reply(id, Ok(0), None).unwrap();
    ipc::cancel(id).unwrap(); // Completed but not yet consumed.
    let missing = unsafe { ipc::submit(usize::MAX, &message) }.unwrap();
    ipc::cancel(missing).unwrap();
    let first = unsafe { ipc::submit(pid, &Message::default()) }.unwrap();
    let second = unsafe { ipc::submit(pid, &Message::default()) }.unwrap();
    self_request(first);
    self_request(second);
    ipc::cancel(first).unwrap();
    ipc::reply(first, Ok(0), None).unwrap();
    ipc::reply(second, Err(Error::EINTR), None).unwrap();
    assert_eq!(ipc::receive_event(), Ok(ipc::Event::Completion {
        id: second, result: Err(Error::EINTR),
    }));

    // Child exit must remove its request from our inbox before its memory dies.
    let child = unsafe { syscall::syscall(220, [17, 0, 0, 0, 0, 0]) };
    assert!((child as isize) >= 0);
    if child == 0 {
        unsafe { ipc::submit(pid, &message) }.unwrap();
        syscall::exit(0);
    }
    let mut info = [0usize; 32];
    assert_eq!(unsafe { syscall::syscall(95, [0, 0, info.as_mut_ptr() as usize, 4, 0, 0]) }, 0);
    let id = unsafe { ipc::submit(pid, &message) }.unwrap();
    self_request(id);
    ipc::cancel(id).unwrap();
    syscall::write(1, b"ASYNC_OK\n").unwrap();
    syscall::exit(0)
}

fn test_async_exec() -> ! {
    let pid = syscall::getpid().unwrap();
    let id = unsafe { ipc::submit(pid, &Message::default()) }.unwrap();
    let bad_argv = [c"/lorem.txt".as_ptr() as usize, 0];
    let raw = unsafe { syscall::syscall(221, [bad_argv[0], bad_argv.as_ptr() as usize, 0, 0, 0, 0]) };
    assert_eq!(raw, Error::from_code(8).unwrap().into_raw());
    ipc::cancel(id).unwrap(); // Failed exec must not cancel it.
    let id = unsafe { ipc::submit(pid, &Message::default()) }.unwrap();
    let text = alloc::ffi::CString::new(alloc::format!("{id}")).unwrap();
    let argv = [c"/runtime-test".as_ptr(), c"after-async-exec".as_ptr(), text.as_ptr(), core::ptr::null()];
    unsafe { syscall::syscall(221, [argv[0] as usize, argv.as_ptr() as usize, 0, 0, 0, 0]) };
    panic!("exec returned")
}

// External syscall regression only: parent and child synchronize through real
// blocking IPC, keeping buffers owned until completion/cancellation.
fn control(parent: usize) {
    assert_eq!(unsafe { ipc::call(parent, &Message::default()) }, Ok(0));
}

fn incoming() -> ipc::Request {
    match ipc::receive_event().unwrap() {
        ipc::Event::Request(request) => request,
        _ => panic!("expected incoming request"),
    }
}

fn pause() {
    let duration = [0i64, 100_000_000];
    assert_eq!(unsafe { syscall::syscall(407, [0, 0, duration.as_ptr() as usize, 0, 0, 0]) }, 0);
}

fn test_async_cross() -> ! {
    let parent = syscall::getpid().unwrap();
    let child = unsafe { syscall::syscall(220, [17, 0, 0, 0, 0, 0]) };
    assert!((child as isize) >= 0);
    if child == 0 {
        let mut bytes = *b"abcdefgh";
        let message = Message {
            operation: ECHO,
            args: [0, bytes.as_mut_ptr() as usize, bytes.len(), 0, 0, 0],
            ..Message::default()
        };
        let id = unsafe { ipc::submit(parent, &message) }.unwrap();
        assert_eq!(ipc::receive_event(), Ok(ipc::Event::Completion { id, result: Ok(4) }));
        assert_eq!(&bytes, b"doneefgh");

        let id = unsafe { ipc::submit(parent, &message) }.unwrap();
        control(parent); // Parent has delivered the submission.
        ipc::cancel(id).unwrap();
        control(parent); // Parent now attempts stale access before acknowledging.
        assert_eq!(&bytes, b"doneefgh");

        unsafe { ipc::submit(parent, &message) }.unwrap();
        control(parent);
        let text = alloc::ffi::CString::new(alloc::format!("{parent}")).unwrap();
        let argv = [c"/runtime-test".as_ptr(), c"after-cross-exec".as_ptr(), text.as_ptr(), core::ptr::null()];
        unsafe { syscall::syscall(221, [argv[0] as usize, argv.as_ptr() as usize, 0, 0, 0, 0]) };
        panic!("exec returned")
    }
    let request = incoming();
    assert_eq!(request.message.sender, child);
    let mut copy = [0; 8];
    assert_eq!(ipc::read_payload(request.id, 0, &mut copy), Ok(8));
    assert_eq!(&copy, b"abcdefgh");
    pause(); // Let the child block in event receive before completing.
    ipc::reply(request.id, Ok(4), Some(b"done")).unwrap();
    for _ in 0..2 { // Delivered cancellation, then delivered successful exec.
        let abandoned = incoming();
        let handshake = incoming();
        ipc::reply(handshake.id, Ok(0), None).unwrap();
        let finished = incoming();
        assert_eq!(ipc::read_payload(abandoned.id, 0, &mut copy), Err(Error::ESRCH));
        ipc::reply(abandoned.id, Ok(4), Some(b"late")).unwrap();
        ipc::reply(finished.id, Ok(0), None).unwrap();
    }
    let mut info = [0usize; 32];
    assert_eq!(unsafe { syscall::syscall(95, [0, 0, info.as_mut_ptr() as usize, 4, 0, 0]) }, 0);

    let child = unsafe { syscall::syscall(220, [17, 0, 0, 0, 0, 0]) };
    assert!((child as isize) >= 0);
    if child == 0 {
        let bytes = *b"exitdata";
        let message = Message { operation: ECHO, args: [0, bytes.as_ptr() as usize, 8, 0, 0, 0], ..Message::default() };
        unsafe { ipc::submit(parent, &message) }.unwrap();
        control(parent);
        syscall::exit(0)
    }
    let abandoned = incoming();
    let handshake = incoming();
    ipc::reply(handshake.id, Ok(0), None).unwrap();
    assert_eq!(unsafe { syscall::syscall(95, [0, 0, info.as_mut_ptr() as usize, 4, 0, 0]) }, 0);
    assert_eq!(ipc::read_payload(abandoned.id, 0, &mut copy), Err(Error::ESRCH));
    ipc::reply(abandoned.id, Ok(4), Some(b"late")).unwrap();
    syscall::write(1, b"ASYNC_CROSS_OK\n").unwrap();
    syscall::exit(0)
}

extern "C" fn interrupted(_: usize) {}

#[unsafe(naked)]
unsafe extern "C" fn restore_signal() -> ! {
    core::arch::naked_asm!("li a7, 139", "ecall", "unimp");
}

fn test_async_interrupt() -> ! {
    let action = [interrupted as *const () as usize, 0, restore_signal as *const () as usize, 0, 0];
    assert_eq!(unsafe { syscall::syscall(134, [2, action.as_ptr() as usize, 0, 8, 0, 0]) }, 0);
    let mask = [0usize; 2];
    assert_eq!(unsafe { syscall::syscall(135, [2, mask.as_ptr() as usize, 0, 8, 0, 0]) }, 0);
    let id = unsafe { ipc::submit(usize::MAX, &Message::default()) }.unwrap();
    syscall::write(1, b"ASYNC_WAIT\n").unwrap();
    assert_eq!(ipc::receive_event(), Err(Error::EINTR));
    ipc::cancel(id).unwrap(); // Interrupted receive did not release the operation.
    syscall::write(1, b"ASYNC_INTR_OK\n").unwrap();
    syscall::exit(0)
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
