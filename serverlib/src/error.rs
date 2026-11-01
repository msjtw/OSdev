#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(usize);

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const ESRCH: Self = Self(3);
    pub const EINTR: Self = Self(4);
    pub const EIO: Self = Self(5);
    pub const EAGAIN: Self = Self(11);
    pub const ENOMEM: Self = Self(12);
    pub const EFAULT: Self = Self(14);
    pub const EINVAL: Self = Self(22);
    pub const EPIPE: Self = Self(32);
    pub const ENOSYS: Self = Self(38);
    pub const EOVERFLOW: Self = Self(75);

    pub const fn from_code(code: usize) -> Option<Self> {
        if code > 0 && code <= 4095 {
            Some(Self(code))
        } else {
            None
        }
    }

    pub const fn code(self) -> usize {
        self.0
    }

    pub const fn into_raw(self) -> usize {
        0usize.wrapping_sub(self.0)
    }
}

/// Decode the Linux negative-errno range, preserving other result words.
pub const fn decode(raw: usize) -> Result<usize> {
    if raw >= 0usize.wrapping_sub(4095) {
        Err(Error(0usize.wrapping_sub(raw)))
    } else {
        Ok(raw)
    }
}

pub const fn encode(result: Result<usize>) -> usize {
    match result {
        Ok(value) => value,
        Err(error) => error.into_raw(),
    }
}
