use crate::{minidump::MinidumpOutput, Error};
use std::{mem, ops::DerefMut, ptr};

// TODO: The original C++ code logs some failures, by using their own logger
// that does a direct write to stderr, ideally we would use log/tracing but
// `log` can have user filters applied to it that can do whatever they want,
// including allocation/etc that could be dangerous in a compromised context
// like we are in during some logging situations, and `tracing` has similar
// problems to `log` in addition to (I believe) requiring allocation

const MIN_STACK_SIZE: usize = 16 * 1024;
/// kill
const SI_USER: i32 = 0;
/// tkill, tgkill
const SI_TKILL: i32 = -6;

struct StackSave {
    old: Option<libc::stack_t>,
    new: libc::stack_t,
}

unsafe impl Send for StackSave {}

static STACK_SAVE: parking_lot::Mutex<Option<StackSave>> = parking_lot::const_mutex(None);

/// Create an alternative stack to run the signal handlers on. This is done since
/// the signal might have been caused by a stack overflow.
unsafe fn install_sigaltstack() -> Result<(), Error> {
    // Check to see if the existing sigaltstack, if it exists, is big
    // enough. If so we don't need to allocate our own.
    let mut old_stack = mem::zeroed();
    let r = libc::sigaltstack(ptr::null(), &mut old_stack);
    assert_eq!(
        r,
        0,
        "learning about sigaltstack failed: {}",
        std::io::Error::last_os_error()
    );

    if old_stack.ss_flags & libc::SS_DISABLE == 0 && old_stack.ss_size >= MIN_STACK_SIZE {
        return Ok(());
    }

    // ... but failing that we need to allocate our own, so do all that
    // here.
    let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    let guard_size = page_size;
    let alloc_size = guard_size + MIN_STACK_SIZE;

    let ptr = libc::mmap(
        ptr::null_mut(),
        alloc_size,
        libc::PROT_NONE,
        libc::MAP_PRIVATE | libc::MAP_ANON,
        -1,
        0,
    );
    if ptr == libc::MAP_FAILED {
        return Err(Error::OutOfMemory);
    }

    // Prepare the stack with readable/writable memory and then register it
    // with `sigaltstack`.
    let stack_ptr = (ptr as usize + guard_size) as *mut libc::c_void;
    let r = libc::mprotect(
        stack_ptr,
        MIN_STACK_SIZE,
        libc::PROT_READ | libc::PROT_WRITE,
    );
    assert_eq!(
        r,
        0,
        "mprotect to configure memory for sigaltstack failed: {}",
        std::io::Error::last_os_error()
    );
    let new_stack = libc::stack_t {
        ss_sp: stack_ptr,
        ss_flags: 0,
        ss_size: MIN_STACK_SIZE,
    };
    let r = libc::sigaltstack(&new_stack, ptr::null_mut());
    assert_eq!(
        r,
        0,
        "registering new sigaltstack failed: {}",
        std::io::Error::last_os_error()
    );

    *STACK_SAVE.lock() = Some(StackSave {
        old: (old_stack.ss_flags & libc::SS_DISABLE != 0).then(|| old_stack),
        new: new_stack,
    });

    Ok(())
}

unsafe fn restore_sigaltstack() {
    let mut ssl = STACK_SAVE.lock();

    // Only restore the old_stack if the current alternative stack is the one
    // installed by the call to install_sigaltstack.
    if let Some(ss) = ssl.deref_mut() {
        let mut current_stack = mem::zeroed();
        if libc::sigaltstack(ptr::null(), &mut current_stack) == -1 {
            return;
        }

        if current_stack.ss_sp == ss.new.ss_sp {
            match ss.old {
                // Restore the old alt stack if there was one
                Some(old) => {
                    if libc::sigaltstack(&old, ptr::null_mut()) == -1 {
                        return;
                    }
                }
                // Restore to the default alt stack otherwise
                None => {
                    let mut disable: libc::stack_t = mem::zeroed();
                    disable.ss_flags = libc::SS_DISABLE;
                    if libc::sigaltstack(&disable, ptr::null_mut()) == -1 {
                        return;
                    }
                }
            }
        }

        let r = libc::munmap(ss.new.ss_sp, ss.new.ss_size);
        debug_assert_eq!(r, 0, "munmap failed during thread shutdown");
        *ssl = None;
    }
}

