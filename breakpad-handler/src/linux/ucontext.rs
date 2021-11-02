use super::thread_info::RawContextCpu;

/// Wrapper around [`libc::ucontext_t`]
pub struct UContext {
    pub inner: libc::ucontext_t,
}

impl UContext {
    #[inline]
    pub fn stack_pointer(&self) -> usize {
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "x86")] {
                self.inner.uc_mcontext.gregs[libc::REG_ESP as usize] as usize
            } else if #[cfg(target_arch = "x86_64")] {
                self.inner.uc_mcontext.gregs[libc::REG_RSP as usize] as usize
            } else if #[cfg(target_arch = "aarch")] {
                self.inner.uc_mcontext.arm_sp as usize
            } else if #[cfg(target_arch = "aarch64")] {
                self.inner.uc_mcontext.sp as usize
            } else {
                compile_error!("unsupported target architecture");
            }
        }
    }

    #[inline]
    pub fn instruction_pointer(&self) -> usize {
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "x86")] {
                self.inner.uc_mcontext.gregs[libc::REG_EIP as usize] as usize
            } else if #[cfg(target_arch = "x86_64")] {
                self.inner.uc_mcontext.gregs[libc::REG_RIP as usize] as usize
            } else if #[cfg(target_arch = "aarch")] {
                self.inner.uc_mcontext.arm_pc as usize
            } else if #[cfg(target_arch = "aarch64")] {
                self.inner.uc_mcontext.pc as usize
            } else {
                compile_error!("unsupported target architecture");
            }
        }
    }

    pub fn get_cpu_context(&self) -> RawContextCpu {
        #[allow(unused)]
        use minidump_common::format::*;

        const CONTROL: u32 = 0x1;
        const INTEGER: u32 = 0x2;
        const SEGMENTS: u32 = 0x4;
        const FLOATING_POINT: u32 = 0x8;
        const DEBUG_REGISTERS: u32 = 0x10;
        const EXTENDED_REGISTERS: u32 = 0x20;

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "x86")] {
                let gregs = &self.inner.uc_mcontext.gregs;

                RawContextCpu {
                    context_flags: // x86
                    0x10000 |
                    CONTROL |
                    INTEGER |
                    SEGMENTS |
                    FLOATING_POINT,
                    gs: gregs[libc::REG_GS],
                    fs: gregs[libc::REG_FS],
                    es: gregs[libc::REG_ES],
                    ds: gregs[libc::REG_DS],
                    edi: gregs[libc::REG_EDI],
                    esi: gregs[libc::REG_ESI],
                    ebx: gregs[libc::REG_EBX],
                    edx: gregs[libc::REG_EDX],
                    ecx: gregs[libc::REG_ECX],
                    eax: gregs[libc::REG_EAX],
                    ebp: gregs[libc::REG_EBP],
                    eip: gregs[libc::REG_EIP],
                    cs: gregs[libc::REG_CS],
                    eflags: gregs[libc::REG_EFL],
                    esp: gregs[libc::REG_UESP],
                    ss: gregs[libc::REG_SS],
                    ..Default::default()
                }
            } else if #[cfg(target_arch = "x86_64")] {
                let gregs = &self.inner.uc_mcontext.gregs;

                RawContextCpu {
                    context_flags:
                        // x86_64
                        0x100000 |
                        CONTROL |
                        INTEGER |
                        FLOATING_POINT,
                    cs: (gregs[libc::REG_CSGSFS as usize] & 0xffff) as u16,
                    fs: ((gregs[libc::REG_CSGSFS as usize] >> 32) & 0xffff) as u16,
                    gs: ((gregs[libc::REG_CSGSFS as usize] >> 16) & 0xffff) as u16,
                    eflags: gregs[libc::REG_EFL as usize] as u32,
                    rax: gregs[libc::REG_RAX as usize] as u64,
                    rcx: gregs[libc::REG_RCX as usize] as u64,
                    rdx: gregs[libc::REG_RDX as usize] as u64,
                    rbx: gregs[libc::REG_RBX as usize] as u64,
                    rsp: gregs[libc::REG_RSP as usize] as u64,
                    rbp: gregs[libc::REG_RBP as usize] as u64,
                    rsi: gregs[libc::REG_RSI as usize] as u64,
                    rdi: gregs[libc::REG_RDI as usize] as u64,
                    r8: gregs[libc::REG_R8 as usize] as u64,
                    r9: gregs[libc::REG_R9 as usize] as u64,
                    r10: gregs[libc::REG_R10 as usize] as u64,
                    r11: gregs[libc::REG_R11 as usize] as u64,
                    r12: gregs[libc::REG_R12 as usize] as u64,
                    r13: gregs[libc::REG_R13 as usize] as u64,
                    r14: gregs[libc::REG_R14 as usize] as u64,
                    r15: gregs[libc::REG_R15 as usize] as u64,
                    rip: gregs[libc::REG_RIP as usize] as u64,
                    ..Default::default()
                }
            } else if #[cfg(target_arch = "aarch")] {
                compile_error!("implement me");
            } else if #[cfg(target_arch = "aarch64")] {
                compile_error!("implement me");
            } else {
                compile_error!("unsupported target architecture");
            }
        }
    }
}
