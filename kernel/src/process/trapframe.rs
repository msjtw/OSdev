#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Trapframe {
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub trap_handler: usize,
    pub epc: usize,
    pub hartid: usize,
    ra: usize,
    pub sp: usize,
    gp: usize,
    tp: usize,
    t0: usize,
    t1: usize,
    t2: usize,
    s0: usize,
    s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    s2: usize,
    s3: usize,
    s4: usize,
    s5: usize,
    s6: usize,
    s7: usize,
    s8: usize,
    s9: usize,
    s10: usize,
    s11: usize,
    t3: usize,
    t4: usize,
    t5: usize,
    t6: usize,
}

// impl Trapframe {
//     pub fn new() -> Self {
//         
//     }
// }