/// Restores the signal handler for the specified signal back to its original
/// handler
unsafe fn install_default_handler(sig: libc::c_int) {
    // Android L+ expose signal and sigaction symbols that override the system
    // ones. There is a bug in these functions where a request to set the handler
    // to SIG_DFL is ignored. In that case, an infinite loop is entered as the
    // signal is repeatedly sent to breakpad's signal handler.
    // To work around this, directly call the system's sigaction.

    if cfg!(target_os = "android") {
        let mut sa: libc::sigaction = mem::zeroed();
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_sigaction = libc::SIG_DFL;
        sa.sa_flags = libc::SA_RESTART;
        libc::syscall(
            libc::SYS_rt_sigaction,
            sig,
            &sa,
            ptr::null::<libc::sigaction>(),
            mem::size_of::<libc::sigset_t>(),
        );
    } else {
        libc::signal(sig, libc::SIG_DFL);
    }
}

/// The various signals we attempt to handle
const EXCEPTION_SIGNALS: [libc::c_int; 6] = [
    libc::SIGSEGV,
    libc::SIGABRT,
    libc::SIGFPE,
    libc::SIGILL,
    libc::SIGBUS,
    libc::SIGTRAP,
];

static OLD_HANDLERS: parking_lot::Mutex<Option<[libc::sigaction; 6]>> =
    parking_lot::const_mutex(None);

/// Restores all of the signal handlers back to their previous values, or the
/// default if the previous value cannot be restored
unsafe fn restore_handlers() {
    let mut ohl = OLD_HANDLERS.lock();

    if let Some(old) = &*ohl {
        for (sig, action) in EXCEPTION_SIGNALS.iter().copied().zip(old.iter()) {
            if libc::sigaction(sig, action, ptr::null_mut()) == -1 {
                install_default_handler(sig);
            }
        }
    }

    *ohl = None;
}

unsafe fn install_handlers() {
    let mut ohl = OLD_HANDLERS.lock();

    if ohl.is_some() {
        return;
    }

    // Attempt store all of the current handlers so we can restore them later
    let mut old_handlers: [mem::MaybeUninit<libc::sigaction>; 6] =
        mem::MaybeUninit::uninit().assume_init();

    for (sig, handler) in EXCEPTION_SIGNALS
        .iter()
        .copied()
        .zip(old_handlers.iter_mut())
    {
        let mut old = mem::zeroed();
        if libc::sigaction(sig, ptr::null(), &mut old) == -1 {
            return;
        }
        *handler = mem::MaybeUninit::new(old);
    }

    let mut sa: libc::sigaction = mem::zeroed();
    libc::sigemptyset(&mut sa.sa_mask);

    // Mask all exception signals when we're handling one of them.
    for sig in EXCEPTION_SIGNALS {
        libc::sigaddset(&mut sa.sa_mask, sig);
    }

    sa.sa_sigaction = signal_handler as usize;
    sa.sa_flags = libc::SA_ONSTACK | libc::SA_SIGINFO;

    // Use our signal_handler for all of the signals we wish to catch
    for sig in EXCEPTION_SIGNALS {
        // At this point it is impractical to back out changes, and so failure to
        // install a signal is intentionally ignored.
        libc::sigaction(sig, &sa, ptr::null_mut());
    }

    // Everything is initialized. Transmute the array to the
    // initialized type.
    *ohl = Some(mem::transmute::<_, [libc::sigaction; 6]>(old_handlers));
}

static HANDLER_STACK: parking_lot::Mutex<Vec<std::sync::Weak<HandlerInner>>> =
    parking_lot::const_mutex(Vec::new());

