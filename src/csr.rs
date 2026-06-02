#[macro_export]
macro_rules! read_csr {
    ($csr:ident) => {{
        let value: usize;
            core::arch::asm!(
                concat!("csrrc {0}, ", stringify!($csr), ", x0"),
                out(reg) value,
            );
        value
    }};
}

#[macro_export]
macro_rules! write_csr {
    ($csr:ident, $val:expr) => {{
            core::arch::asm!(
                concat!("csrrw x0, ", stringify!($csr), ", {0}"),
                in(reg) $val,
            );
    }};
}

pub const SSTATUS_SPP: u32 = 1 << 8; // Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPIE: u32 = 1 << 5; // Supervisor Previous Interrupt Enable
pub const SSTATUS_UPIE: u32 = 1 << 4; // User Previous Interrupt Enable
pub const SSTATUS_SIE: u32 = 1 << 1; // Supervisor Interrupt Enable
pub const SSTATUS_UIE: u32 = 1 << 0; // User Interrupt Enable

#[derive(Debug)]
#[allow(non_camel_case_types, dead_code)]
pub enum Csr {
    mvendorid,
    marchid,
    mimpid,

    menvcfg,
    menvcfgh,

    mscratch,
    sscratch,

    cycle,
    cycleh,
    time,
    timeh,
    instret,
    instreth,

    mcycle,
    mcycleh,
    minstret,
    minstreth,

    mcountinhibit,
    scountinhibit,
    mcounteren,
    scounteren,

    mip,
    sip,
    mie,
    sie,
    mtvec,
    stvec,

    mepc,
    sepc,
    mcause,
    scause,
    mtval,
    stval,

    medeleg,
    medelegh,
    mideleg,

    mstatus,
    mstatush,
    sstatus,

    satp,

    misa,
    mhartid,

    pmpcfg0,
    pmpcfg1,
    pmpcfg2,
    pmpcfg3,

    pmpaddr0,
    pmpaddr1,
    pmpaddr2,
    pmpaddr3,
    pmpaddr4,
    pmpaddr5,
    pmpaddr6,
    pmpaddr7,
    pmpaddr8,
    pmpaddr9,
    pmpaddr10,
    pmpaddr11,
    pmpaddr12,
    pmpaddr13,
    pmpaddr14,
    pmpaddr15,
}

// pub fn csr_name(csr: &Csr) -> String {
//     match csr {
//         Csr::mvendorid => "mvendorid".to_string(),
//         Csr::marchid => "marchid".to_string(),
//         Csr::mimpid => "mimpid".to_string(),
//         Csr::mhartid => "mhartid".to_string(),
//         Csr::menvcfg => "menvcfg".to_string(),
//         Csr::menvcfgh => "menvcfgh".to_string(),
//         Csr::mscratch => "mscratch".to_string(),
//         Csr::sscratch => "sscratch".to_string(),
//         Csr::cycle => "cycle".to_string(),
//         Csr::cycleh => "cycleh".to_string(),
//         Csr::time => "time".to_string(),
//         Csr::timeh => "timeh".to_string(),
//         Csr::instret => "instret".to_string(),
//         Csr::instreth => "instreth".to_string(),
//         Csr::mcycle => "mcycle".to_string(),
//         Csr::mcycleh => "mcycleh".to_string(),
//         Csr::minstret => "minstret".to_string(),
//         Csr::minstreth => "minstreth".to_string(),
//         Csr::mcountinhibit => "mcountinhibit".to_string(),
//         Csr::scountinhibit => "scountinhibit".to_string(),
//         Csr::mcounteren => "mcounteren".to_string(),
//         Csr::scounteren => "scounteren".to_string(),
//         Csr::mip => "mip".to_string(),
//         Csr::sip => "sip".to_string(),
//         Csr::mie => "mie".to_string(),
//         Csr::sie => "sie".to_string(),
//         Csr::mtvec => "mtvec".to_string(),
//         Csr::stvec => "stvec".to_string(),
//         Csr::mepc => "mepc".to_string(),
//         Csr::sepc => "sepc".to_string(),
//         Csr::mcause => "mcause".to_string(),
//         Csr::scause => "scause".to_string(),
//         Csr::mtval => "mtval".to_string(),
//         Csr::stval => "stval".to_string(),
//         Csr::medeleg => "medeleg".to_string(),
//         Csr::medelegh => "medelegh".to_string(),
//         Csr::mideleg => "mideleg".to_string(),
//         Csr::mstatus => "mstatus".to_string(),
//         Csr::mstatush => "mstatush".to_string(),
//         Csr::sstatus => "sstatus".to_string(),
//         Csr::satp => "satp".to_string(),
//         Csr::misa => "misa".to_string(),
//         Csr::pmpcfg0 => "pmpcfg0".to_string(),
//         Csr::pmpcfg1 => "pmpcfg1".to_string(),
//         Csr::pmpcfg2 => "pmpcfg2".to_string(),
//         Csr::pmpcfg3 => "pmpcfg3".to_string(),
//         Csr::pmpaddr0 => "pmpaddr0".to_string(),
//         Csr::pmpaddr1 => "pmpaddr1".to_string(),
//         Csr::pmpaddr2 => "pmpaddr2".to_string(),
//         Csr::pmpaddr3 => "pmpaddr3".to_string(),
//         Csr::pmpaddr4 => "pmpaddr4".to_string(),
//         Csr::pmpaddr5 => "pmpaddr5".to_string(),
//         Csr::pmpaddr6 => "pmpaddr6".to_string(),
//         Csr::pmpaddr7 => "pmpaddr7".to_string(),
//         Csr::pmpaddr8 => "pmpaddr8".to_string(),
//         Csr::pmpaddr9 => "pmpaddr9".to_string(),
//         Csr::pmpaddr10 => "pmpaddr10".to_string(),
//         Csr::pmpaddr11 => "pmpaddr11".to_string(),
//         Csr::pmpaddr12 => "pmpaddr12".to_string(),
//         Csr::pmpaddr13 => "pmpaddr13".to_string(),
//         Csr::pmpaddr14 => "pmpaddr14".to_string(),
//         Csr::pmpaddr15 => "pmpaddr15".to_string(),
//     }
// }
