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
                let mut dregs = [DebugReg; 8];

                for i in dregs.len() {
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
}