unsafe extern "C" fn signal_handler(
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    uc: *mut libc::c_void,
) {
    let info = &mut *info;
    let uc = &mut *uc;

    {
        let handlers = HANDLER_STACK.lock();

        // Sometimes, Breakpad runs inside a process where some other buggy code
        // saves and restores signal handlers temporarily with 'signal'
        // instead of 'sigaction'. This loses the SA_SIGINFO flag associated
        // with this function. As a consequence, the values of 'info' and 'uc'
        // become totally bogus, generally inducing a crash.
        //
        // The following code tries to detect this case. When it does, it
        // resets the signal handlers with sigaction + SA_SIGINFO and returns.
        // This forces the signal to be thrown again, but this time the kernel
        // will call the function with the right arguments.
        {
            let mut cur_handler = mem::zeroed();
            if libc::sigaction(sig, ptr::null_mut(), &mut cur_handler) == 0
                && cur_handler.sa_sigaction == signal_handler as usize
                && cur_handler.sa_flags & libc::SA_SIGINFO == 0
            {
                // Reset signal handler with the correct flags.
                libc::sigemptyset(&mut cur_handler.sa_mask);
                libc::sigaddset(&mut cur_handler.sa_mask, sig);

                cur_handler.sa_sigaction = signal_handler as usize;
                cur_handler.sa_flags = libc::SA_ONSTACK | libc::SA_SIGINFO;

                if libc::sigaction(sig, &cur_handler, ptr::null_mut()) == -1 {
                    // When resetting the handler fails, try to reset the
                    // default one to avoid an infinite loop here.
                    install_default_handler(sig);
                }

                // exit the handler as we should be called again soon
                return;
            }
        }

        let handled = (|| {
            for handler in handlers.iter() {
                if let Some(handler) = handler.upgrade() {
                    if handler.handle_signal(sig, info, uc) {
                        return true;
                    }
                }
            }

            false
        })();

        // Upon returning from this signal handler, sig will become unmasked and then
        // it will be retriggered. If one of the ExceptionHandlers handled it
        // successfully, restore the default handler. Otherwise, restore the
        // previously installed handler. Then, when the signal is retriggered, it will
        // be delivered to the appropriate handler.
        if handled {
            install_default_handler(sig);
        } else {
            restore_handlers();
        }
    }

    if info.si_code <= 0 || sig == libc::SIGABRT {
        // This signal was triggered by somebody sending us the signal with kill().
        // In order to retrigger it, we have to queue a new signal by calling
        // kill() ourselves.  The special case (si_pid == 0 && sig == SIGABRT) is
        // due to the kernel sending a SIGABRT from a user request via SysRQ.
        let tid = libc::gettid();
        if libc::syscall(libc::SYS_tgkill, std::process::id(), tid, sig) < 0 {
            // If we failed to kill ourselves (e.g. because a sandbox disallows us
            // to do so), we instead resort to terminating our process. This will
            // result in an incorrect exit code.
            libc::_exit(1);
        }
    } else {
        // This was a synchronous signal triggered by a hard fault (e.g. SIGSEGV).
        // No need to reissue the signal. It will automatically trigger again,
        // when we return from the signal handler.
    }
}

pub(crate) struct CrashContext {
    /// The signal info for the crash
    pub(crate) siginfo: nix::sys::signalfd::siginfo,
    /// The crashing thread
    pub(crate) tid: libc::pid_t,
    pub(crate) context: Option<crate::linux::UContext>,
    /// Float state. This isn't part of the user ABI for Linux aarch, and is
    /// already part of ucontext_t in mips
    #[cfg(not(all(target_arch = "aarch", target_arch = "mips", target_arch = "mips64")))]
    pub(crate) float_state: libc::_libc_fpstate,
}

impl CrashContext {
    pub(crate) fn get_cpu_context(&self) -> Option<super::thread_info::RawContextCpu> {
        let mut cpu_ctx = self.context.as_ref().map(|uc| uc.get_cpu_context())?;

        #[cfg(not(all(target_arch = "aarch", target_arch = "mips", target_arch = "mips64")))]
        {
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "x86")] {
                    compile_error!("impelement me");
                } else if #[cfg(target_arch = "x86_64")] {
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

                    let fpregs = &self.float_state;

                    let mut fs = FloatSave {
                        control_word: fpregs.cwd,
                        status_word: fpregs.swd,
                        tag_word: fpregs.ftw as u8,
                        error_opcode: fpregs.fop,
                        error_offset: fpregs.rip as u32,
                        // We don't have these
                        error_selector: 0,
                        data_selector: 0,
                        data_offset: fpregs.rdp as u32,
                        mx_csr: fpregs.mxcsr,
                        mx_csr_mask: fpregs.mxcr_mask,
                        float_registers: [0; 8],
                        xmm_registers: [0; 16],
                        reserved1: 0,
                        reserved2: 0,
                        reserved3: 0,
                        reserved4: [0; 96],
                    };

