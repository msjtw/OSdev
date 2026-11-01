#![cfg_attr(target_arch = "riscv32", no_std)]
#![cfg_attr(target_arch = "riscv32", no_main)]

// Standalone RV32 client/server exercise, loaded by the host QEMU test.
#[cfg(not(target_arch = "riscv32"))]
fn main() {}

#[cfg(target_arch = "riscv32")]
mod program {
    use serverlib::{
        Error, Result,
        abi::{CUSTOM_BIT, Message, SYS_READ, SYS_WRITE},
        ipc, syscall,
    };

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        syscall::exit(1)
    }

    fn require<T>(result: Result<T>) -> T {
        match result {
            Ok(value) => value,
            Err(_) => syscall::exit(1),
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn _start() -> ! {
        let parent = require(syscall::getpid());
        let child = require(serverlib::error::decode(unsafe {
            syscall::syscall(220, [17, 0, 0, 0, 0, 0]) // clone(SIGCHLD)
        }));
        if child == 0 {
            let source = [1u8, 2, 3, 4];
            let message = Message {
                sender: 999,
                operation: SYS_WRITE,
                args: [1, source.as_ptr() as usize, source.len(), 0, 0, 0],
            };
            assert_eq!(require(unsafe { ipc::call(parent, &message) }), 4);
            let mut destination = [0u8; 4];
            let message = Message {
                operation: SYS_READ,
                args: [
                    0,
                    destination.as_mut_ptr() as usize,
                    destination.len(),
                    0,
                    0,
                    0,
                ],
                ..Message::default()
            };
            assert_eq!(require(unsafe { ipc::call(parent, &message) }), 4);
            assert_eq!(destination, [5, 6, 7, 8]);
            let message = Message {
                operation: CUSTOM_BIT | 100,
                ..Message::default()
            };
            assert_eq!(unsafe { ipc::call(parent, &message) }, Err(Error::EINVAL));
            syscall::exit(0);
        }

        let request = require(ipc::receive());
        assert_eq!(request.message.sender, child);
        assert_eq!(request.message.operation, SYS_WRITE);
        let mut data = [0u8; 4];
        assert_eq!(require(ipc::read_payload(request.id, 0, &mut data)), 4);
        assert_eq!(data, [1, 2, 3, 4]);
        require(ipc::reply(request.id, Ok(4), None));

        let request = require(ipc::receive());
        assert_eq!(request.message.operation, SYS_READ);
        require(ipc::reply(request.id, Ok(4), Some(&[5, 6, 7, 8])));
        let request = require(ipc::receive());
        require(ipc::reply(request.id, Err(Error::EINVAL), None));

        let mut info = [0usize; 32];
        loop {
            match serverlib::error::decode(unsafe {
                syscall::syscall(95, [0, 0, info.as_mut_ptr() as usize, 4, 0, 0]) // waitid
            }) {
                Err(Error::EINTR) => continue,
                result => {
                    require(result);
                    break;
                }
            }
        }
        assert_eq!(info[2], 1); // CLD_EXITED
        assert_eq!(info[5], 0);
        require(syscall::write(1, b"SERVERLIB_IPC_OK\n"));
        syscall::exit(0)
    }
}
