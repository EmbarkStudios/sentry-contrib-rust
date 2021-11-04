use super::ptrace_dumper::Error;
use std::{mem, ptr};

cfg_if::cfg_if! {
    if #[cfg(any(target_arch = "x86", target_arch = "x86_64"))] {
        type GPRegs = libc::user_regs_struct;
        type FPRegs = libc::user_fpregs_struct;
    } else if #[cfg(target_arch = "aarch")] {
        type GPRegs = libc::user_regs;
        type FPRegs = libc::user_fpregs;
    } else if #[cfg(target_arch = "aarch64")] {
        type GPRegs = libc::user_regs_struct;
        type FPRegs = libc::user_fpsimd_struct;
    }
}

// These aren't exposed by libc as a specific type, so...yah
cfg_if::cfg_if! {
    // https://github.com/rust-lang/libc/blob/master/src/unix/linux_like/linux/gnu/b64/x86_64/mod.rs#L226
    if #[cfg(all(target_arch = "x86_64", target_env = "gnu"))] {
        type DebugReg = u64;
    // https://github.com/rust-lang/libc/blob/b1c89cc918728998f70f12dc559d210b409bfc63/src/unix/linux_like/linux/musl/b64/x86_64/mod.rs#L100
    } else if #[cfg(all(target_arch = "x86_64", target_env = "musl"))] {
        type DebugReg = u32;
    // https://github.com/rust-lang/libc/blob/b1c89cc918728998f70f12dc559d210b409bfc63/src/unix/linux_like/linux/gnu/b32/x86/mod.rs#L108
    } else if #[cfg(all(target_arch = "x86", target_env = "gnu"))] {
        type DebugReg = u32;
    } else if #[cfg(all(target_arch = "x86", target_env = "musl"))] {
        compile_error!("unsupported target");
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        pub type RawContextCpu = minidump_common::format::CONTEXT_AMD64;
    } else if #[cfg(target_arch = "x86")] {
        pub type RawContextCpu = minidump_common::format::CONTEXT_X86;
    } else if #[cfg(target_arch = "aarch")] {
        pub type RawContextCpu = minidump_common::format::CONTEXT_ARM;
    } else if #[cfg(target_arch = "aarch64")] {
        pub type RawContextCpu = minidump_common::format::CONTEXT_ARM64_OLD;
    } else {
        compile_error!("unsupported target architecture");
    }
}

pub(crate) struct ThreadInfo {
    gp_regs: GPRegs,
    fp_regs: FPRegs,
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    debug_regs: [DebugReg; 8],
    #[cfg(target_arch = "x86")]
    fpx_regs: libc::user_fpxregs_struct,

    pub stack_pointer: usize,
    /// Thread group id
    pub tgid: u32,
    /// Parent process id
    pub parent: u32,
}