                    unsafe {
                        unimplemented!()
                        // fs.float_registers.copy_from_slice(std::mem::transmute(fpregs._st));
                        // fs.xmm_registers.copy_from_slice(std::mem::transmute(fpregs._xmm));
                    }

                    cpu_ctx.float_save.copy_from_slice(crate::utils::to_byte_array(&fs));
                } else {
                    compile_error!("impelement me");
                }
            }
        }

        Some(cpu_ctx)
    }
}

unsafe impl Send for CrashContext {}

/// The size of `CrashContext` can be too big w.r.t the size of alternatate stack
/// for `signal_handler`. Keep the crash context as a .bss field.
static CRASH_CONTEXT: parking_lot::Mutex<mem::MaybeUninit<CrashContext>> =
    parking_lot::const_mutex(mem::MaybeUninit::uninit());

struct ThreadArgument {
    /// the crashing process
    pid: libc::pid_t,
    context: *const CrashContext,
    /// The handler performing the minidump creation
    handler: *const HandlerInner,
    pipe_read: libc::c_int,
    pipe_write: libc::c_int,
}

/// This is the entry function for the cloned process. We are in a compromised
/// context here.
extern "C" fn thread_entry(ta: *mut libc::c_void) -> libc::c_int {
    unsafe {
        let ta = &*ta.cast::<ThreadArgument>();

        // Close the write end of the pipe. This allows us to fail if the parent dies
        // while waiting for the continue signal.
        libc::close(ta.pipe_write);

        // Block here until the crashing process unblocks us when
        // we're allowed to use ptrace
        let mut received = 0u8;
        let r = loop {
            let res = libc::read(
                ta.pipe_read,
                &mut received as *mut _ as *mut libc::c_void,
                mem::size_of::<u8>(),
            );

            if res == -1 {
                let err = std::io::Error::last_os_error();
                if let Some(libc::EINTR) = err.raw_os_error() {
                    continue;
                }
            }

            break res;
        };

        if r == -1 {
            //log::error!("failed to wait for continue signal `read`: {}, std::io::Error::last_os_error());
        }

        libc::close(ta.pipe_read);

        if (*ta.handler).perform_dump(ta.pid, &*ta.context) {
            0
        } else {
            1
        }
    }
}

struct HandlerInner {
    output: MinidumpOutput,
    on_crash: Option<Box<dyn crate::CrashEvent>>,
}

