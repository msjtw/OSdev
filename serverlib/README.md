
# serverlib

Shared service interfaces and generic userspace support.

Service modules define ABI constants, payload layouts, serialization and thin
transport/syscall wrappers. Service-specific policy belongs in the corresponding
server, not here. In particular, `tty_driver` chooses terminal defaults and owns
input buffering, echo, signal policy, readiness calculation and retry decisions;
`serverlib::tty::Termios` only describes and encodes the shared payload.

Generic startup, fixed-heap allocation and IPC support remain shared infrastructure.