impl ThreadInfo {
    pub fn new(tid: u32, tgid: u32, parent: u32) -> Result<Self, Error> {
        unsafe {
            let mut gp_regs: GPRegs = mem::zeroed();

            let mut io = libc::iovec {
                iov_base: (&mut gp_regs as *mut GPRegs).cast(),
                iov_len: mem::size_of::<GPRegs>(),
            };

            if libc::ptrace(
                libc::PTRACE_GETREGSET,
                tid,
                1u32 as *mut libc::c_void,
                &mut io,
            ) == -1
            {
                return Err(Error::PtraceFailed);
            }

            let mut fp_regs: FPRegs = mem::zeroed();

            let mut io = libc::iovec {
                iov_base: (&mut fp_regs as *mut FPRegs).cast(),
                iov_len: mem::size_of::<FPRegs>(),
            };

            if libc::ptrace(
                libc::PTRACE_GETREGSET,
                tid,
                2u32 as *mut libc::c_void,
                &mut io,
            ) == -1
            {
                return Err(Error::PtraceFailed);
            }

            if libc::ptrace(
                libc::PTRACE_GETREGS,
                tid,
                ptr::null_mut::<libc::c_void>(),
                &mut gp_regs as *mut _,
            ) == -1
            {
                return Err(Error::PtraceFailed);
            }

            // When running an arm build on an arm64 device, attempting to get the
            // floating point registers fails. On Android, the floating point registers
            // aren't written to the cpu context anyway, so just don't get them here.
            // See http://crbug.com/508324
            if cfg!(not(all(target_os = "android", target_arch = "aarch"))) {
                if libc::ptrace(
                    libc::PTRACE_GETFPREGS,
                    tid,
                    ptr::null_mut::<libc::c_void>(),
                    &mut fp_regs as *mut _,
                ) == -1
                {
                    return Err(Error::PtraceFailed);
                }
            }

            #[cfg(target_arch = "x86")]
            let fpx_regs = {
                let cpuid = raw_cpuid::CpuId::new();

                let mut fpx_regs: libc::user_fpxregs_struct = mem::zeroed();

                if cpuid
                    .get_feature_info()
                    .map_or(false, |fi| fi.has_fxsave_fxstor())
                {
                    if libc::ptrace(
                        libc::PTRACE_GETFPXREGS,
                        tid,
                        ptr::null_mut::<libc::c_void>(),
                        &mut fpx_regs as *mut _,
                    ) == -1
                    {
                        return Err(Error::PtraceFailed);
                    }
                }

                fpx_regs
            };

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            let debug_regs = {
                let mut dregs = [0; 8];

                for i in 0..dregs.len() {
                    let offset = memoffset::offset_of!(libc::user, u_debugreg)
                        + i * mem::size_of::<DebugReg>();

                    if libc::ptrace(
                        libc::PTRACE_PEEKUSER,
                        tid,
                        offset as *mut libc::c_void,
                        &mut dregs[i] as *mut _,
                    ) == -1
                    {
                        return Err(Error::PtraceFailed);
                    }
                }

                dregs
            };

            let stack_pointer;

            cfg_if::cfg_if! {
                if #[cfg(target_arch = "x86")] {
                    stack_pointer = gp_regs.esp as usize
                } else if #[cfg(target_arch = "x86_64")] {
                    stack_pointer = gp_regs.rsp as usize
                } else if #[cfg(target_arch = "aarch")] {
                    stack_pointer = gp_regs.arm_sp as usize
                } else if #[cfg(target_arch = "aarch64")] {
                    stack_pointer = gp_regs.sp as usize
                } else {
                    compile_error!("unsupported target architecture");
                }
            };

            Ok(Self {
                gp_regs,
                fp_regs,
                #[cfg(target_arch = "x86")]
                fpx_regs,
                #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                debug_regs,
                stack_pointer,
                tgid,
                parent,
            })
        }
    }

    pub fn get_ip(&self) -> usize {
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "x86_64")] {
                self.gp_regs.rip as usize
            } else if #[cfg(target_arch = "x86")] {
                self.gp_regs.eip as usize
            } else if #[cfg(target_arch = "aarch")] {
                self.gp_regs.uregs[15] as usize
            } else if #[cfg(target_arch = "aarch64")] {
                self.gp_regs.pc as usize
            } else {
                compile_error!("unsupported target architecture");
            }
        }
    }

    pub fn get_cpu_context(&self) -> RawContextCpu {
        use crate::utils::to_byte_array;
        #[allow(unused)]
        use minidump_common::format::*;

        const CONTROL: u32 = 0x1;
        const INTEGER: u32 = 0x2;
        const SEGMENTS: u32 = 0x4;
        const FLOATING_POINT: u32 = 0x8;
        const DEBUG_REGISTERS: u32 = 0x10;
        const EXTENDED_REGISTERS: u32 = 0x20;

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "x86_64")] {
                struct FloatSave {
                    control_word: u16,
                    status_word: u16,
                    tag_word: u8,
                    reserved1: u8,
                    error_opcode: u16,
                    error_offset: u32,
                    error_selector: u16,
                    reserved2: u16,
                    data_offset: u32,
                    data_selector: u16,
                    reserved3: u16,
                    mx_csr: u32,
                    mx_csr_mask: u32,
                    float_registers: [u128; 8],
                    xmm_registers: [u128; 16],
                    reserved4: [u8; 96],
                }

                let mut fs = FloatSave {
                    control_word: self.fp_regs.cwd,
                    status_word: self.fp_regs.swd,
                    tag_word: self.fp_regs.ftw as u8,
                    error_opcode: self.fp_regs.fop,
                    error_offset: self.fp_regs.rip as u32,
                    // We don't have these
                    error_selector: 0,
                    data_selector: 0,
                    data_offset: self.fp_regs.rdp as u32,
                    mx_csr: self.fp_regs.mxcsr,
                    mx_csr_mask: self.fp_regs.mxcr_mask,
                    float_registers: [0; 8],
                    xmm_registers: [0; 16],
                    reserved1: 0,
                    reserved2: 0,
                    reserved3: 0,
                    reserved4: [0; 96],
                };

                unsafe {
                    unimplemented!()
                    // fs.float_registers.copy_from_slice(std::mem::transmute(self.fp_regs.st_space));
                    // fs.xmm_registers.copy_from_slice(std::mem::transmute(self.fp_regs.xmm_space));
                }

                let mut cpu_ctx = RawContextCpu {
                    context_flags:
                        // x86_64
                        0x100000 |
                        CONTROL |
                        INTEGER |
                        SEGMENTS |
                        FLOATING_POINT,
                    cs: self.gp_regs.cs as u16,
                    ds: self.gp_regs.ds as u16,
                    es: self.gp_regs.es as u16,
                    fs: self.gp_regs.fs as u16,
                    gs: self.gp_regs.gs as u16,
                    ss: self.gp_regs.ss as u16,
                    eflags: self.gp_regs.eflags as u32,
                    dr0: self.debug_regs[0],
                    dr1: self.debug_regs[1],
                    dr2: self.debug_regs[2],
                    dr3: self.debug_regs[3],
                    // Not included in the minidump format
                    //dr4: self.debug_regs[0],
                    //dr5: self.debug_regs[0],
                    dr6: self.debug_regs[6],
                    dr7: self.debug_regs[7],
                    rax: self.gp_regs.rax,
                    rcx: self.gp_regs.rcx,
                    rdx: self.gp_regs.rdx,
                    rbx: self.gp_regs.rbx,
                    rsp: self.gp_regs.rsp,
                    rbp: self.gp_regs.rbp,
                    rsi: self.gp_regs.rsi,
                    rdi: self.gp_regs.rdi,
                    r8: self.gp_regs.r8,
                    r9: self.gp_regs.r9,
                    r10: self.gp_regs.r10,
                    r11: self.gp_regs.r11,
                    r12: self.gp_regs.r12,
                    r13: self.gp_regs.r13,
                    r14: self.gp_regs.r14,
                    r15: self.gp_regs.r15,
                    rip: self.gp_regs.rip,
                    ..Default::default()
                };

                cpu_ctx.float_save.copy_from_slice(to_byte_array(&fs));

                cpu_ctx
            } else if #[cfg(target_arch = "x86")] {
                let mut fs = FLOATING_SAVE_AREA_X86 {
                    control_word: self.fp_regs.cwd,
                    status_word: self.fp_regs.swd,
                    tag_word: self.fp_regs.twd,
                    error_offset: self.fp_regs.fip,
                    error_selector: self.fp_regs.fcs,
                    data_offset: self.fp_regs.foo,
                    data_selector: self.fp_regs.fos,
                    register_area: [0; 80],
                    cr0_npx_state: 0,
                };

                unsafe {
                    fs.register_area.copy_from_slice(to_byte_array(self.fp_regs.st_space)[..80]);
                }

                // This matches the Intel fpsave format.
                struct ExtendedRegisters {
                    control_word: u16,
                    status_word: u16,
                    tag_word: u16,
                    error_opcode: u16,
                    error_offset: u32,
                    error_selector: u16,
                    data_offset: u32,
                    data_selector: u16,
                    mx_csr: u32,
                    float_registers: [u8; 128],
                    xmm_registers: [u8; 128],
                }

                let mut er = ExtendedRegisters {
                    control_word: self.fp_regs.cwd,
                    status_word: self.fp_regs.swd,
                    tag_word: self.fp_regs.twd,
                    error_opcode: self.fpx_regs.fop,
                    error_offset: self.fpx_regs.fip,
                    error_selector: self.fpx_regs.fcs,
                    data_offset: self.fp_regs.foo,
                    data_selector: self.fp_regs.fos,
                    mx_csr: self.fpx_regs.mxcsr,
                    float_registers: [0u8; 128],
                    xmm_registers: [0u8; 128],
                };

                unsafe {
                    er.float_registers.copy_from_slice(to_byte_array(self.fpx_regs.st_space)[..128]);
                    er.xmm_registers.copy_from_slice(to_byte_array(self.fpx_regs.xmm_space)[..128]);
                }

                let mut cpu_ctx = RawContextCpu {
                    context_flags:
                        // x86
                        0x10000 |
                        CONTROL |
                        INTEGER |
                        SEGMENTS |
                        FLOATING_POINT |
                        DEBUG_REGISTERS |
                        EXTENDED_REGISTERS,
                    dr0: self.debug_regs[0],
                    dr1: self.debug_regs[1],
                    dr2: self.debug_regs[2],
                    dr3: self.debug_regs[3],
                    // Not included in the minidump format
                    //dr4: self.debug_regs[0],
                    //dr5: self.debug_regs[0],
                    dr6: self.debug_regs[6],
                    dr7: self.debug_regs[7],
                    gs: self.gp_regs.xgs,
                    fs: self.gp_regs.xfs,
                    es: self.gp_regs.xes,
                    ds: self.gp_regs.xds,
                    edi: self.gp_regs.edi,
                    esi: self.gp_regs.esi,
                    ebx: self.gp_regs.ebx,
                    edx: self.gp_regs.edx,
                    ecx: self.gp_regs.ecx,
                    eax: self.gp_regs.eax,
                    ebp: self.gp_regs.ebp,
                    eip: self.gp_regs.eip,
                    cs: self.gp_regs.xcs,
                    eflags: self.gp_regs.eflags,
                    esp: self.gp_regs.esp,
                    ss: self.gp_regs.xss,
                    float_save: fs,
                    ..Default::default()
                };

                unsafe {
                    cpu_ctx.extended_registers[..std::mem::size_of::<ExtendedRegisters>()].copy_from_slice(to_byte_array(&er));
                }

                cpu_ctx
            } else if #[cfg(target_arch = "aarch")] {
                // TODO:
            } else if #[cfg(target_arch = "aarch64")] {
                // TODO:
            } else {
                compile_error!("unsupported target architecture");
            }
        }
    }
}