impl HandlerInner {
    unsafe fn handle_signal(
        &self,
        _sig: libc::c_int,
        info: &mut libc::siginfo_t,
        uc: &mut libc::c_void,
    ) -> bool {
        //     if (filter_ && !filter_(callback_context_))
        // return false;

        // The siginfo_t in libc is lowest common denominator, but this code is
        // specifically targeting linux/android, which contains the si_pid field
        // that we require
        let nix_info = &*((info as *const libc::siginfo_t).cast::<nix::sys::signalfd::siginfo>());

        // Allow ourselves to be dumped if the signal is trusted.
        if info.si_code > 0
            || ((info.si_code == SI_USER || info.si_code == SI_TKILL)
                && nix_info.ssi_pid == std::process::id())
        {
            libc::syscall(libc::SYS_prctl, libc::PR_SET_DUMPABLE, 1, 0, 0, 0);
        }

        let mut crash_ctx = CRASH_CONTEXT.lock();

        *crash_ctx = mem::MaybeUninit::zeroed();
        ptr::copy_nonoverlapping(nix_info, &mut (*(*crash_ctx).as_mut_ptr()).siginfo, 1);

        let uc_ptr = &*(uc as *const libc::c_void).cast::<libc::ucontext_t>();
        let mut uctx = mem::MaybeUninit::<libc::ucontext_t>::zeroed();
        ptr::copy_nonoverlapping(uc_ptr, uctx.as_mut_ptr(), 1);

        (*crash_ctx.as_mut_ptr()).context = Some(crate::linux::UContext {
            inner: uctx.assume_init(),
        });

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "aarch64")] {
                let fp_ptr = uc_ptr.uc_mcontext.__reserved.cast::<libc::fpsimd_context>();

                if fp_ptr.head.magic == libc::FPSIMD_MAGIC {
                    ptr::copy_nonoverlapping(fp_ptr, &mut (*(*crash_ctx).as_mut_ptr()).float_state, mem::size_of::<libc::_libc_fpstate>());
                }
            } else if #[cfg(not(all(
                target_arch = "aarch",
                target_arch = "mips",
                target_arch = "mips64")))] {
                if !uc_ptr.uc_mcontext.fpregs.is_null() {
                    ptr::copy_nonoverlapping(uc_ptr.uc_mcontext.fpregs, &mut (*(*crash_ctx).as_mut_ptr()).float_state, 1);

                }
            } else {
            }
        }

        (*(*crash_ctx).as_mut_ptr()).tid = libc::syscall(libc::SYS_gettid) as i32;

        self.generate_dump(&*crash_ctx.as_ptr())
    }

    unsafe fn generate_dump(&self, ctx: &CrashContext) -> bool {
        // if (IsOutOfProcess())
        //     return crash_generation_client_->RequestDump(context, sizeof(*context));

        const CHILD_STACK_SIZE: usize = 16000;

        let stack = libc::mmap(
            ptr::null_mut(),
            CHILD_STACK_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        if stack == libc::MAP_FAILED {
            return false;
        }

        let mut stack = stack.cast::<u8>();
        // clone() needs the top-most address. (scrub just to be safe)
        stack = stack.offset(CHILD_STACK_SIZE as isize);
        // let header = std::slice::from_raw_parts_mut(stack.sub(16), 16);
        // header.fill(0);

        // We need to explicitly enable ptrace of parent processes on some
        // kernels, but we need to know the PID of the cloned process before we
        // can do this. We create a pipe which we can use to block the
        // cloned process after creating it, until we have explicitly enabled
        // ptrace. This is used to store the file descriptors for the pipe
        let mut fds = [-1, -1];

        // We need to explicitly enable ptrace of parent processes on some
        // kernels, but we need to know the PID of the cloned process before we
        // can do this. Create a pipe here which we can use to block the
        // cloned process after creating it, until we have explicitly enabled ptrace
        if libc::pipe(fds.as_mut_ptr()) == -1 {
            // Creating the pipe failed. We'll log an error but carry on anyway,
            // as we'll probably still get a useful crash report. All that will happen
            // is the write() and read() calls will fail with EBADF
            //log::error!(generate_dump failed to create a pipe: {}", std::io::Error::last_os_error());

            fds.fill(-1);
        }

        let pipe_read = fds[0];
        let pipe_write = fds[1];

        let mut thread_args = ThreadArgument {
            handler: self as *const Self,
            pid: libc::getpid(),
            context: ctx,
            pipe_read,
            pipe_write,
        };

        let child = libc::clone(
            thread_entry,
            stack.cast::<libc::c_void>(),
            libc::CLONE_FS | libc::CLONE_UNTRACED,
            (&mut thread_args as *mut ThreadArgument).cast::<libc::c_void>(),
        );
        if child == -1 {
            libc::close(pipe_read);
            libc::close(pipe_write);
            return false;
        }

        libc::close(pipe_read);
        // Allow the child to ptrace us
        libc::syscall(libc::SYS_prctl, libc::PR_SET_PTRACER, child, 0, 0, 0);

        let ok_to_continue = b'a';
        let r = loop {
            let res = libc::write(
                pipe_write,
                (&ok_to_continue as *const u8).cast::<libc::c_void>(),
                mem::size_of::<u8>(),
            );

            if res == -1 {
                let err = std::io::Error::last_os_error();
                if let Some(libc::EINTR) = err.raw_os_error() {
                    continue;
                }
            }

            break res;
        };

        if r == -1 {
            //log::error!("failed to send continue signal to child `write` failed: {}", std::io::Error::last_os_error());
        }

        let mut status = 0;
        let r = loop {
            let res = libc::waitpid(child, &mut status, libc::__WALL);

            if res == -1 {
                let err = std::io::Error::last_os_error();
                if let Some(libc::EINTR) = err.raw_os_error() {
                    continue;
                }
            }

            break res;
        };

        libc::close(pipe_write);

        if r == -1 {
            //log::error!(generate_dump waitpid failed: {}", std::io::Error::last_os_error());
        }

        let mut great_success =
            r != -1 && libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;

        if let Some(on_crash) = &self.on_crash {
            great_success = on_crash.on_crash(&self.output, great_success);
        }

        great_success
    }

    unsafe fn perform_dump(&self, crashing_process: libc::pid_t, context: &CrashContext) -> bool {
        //         const bool may_skip_dump =
        //       minidump_descriptor_.skip_dump_if_principal_mapping_not_referenced();
        //   const uintptr_t principal_mapping_address =
        //       minidump_descriptor_.address_within_principal_mapping();
        //   const bool sanitize_stacks = minidump_descriptor_.sanitize_stacks();
        //   if (minidump_descriptor_.IsMicrodumpOnConsole()) {
        //     return google_breakpad::WriteMicrodump(
        //         crashing_process,
        //         context,
        //         context_size,
        //         mapping_list_,
        //         may_skip_dump,
        //         principal_mapping_address,
        //         sanitize_stacks,
        //         *minidump_descriptor_.microdump_extra_info());
        //   }
        //   if (minidump_descriptor_.IsFD()) {
        //     return google_breakpad::WriteMinidump(minidump_descriptor_.fd(),
        //                                           minidump_descriptor_.size_limit(),
        //                                           crashing_process,
        //                                           context,
        //                                           context_size,
        //                                           mapping_list_,
        //                                           app_memory_list_,
        //                                           may_skip_dump,
        //                                           principal_mapping_address,
        //                                           sanitize_stacks);
        //   }
        //   return google_breakpad::WriteMinidump(minidump_descriptor_.path(),
        //                                         minidump_descriptor_.size_limit(),
        //                                         crashing_process,
        //                                         context,
        //                                         context_size,
        //                                         mapping_list_,
        //                                         app_memory_list_,
        //                                         may_skip_dump,
        //                                         principal_mapping_address,
        //                                         sanitize_stacks);
        false
    }
}

