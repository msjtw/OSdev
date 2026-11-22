use serverlib::{
    Error,
    abi::*,
    error::{decode, encode},
};

#[test]
fn tty_mechanism_numbers_are_distinct_from_transport() {
    assert_eq!(SYS_TTY_CALLER_SESSION, CUSTOM_BIT | 0x100);
    assert_eq!(SYS_TTY_VALIDATE_FOREGROUND, CUSTOM_BIT | 0x101);
    assert_eq!(SYS_TTY_SIGNAL_GROUP, CUSTOM_BIT | 0x102);
    assert_eq!(SYS_TTY_PUBLISH_READINESS, CUSTOM_BIT | 0x103);
    let numbers = [
        SYS_IPC_CALL, SYS_IPC_RECEIVE, SYS_IPC_READ_PAYLOAD, SYS_IPC_REPLY,
        SYS_IPC_SUBMIT, SYS_IPC_RECEIVE_EVENT, SYS_IPC_CANCEL,
        SYS_TTY_CALLER_SESSION, SYS_TTY_VALIDATE_FOREGROUND, SYS_TTY_SIGNAL_GROUP,
        SYS_TTY_PUBLISH_READINESS,
    ];
    for (index, number) in numbers.iter().enumerate() {
        assert_ne!(number & CUSTOM_BIT, 0);
        assert!(!numbers[..index].contains(number));
    }
}

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
    assert_eq!(SYS_IPC_SUBMIT, CUSTOM_BIT | 5);
    assert_eq!(SYS_IPC_RECEIVE_EVENT, CUSTOM_BIT | 6);
    assert_eq!(SYS_IPC_CANCEL, CUSTOM_BIT | 7);
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
fn event_layout_preserves_the_existing_message_abi() {
    let word = core::mem::size_of::<usize>();
    assert_eq!(core::mem::size_of::<IpcEvent>(), 11 * word);
    assert_eq!(core::mem::offset_of!(IpcEvent, message), 3 * word);
    assert_eq!(SYS_IPC_SUBMIT, CUSTOM_BIT | 5);
    assert_eq!(SYS_IPC_RECEIVE_EVENT, CUSTOM_BIT | 6);
    assert_ne!(IPC_EVENT_REQUEST, IPC_EVENT_COMPLETION);
    let event = IpcEvent {
        kind: IPC_EVENT_COMPLETION,
        id: 42,
        result: Error::EINTR.into_raw(),
        message: Message::default(),
    };
    assert_eq!(decode(event.result), Err(Error::EINTR));
}

#[test]
fn successful_results_are_preserved() {
    for value in [0, 1, 8197, isize::MAX as usize, 0usize.wrapping_sub(4096)] {
        assert_eq!(decode(value), Ok(value));
        assert_eq!(encode(Ok(value)), value);
    }
}
