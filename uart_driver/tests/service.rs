use serverlib::{
    Error,
    abi::{Message, SYS_READ, SYS_WRITE},
};
use uart_driver::{handle, uart::Uart};

fn message(sender: usize, operation: usize, count: usize) -> Message {
    Message {
        sender,
        operation,
        args: [0, 0, count, 0, 0, 0],
    }
}

#[test]
fn empty_and_zero_length_reads_do_not_allocate() {
    let mut registers = [0u8; 8];
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = Vec::new();
    assert_eq!(
        handle(
            &message(0, SYS_READ, usize::MAX),
            &uart,
            &mut buffer,
            |_| panic!()
        ),
        Ok(0)
    );
    assert_eq!(buffer.capacity(), 0);
    registers[5] = 1;
    assert_eq!(
        handle(&message(0, SYS_READ, 0), &uart, &mut buffer, |_| panic!()),
        Ok(0)
    );
    assert_eq!(buffer.capacity(), 0);
}

#[test]
fn reads_obey_count_and_clear_previous_contents() {
    let mut registers = [0u8; 8];
    registers[0] = b'x';
    registers[5] = 1;
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = vec![b'!'; 12];
    assert_eq!(
        handle(&message(0, SYS_READ, 3), &uart, &mut buffer, |_| panic!()),
        Ok(3)
    );
    // Static register storage is not a FIFO emulator: each read sees the same byte.
    assert_eq!(buffer, b"xxx");
    registers[5] = 0;
    assert_eq!(
        handle(&message(0, SYS_READ, 3), &uart, &mut buffer, |_| panic!()),
        Ok(0)
    );
    assert!(buffer.is_empty());
}

#[test]
fn unauthorized_requests_are_rejected_before_payload_or_mmio() {
    let mut registers = [0u8; 8];
    registers[0] = b'!';
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = Vec::new();
    for operation in [SYS_READ, SYS_WRITE, 0x8000ffff] {
        assert_eq!(
            handle(
                &message(7, operation, usize::MAX),
                &uart,
                &mut buffer,
                |_| panic!()
            ),
            Err(Error::EPERM)
        );
    }
    assert_eq!(buffer.capacity(), 0);
    assert_eq!(registers[0], b'!');
}

#[test]
fn unknown_kernel_operations_are_not_device_operations() {
    let mut registers = [0u8; 8];
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    assert_eq!(
        handle(
            &message(0, 0x8000ffff, 1),
            &uart,
            &mut Vec::new(),
            |_| panic!()
        ),
        Err(Error::ENOSYS)
    );
}

#[test]
fn writes_copy_payload_before_transmitting_and_reuse_storage() {
    let mut registers = [0u8; 8];
    registers[5] = 1 << 5;
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = Vec::new();
    assert_eq!(
        handle(&message(0, SYS_WRITE, 3), &uart, &mut buffer, |bytes| {
            bytes.copy_from_slice(b"abc");
            Ok(3)
        }),
        Ok(3)
    );
    assert_eq!(registers[0], b'c');
    let capacity = buffer.capacity();
    assert_eq!(
        handle(&message(0, SYS_WRITE, 1), &uart, &mut buffer, |bytes| {
            bytes[0] = b'z';
            Ok(1)
        }),
        Ok(1)
    );
    assert_eq!(buffer, b"z");
    assert_eq!(buffer.capacity(), capacity);
    assert_eq!(registers[0], b'z');
}

#[test]
fn payload_errors_and_short_copies_do_not_transmit() {
    let mut registers = [0u8; 8];
    registers[0] = b'!';
    registers[5] = 1 << 5;
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = Vec::new();
    assert_eq!(
        handle(&message(0, SYS_WRITE, 8), &uart, &mut buffer, |_| Err(
            Error::EFAULT
        )),
        Err(Error::EFAULT)
    );
    assert_eq!(registers[0], b'!');
    assert_eq!(
        handle(&message(0, SYS_WRITE, 8), &uart, &mut buffer, |_| Ok(7)),
        Err(Error::EIO)
    );
    assert_eq!(registers[0], b'!');
}

#[test]
fn impossible_reservations_fail_before_copying() {
    let mut registers = [0u8; 8];
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = Vec::new();
    assert_eq!(
        handle(
            &message(0, SYS_WRITE, usize::MAX),
            &uart,
            &mut buffer,
            |_| panic!()
        ),
        Err(Error::ENOMEM)
    );
    assert_eq!(buffer.capacity(), 0);
}

#[test]
fn zero_length_writes_do_not_copy_or_wait_for_transmitter() {
    let mut registers = [0u8; 8];
    let uart = unsafe { Uart::new(registers.as_mut_ptr() as usize) };
    let mut buffer = vec![b'!'; 4];
    assert_eq!(
        handle(&message(0, SYS_WRITE, 0), &uart, &mut buffer, |_| panic!()),
        Ok(0)
    );
    assert!(buffer.is_empty());
    assert_eq!(registers[0], 0);
}
