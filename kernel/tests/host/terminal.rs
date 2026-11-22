//! Test real mechanism policy with host process metadata and an IPC lookup boundary.
#[path = "../../src/errno.rs"]
mod errno;
mod process {
    pub mod poll {
        pub type PollEvents = i16;
        pub const POLLIN: PollEvents = 1;
        pub const POLLOUT: PollEvents = 4;
        pub fn poll_channel() -> usize {
            42
        }
    }
    #[derive(Default, PartialEq, Eq)]
    pub enum ProcState {
        Used,
        Sleeping,
        Runnable,
        Running,
        #[default]
        Unused,
        Zombie,
    }
    #[derive(Default)]
    pub struct Process {
        pub pid: Option<usize>,
        pub sid: Option<usize>,
        pub pgid: Option<usize>,
        pub state: ProcState,
        pub pending_signals: u64,
        pub lock: std::sync::Mutex<()>,
        pub sleep_channel: Option<usize>,
    }
    pub mod signals {
        pub const SIGINT: usize = 2;
        pub fn signal_bit(signal: usize) -> Option<u64> {
            Some(1 << (signal - 1))
        }
    }
    pub mod ipc {
        pub fn caller_session(
            _: &mut crate::kernel::Kernel,
            _: &super::Process,
            id: usize,
        ) -> Result<Option<usize>, crate::errno::Errno> {
            match id {
                10 => Ok(Some(3)),
                11 => Ok(None),
                0 => Err(crate::errno::Errno::EINVAL),
                _ => Err(crate::errno::Errno::ESRCH),
            }
        }
    }
}
mod kernel {
    #[derive(Default)]
    pub struct Kernel {
        pub tty_pid: Option<usize>,
        pub process_table: Vec<crate::process::Process>,
        pub wakes: usize,
    }
    impl Kernel {
        pub fn wakeup(&mut self, channel: Option<usize>, _: bool) {
            self.wakes += 1;
            for proc in &mut self.process_table {
                if proc.state == crate::process::ProcState::Sleeping
                    && proc.sleep_channel == channel
                {
                    proc.state = crate::process::ProcState::Runnable;
                }
            }
        }
    }
}
#[path = "../../src/process/terminal.rs"]
mod terminal;
use process::{ProcState, Process};

#[test]
fn readiness_authorization_masking_and_changed_level_wakeups() {
    let tty = Process {
        pid: Some(7),
        ..Default::default()
    };
    let mut kernel = kernel::Kernel::default();
    assert!(matches!(
        terminal::publish_readiness(&mut kernel, &tty, 1),
        Err(errno::Errno::EPERM)
    ));
    kernel.tty_pid = Some(7);
    assert!(matches!(
        terminal::publish_readiness(&mut kernel, &tty, 2),
        Err(errno::Errno::EINVAL)
    ));
    assert_eq!(terminal::readiness(0x7fff), 4);
    kernel.process_table.push(Process {
        state: ProcState::Sleeping,
        sleep_channel: Some(42),
        ..Default::default()
    });
    kernel.process_table.push(Process {
        state: ProcState::Sleeping,
        sleep_channel: Some(99),
        ..Default::default()
    });
    terminal::publish_readiness(&mut kernel, &tty, 0).unwrap();
    assert_eq!(kernel.wakes, 0);
    terminal::publish_readiness(&mut kernel, &tty, 1).unwrap();
    assert_eq!(kernel.wakes, 1);
    assert!(kernel.process_table[0].state == ProcState::Runnable);
    assert!(kernel.process_table[1].state == ProcState::Sleeping);
    assert_eq!(terminal::readiness(1), 1);
    assert_eq!(terminal::readiness(4), 4);
    assert_eq!(terminal::readiness(0), 0);
    assert_eq!(terminal::readiness(0x7fff), 5);
    terminal::publish_readiness(&mut kernel, &tty, 1).unwrap();
    assert_eq!(kernel.wakes, 1);
    terminal::publish_readiness(&mut kernel, &tty, 0).unwrap();
    assert_eq!(kernel.wakes, 2);
    assert_eq!(terminal::readiness(1), 0);
}

#[test]
fn authorization_lookup_errors_and_foreground_membership() {
    let tty = Process {
        pid: Some(7),
        ..Default::default()
    };
    let mut kernel = kernel::Kernel::default();
    assert!(matches!(
        terminal::caller_session(&mut kernel, &tty, 10),
        Err(errno::Errno::EPERM)
    ));
    kernel.tty_pid = Some(7);
    assert_eq!(terminal::caller_session(&mut kernel, &tty, 10).unwrap(), 3);
    assert_eq!(terminal::caller_session(&mut kernel, &tty, 11).unwrap(), 0);
    assert!(matches!(
        terminal::caller_session(&mut kernel, &tty, 0),
        Err(errno::Errno::EINVAL)
    ));
    assert!(matches!(
        terminal::caller_session(&mut kernel, &tty, 99),
        Err(errno::Errno::ESRCH)
    ));
    kernel.process_table.push(Process {
        sid: Some(3),
        pgid: Some(4),
        ..Default::default()
    });
    assert_eq!(
        terminal::validate_foreground(&mut kernel, &tty, 10, 3, 4).unwrap(),
        0
    );
    for (id, sid, pgid) in [(11, 3, 4), (10, 8, 4), (10, 3, 0), (10, 3, 99)] {
        assert!(matches!(
            terminal::validate_foreground(&mut kernel, &tty, id, sid, pgid),
            Err(errno::Errno::EPERM)
        ));
    }
}

#[test]
fn signals_select_live_session_members_and_wake_sleepers() {
    let tty = Process {
        pid: Some(7),
        ..Default::default()
    };
    let mut kernel = kernel::Kernel {
        tty_pid: Some(7),
        ..Default::default()
    };
    for (sid, pgid, state) in [
        (3, 4, ProcState::Sleeping),
        (3, 4, ProcState::Running),
        (3, 4, ProcState::Zombie),
        (9, 4, ProcState::Sleeping),
        (3, 8, ProcState::Used),
    ] {
        kernel.process_table.push(Process {
            sid: Some(sid),
            pgid: Some(pgid),
            state,
            ..Default::default()
        });
    }
    assert_eq!(terminal::signal_group(&mut kernel, &tty, 3, 4).unwrap(), 0);
    assert!(kernel.process_table[0].state == ProcState::Runnable);
    for (index, process) in kernel.process_table.iter().enumerate() {
        assert_eq!(process.pending_signals, if index < 2 { 2 } else { 0 });
    }
    assert_eq!(terminal::signal_group(&mut kernel, &tty, 3, 99).unwrap(), 0);
    assert!(matches!(
        terminal::signal_group(&mut kernel, &tty, 0, 4),
        Err(errno::Errno::EPERM)
    ));
    kernel.tty_pid = None;
    assert!(matches!(
        terminal::signal_group(&mut kernel, &tty, 3, 4),
        Err(errno::Errno::EPERM)
    ));
}