pub struct ExceptionHandler {
    inner: std::sync::Arc<HandlerInner>,
}

impl ExceptionHandler {
    pub fn attach(
        output: MinidumpOutput,
        on_crash: Option<Box<dyn crate::CrashEvent>>,
    ) -> Result<Self, Error> {
        unsafe {
            install_sigaltstack()?;
            install_handlers();
        }

        let inner = std::sync::Arc::new(HandlerInner { output, on_crash });

        {
            let mut handlers = HANDLER_STACK.lock();
            handlers.push(std::sync::Arc::downgrade(&inner));
        }

        Ok(Self { inner })
    }

    pub fn detach(self) {
        self.do_detach();
    }

    fn do_detach(&self) {
        let mut handlers = HANDLER_STACK.lock();

        if let Some(ind) = handlers.iter().position(|handler| {
            handler.upgrade().map_or(false, |handler| {
                std::sync::Arc::ptr_eq(&handler, &self.inner)
            })
        }) {
            handlers.remove(ind);

            if handlers.is_empty() {
                unsafe {
                    restore_sigaltstack();
                    restore_handlers();
                }
            }
        }
    }

    // Add information about a memory mapping. This can be used if
    // a custom library loader is used that maps things in a way
    // that the linux dumper can't handle by reading the maps file.
    //   void AddMappingInfo(const string& name,
    //     const uint8_t identifier[sizeof(MDGUID)],
    //     uintptr_t start_address,
    //     size_t mapping_size,
    //     size_t file_offset);

    // // Register a block of memory of length bytes starting at address ptr
    // // to be copied to the minidump when a crash happens.
    // void RegisterAppMemory(void* ptr, size_t length);

    // // Unregister a block of memory that was registered with RegisterAppMemory.
    // void UnregisterAppMemory(void* ptr);

    /// Force signal handling for the specified signal.
    pub fn simulate_signal(&self, signal: i32) -> bool {
        unsafe {
            let mut siginfo: nix::sys::signalfd::siginfo = mem::zeroed();
            siginfo.ssi_code = SI_USER;
            siginfo.ssi_pid = std::process::id();

            let mut context = mem::zeroed();
            libc::getcontext(&mut context);

            self.inner.handle_signal(
                signal,
                &mut *(&mut siginfo as *mut nix::sys::signalfd::siginfo).cast::<libc::siginfo_t>(),
                &mut *(&mut context as *mut libc::ucontext_t).cast::<libc::c_void>(),
            )
        }
    }
}

impl Drop for ExceptionHandler {
    fn drop(&mut self) {
        self.do_detach();
    }
}
