use serverlib::{
    Error,
    abi::*,
    error::{decode, encode},
};

#[test]
fn message_layout_and_custom_syscall_numbers() {
    let word = core::mem::size_of::<usize>();
    assert_eq!(core::mem::size_of::<Message>(), 8 * word);
    assert_eq!(core::mem::offset_of!(Message, operation), word);
    assert_eq!(core::mem::offset_of!(Message, args), 2 * word);
    assert_eq!(SYS_IPC_CALL, CUSTOM_BIT | 1);
    assert_eq!(SYS_IPC_RECEIVE, CUSTOM_BIT | 2);
    assert_eq!(SYS_IPC_READ_PAYLOAD, CUSTOM_BIT | 3);
    assert_eq!(SYS_IPC_REPLY, CUSTOM_BIT | 4);
}

#[test]
fn errors_roundtrip_without_losing_unknown_codes() {
    for code in 1..=4095 {
        let error = Error::from_code(code).unwrap();
        assert_eq!(decode(error.into_raw()), Err(error));
        assert_eq!(encode(Err(error)), error.into_raw());
        assert_eq!(error.code(), code);
    }
    assert!(Error::from_code(0).is_none());
    assert!(Error::from_code(4096).is_none());
    assert_eq!(Error::EINTR.into_raw(), -4isize as usize);
}

#[test]
fn successful_results_are_preserved() {
    for value in [0, 1, 8197, isize::MAX as usize, 0usize.wrapping_sub(4096)] {
        assert_eq!(decode(value), Ok(value));
        assert_eq!(encode(Ok(value)), value);
    }
}
