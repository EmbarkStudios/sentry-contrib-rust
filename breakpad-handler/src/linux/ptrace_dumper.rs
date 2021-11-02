use crate::{
    alloc::{Allocator, PageVec},
    utils::{self, fs, FixedCStr},
};
use std::{
    fmt::{self, Write},
    io::Read,
    mem,
};

// When we find the VDSO mapping in the process's address space, this
// is the name we use for it when writing it to the minidump.
// This should always be less than NAME_MAX!
const LINUX_GATE_LIBRARY_NAME: &str = "linux-gate.so";

cfg_if::cfg_if! {
    if #[cfg(target_pointer_width = "32")] {
        #[derive(Copy, Clone)]
        #[repr(C)]
        struct ElfAux {
            kind: AtKinds,
            val: u32,
        }

        impl ElfAux {
            fn from_bytes(bytes: [u8; 8]) -> Option<Self> {
                unsafe {
                    let kind = mem::transmute::<[u8; 4], u32>(bytes[..4].try_into().unwrap());
                    let kind = AtKinds::from_int(kind)?;

                    let val = mem::transmute::<[u8; 4], u32>(bytes[4..].try_into().unwrap());

                    Some(Self {
                        kind,
                        val,
                    })
                }
            }
        }
    } else if #[cfg(target_pointer_width = "64")] {
        #[derive(Copy, Clone)]
        #[repr(C)]
        struct ElfAux {
            kind: AtKinds,
            val: u64,
        }

        impl ElfAux {
            fn from_bytes(bytes: [u8; 16]) -> Option<Self> {
                use std::convert::TryInto;
                unsafe {
                    let kind = mem::transmute::<[u8; 8], u64>(bytes[..8].try_into().unwrap());
                    let kind = AtKinds::from_int(kind as u32)?;

                    let val = mem::transmute::<[u8; 8], u64>(bytes[8..].try_into().unwrap());

                    Some(Self {
                        kind,
                        val,
                    })
                }
            }
        }
    } else {
        compile_error!("invalid target_pointer_width");
    }
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(target_pointer_width = "32", repr(u32))]
#[cfg_attr(target_pointer_width = "64", repr(u64))]
pub enum AtKinds {
    Null = 0,
    /// File descriptor of the program
    ExecFD = 2,
    /// Address of the program headers of the executable
    ProgramHeaders = 3,
    /// Size of [`ProgramHeaders`]
    PHEntrySize = 4,
    /// The number of [`ProgramHeaders`]
    PHNum = 5,
    /// The system page size
    PageSize = 6,
    /// The base address of the program interpreter (eg dynamic linker)
    Base = 7,
    /// Flags (unused)
    Flags = 8,
    /// The entry address of the executable
    Entry = 9,
    /// Elf note
    NotElf = 10,
    /// The real user ID of the thread
    Uid = 11,
    /// The effective user ID of the thread
    EUid = 12,
    /// The real group ID of the thread
    Gid = 13,
    /// The effective group ID of the thread
    EGid = 14,
    ///  A pointer to a string that identifies the hardware platform that the
    /// program is running on.  The dynamic linker uses this in the
    /// interpretation of `rpath` values.
    Platform = 15,
    /// An architecture and ABI dependent bit-mask whose settings indicate
    /// detailed processor capabilities.  The contents of the bit mask are
    /// hardware dependent.  A human-readable version of the same information
    /// is available via `/proc/cpuinfo`.
    HardwareCapabilities = 16,
    /// The frequency at which [time](https://man7.org/linux/man-pages/man2/times.2.html) counts
    ClockTick = 17,
    /// Used FPU control word (SuperH architecture only).  This gives some
    /// information about the FPU initialization performed by the kernel.
    FpuControlWord = 18,
    /// Data cache block size
    DCacheBlockSize = 19,
    /// Instruction cache block size
    ICacheBlockSize = 20,
    /// Unified cache block size
    UCacheBlockSize = 21,
    //IgnorePPC = 22,
    /// Has a nonzero value if this executable should be treated securely.  Most
    /// commonly, a nonzero value indicates that the process is executing a
    /// set-user-ID or set-group-ID binary (so that its real and effective UIDs
    /// or GIDs differ from one another), or that it gained capabilities by
    /// executing a binary file that has [capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html).
    /// Alternatively, a nonzero value may be triggered by a Linux Security
    /// Module.  When this value is nonzero, the dynamic linker disables the use
    /// of [certain environment variables](https://man7.org/linux/man-pages/man8/ld-linux.so.8.html)
    /// and glibc changes other aspects of its [behavior](https://man7.org/linux/man-pages/man3/secure_getenv.3.html).
    Secure = 23,
    /// A pointer to a string (PowerPC and MIPS only).  On PowerPC, this
    /// identifies the real platform; may differ from [`Platform`]. On MIPS,
    /// this identifies the ISA level (since Linux 5.7).
    BasePlatform = 24,
    /// The address of sixteen bytes containing a random value.
    Random = 25,
    /// Further machine-dependent hints about processor capabilities.
    HardwareCaps2 = 26,
    /// A pointer to a string containing the pathname used to execute the
    /// program.
    ExecPath = 27,
    /// The entry point to the system call function in the vDSO. Not
    /// present/needed on all architectures (e.g., absent on x86-64).
    SysInfo = 32,
    /// The address of a page containing the virtual Dynamic Shared Object
    /// (vDSO) that the kernel creates in order to provide fast implementations
    /// of certain system calls.
    SysInfoEhdr = 33,
}

impl AtKinds {
    fn from_int(val: u32) -> Option<Self> {
        Some(match val {
            2 => Self::ExecFD,
            3 => Self::ProgramHeaders,
            4 => Self::PHEntrySize,
            5 => Self::PHNum,
            6 => Self::PageSize,
            7 => Self::Base,
            8 => Self::Flags,
            9 => Self::Entry,
            10 => Self::NotElf,
            11 => Self::Uid,
            12 => Self::EUid,
            13 => Self::Gid,
            14 => Self::EGid,
            15 => Self::Platform,
            16 => Self::HardwareCapabilities,
            17 => Self::ClockTick,
            18 => Self::FpuControlWord,
            19 => Self::DCacheBlockSize,
            20 => Self::ICacheBlockSize,
            21 => Self::UCacheBlockSize,
            23 => Self::Secure,
            24 => Self::BasePlatform,
            25 => Self::Random,
            26 => Self::HardwareCaps2,
            27 => Self::ExecPath,
            32 => Self::SysInfo,
            33 => Self::SysInfoEhdr,
            _ => return None,
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no valid auxv entries were found")]
    NoValidAuxvEntries,
    #[error("missing valid entry for {0:?}")]
    MissingAuxvEntry(AtKinds),
    #[error(transparent)]
    Format(#[from] fmt::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a mapping entry is invalid")]
    InvalidMapping,
    #[error("no threads could be suspended")]
    NoValidThreads,
    #[error("threads are not suspended")]
    ThreadsNotSuspended,
    #[error("not all threads could be resumed")]
    AllThreadsNotResumed,
    #[error("a thread's status is invalid")]
    InvalidStatus,
    #[error("a ptrace syscall failed")]
    PtraceFailed,
}
#[cfg_attr(test, derive(PartialEq, Debug))]
pub struct MappingInfo {
    // On Android, relocation packing can mean that the reported start
    // address of the mapping must be adjusted by a bias in order to
    // compensate for the compression of the relocation section. The
    // following two members hold (after LateInit) the adjusted mapping
    // range. See crbug.com/606972 for more information.
    pub start_addr: usize,
    pub size: usize,
    // When Android relocation packing causes |start_addr| and |size| to
    // be modified with a load bias, we need to remember the unbiased
    // address range. The following structure holds the original mapping
    // address range as reported by the operating system.
    pub sys_start_addr: usize,
    pub sys_end_addr: usize,
    pub offset: usize,
    /// true if the mapping has the execute bit set.
    pub has_exec: bool,
    pub name: utils::FixedStr<255>,
}

impl MappingInfo {
    #[inline]
    pub fn contains_address(&self, address: usize) -> bool {
        self.start_addr <= address && self.start_addr + self.size > address
    }
}

impl std::str::FromStr for MappingInfo {
    type Err = Error;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        // start       - end         permissions offset   dev   inode       pathname
        // 7feca168a000-7feca1699000 rwxp        00007000 fd:00 1705088     /usr/lib64/libpthread-2.33.so
        fn do_parse(line: &str) -> Option<MappingInfo> {
            let dash_ind = line.find('-')?;
            let start_addr = usize::from_str_radix(&line[..dash_ind], 16).ok()?;

            let end = line[dash_ind + 1..].find(' ')? + dash_ind + 1;
            let end_addr = usize::from_str_radix(&line[dash_ind + 1..end], 16).ok()?;

            let has_exec = dbg!(&line[end + 1..end + 5]).find('x').is_some();

            let offset_end = line[end + 6..].find(' ')?;
            let offset =
                usize::from_str_radix(dbg!(&line[end + 6..end + 6 + offset_end]), 16).ok()?;

            let mut name = utils::FixedStr::<255>::new();

            // Find the path, special entries like [vdso] will be fixed up later
            if let Some(path_start) = line[offset_end..].find('/') {
                name.write_str(&line[offset_end + path_start..]).ok()?;
            }

            Some(MappingInfo {
                start_addr,
                size: end_addr - start_addr,
                sys_start_addr: start_addr,
                sys_end_addr: end_addr,
                offset,
                has_exec,
                name,
            })
        }

        do_parse(line).ok_or(Error::InvalidMapping)
    }
}

pub(crate) struct PTraceDumper {
    /// Path of the root directory to which mapping paths are relative.
    root_prefix: &'static str,
    /// Virtual address at which the process crashed.
    crash_address: usize,
    /// Signal that terminated the crashed process.
    crash_signal: i32,
    /// The code associated with `crash_signal`
    crash_signal_code: i32,
    /// The additional fields associated with `crash_signal`
    crash_exception_info: PageVec<u64>,
    /// PID for the crashing process
    pid: u32,
    /// ID of the crashed thread.
    crash_thread: libc::pid_t,
    /// IDs of all the threads.
    pub(crate) threads: PageVec<Option<u32>>,
    /// Info from /proc/<pid>/maps.
    mappings: PageVec<MappingInfo>,
    /// Info from /proc/<pid>/auxv
    auxv: PageVec<Option<usize>>,
    /// True if threads are currently suspended
    threads_suspended: bool,
}

impl PTraceDumper {
    pub fn new(
        allocator: Allocator,
        crashing_process: std::num::NonZeroU32,
        cc: &super::handler::CrashContext,
    ) -> Self {
        Self {
            root_prefix: "",
            crash_address: cc.siginfo.ssi_addr as usize,
            crash_signal: cc.siginfo.ssi_signo as i32,
            crash_signal_code: cc.siginfo.ssi_code,
            crash_exception_info: PageVec::new_in(allocator.clone()),
            pid: crashing_process.get(),
            crash_thread: cc.tid,
            threads: PageVec::new_in(allocator.clone()),
            mappings: PageVec::new_in(allocator.clone()),
            auxv: PageVec::new_in(allocator),
            threads_suspended: false,
        }
    }

    pub fn init(&mut self) -> Result<(), Error> {
        self.read_auxv()?;
        self.enumerate_threads()?;
        self.enumerate_mappings()?;

        Ok(())
    }

    pub fn is_post_mortem(&self) -> bool {
        false
    }

    pub fn set_crash_address(&mut self, addr: usize) {
        self.crash_address = addr;
    }

    fn read_auxv(&mut self) -> Result<(), Error> {
        let mut path = FixedCStr::<32>::new();
        write!(&mut path, "/proc/{}/auxv", self.pid)?;

        let mut oo = fs::OpenOptions::new();
        oo.read(true);

        let mut auxv = fs::open(&path, oo)?;

        // All interesting auvx entry types are below `SysInfoEhdr`
        self.auxv.resize(AtKinds::SysInfoEhdr as usize, None);

        const AUX_ENTRY_SIZE: usize = std::mem::size_of::<ElfAux>();
        let mut entry = [0u8; AUX_ENTRY_SIZE];

        let mut has_valid_entry = false;

        while let Ok(read) = auxv.read(&mut entry) {
            if read < AUX_ENTRY_SIZE {
                break;
            }

            if let Some(aux) = ElfAux::from_bytes(entry) {
                has_valid_entry = true;
                self.auxv[aux.kind as usize] = Some(aux.val as usize);
            }
        }

        if has_valid_entry {
            Ok(())
        } else {
            Err(Error::NoValidAuxvEntries)
        }
    }

    fn enumerate_threads(&mut self) -> Result<(), Error> {
        let mut path = FixedCStr::<32>::new();
        write!(&mut path, "/proc/{}/task", self.pid)?;

        // /proc/{pid}/task contains a subdirectory for each thread in the
        // process, named with its numerical thread id.
        let dr = fs::read_dir(&path)?;

        // The directory may contain duplicate entries which we filter by
        // assuming that they are consecutive.
        let mut last_tid = None;

        for entry in dr.filter_map(|res| res.ok()) {
            if let Some(name) = entry.file_name_os_str().to_str() {
                if let Some(tid) = name.parse().ok() {
                    if Some(tid) != last_tid {
                        last_tid = Some(tid);
                        self.threads.push(Some(tid));
                    }
                }
            }
        }

        Ok(())
    }

    fn enumerate_mappings(&mut self) -> Result<(), Error> {
        let mut path = FixedCStr::<32>::new();
        write!(&mut path, "/proc/{}/maps", self.pid)?;

        // linux_gate_loc is the beginning of the kernel's mapping of
        // linux-gate.so in the process.  It doesn't actually show up in the
        // maps list as a filename, but it can be found using the AT_SYSINFO_EHDR
        // aux vector entry, which gives the information necessary to special
        // case its entry when creating the list of mappings.
        // See https://gist.github.com/Jake-Shadle/6bfef4d461f55767227e1514ca829c4c
        // for more information.
        let linux_gate_loc = self.auxv[AtKinds::SysInfoEhdr as usize];

        // Although the initial executable is _usually_ the first mapping, it's
        // not guaranteed (see http://crosbug.com/25355); therefore, try to use
        // the actual entry point to find the mapping.
        let entry_point_loc = self.auxv[AtKinds::Entry as usize];

        let mut oo = fs::OpenOptions::new();
        oo.read(true);
        let mfile = fs::open(&path, oo)?;

        let line_reader = utils::LineReader::<_, 512>::new(mfile);

        for line in line_reader {
            let line = line.as_ref();

            let info = match line.parse::<MappingInfo>().ok() {
                Some(mut nfo) => {
                    if Some(nfo.start_addr) == linux_gate_loc {
                        nfo.name.clear();
                        nfo.name.write_str(LINUX_GATE_LIBRARY_NAME).ok();
                        // Sanity check
                        nfo.offset = 0;
                    }

                    let name = nfo.name.as_ref();

                    // Merge adjacent mappings into one module, assuming they're a single
                    // library mapped by the dynamic linker. Do this only if their name
                    // matches and either they have the same +x protection flag, or if the
                    // previous mapping is not executable and the new one is, to handle
                    // lld's output (see crbug.com/716484).
                    if let Some(last) = self.mappings.last_mut() {
                        if nfo.start_addr == last.start_addr + last.size
                            && name == last.name.as_ref()
                            && (nfo.has_exec == last.has_exec || !last.has_exec && nfo.has_exec)
                        {
                            last.sys_end_addr = nfo.sys_end_addr;
                            last.size = last.sys_end_addr - last.start_addr;
                            last.has_exec |= nfo.has_exec;
                            continue;
                        }
                    }

                    nfo
                }
                None => continue,
            };

            self.mappings.push(info);
        }

        // Find the module which contains the entry point, and if it's not
        // already the first one, then we need to make it be first.  This is
        // because the minidump format assumes the first module is the one that
        // corresponds to the main executable
        if let Some(ep) = entry_point_loc {
            if let Some(entry_pos) = self.mappings.iter().position(|mapping| {
                ep >= mapping.start_addr && ep < mapping.start_addr + mapping.size
            }) {
                if entry_pos != 0 {
                    let entry = self.mappings.remove(entry_pos);
                    self.mappings.insert(0, entry);
                }
            }
        }

        Ok(())
    }

    /// Find the mapping which the given memory address falls in. Uses the
    /// unadjusted mapping address range from the kernel, rather than the
    /// biased range.
    #[inline]
    pub fn find_mapping_no_bias(&self, address: usize) -> Option<&MappingInfo> {
        self.mappings
            .iter()
            .find(|mapping| mapping.contains_address(address))
    }

    pub fn suspend_threads(&mut self) -> Result<(), Error> {
        if self.threads_suspended {
            return Ok(());
        }

        fn suspend_thread(tid: u32) -> bool {
            use std::ptr;

            // This may fail if the thread has just died or debugged.
            errno::set_errno(errno::Errno(0));

            if libc::ptrace(
                libc::PTRACE_ATTACH,
                tid,
                ptr::null::<u8>(),
                ptr::null::<u8>(),
            ) != 0
                && errno::errno().0 != 0
            {
                return false;
            }

            while libc::waitpid(tid as i32, ptr::null_mut(), libc::__WALL) < 0 {
                if errno::errno().0 == libc::EINTR {
                    libc::ptrace(
                        libc::PTRACE_DETACH,
                        tid,
                        ptr::null::<u8>(),
                        ptr::null::<u8>(),
                    );
                    return false;
                }
            }

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                // On x86, the stack pointer is NULL or -1, when executing trusted code in
                // the seccomp sandbox. Not only does this cause difficulties down the line
                // when trying to dump the thread's stack, it also results in the minidumps
                // containing information about the trusted threads. This information is
                // generally completely meaningless and just pollutes the minidumps.
                // We thus test the stack pointer and exclude any threads that are part of
                // the seccomp sandbox's trusted code.
                let mut regs: libc::user_regs_struct = std::mem::zeroed();

                let grsuc = libc::ptrace(
                    libc::PTRACE_GETREGS,
                    tid,
                    ptr::null::<u8>(),
                    &mut regs as *mut _,
                );

                #[cfg(target_arch = "x86")]
                let valid = regs.esp != 0;

                #[cfg(target_arch = "x86_64")]
                let valid = regs.rsp != 0;

                if grsuc == -1 || !valid {
                    libc::ptrace(
                        libc::PTRACE_DETACH,
                        tid,
                        ptr::null::<u8>(),
                        ptr::null::<u8>(),
                    );
                    return false;
                }
            }

            true
        }

        for thread in self.threads.as_mut_slice() {
            if let Some(tid) = thread {
                // If the thread either disappeared before we could attach to it, or if
                // it was part of the seccomp sandbox's trusted code, it is OK to
                // silently drop it from the minidump.
                if !suspend_thread(*tid) {
                    *thread = None;
                }
            }
        }

        self.threads_suspended = true;

        if self.threads.iter().any(|f| f.is_some()) {
            Ok(())
        } else {
            Err(Error::NoValidThreads)
        }
    }

    pub fn resume_threads(&mut self) -> Result<(), Error> {
        if !self.threads_suspended {
            return Err(Error::ThreadsNotSuspended);
        }

        let mut all_threads_resumed = true;
        for tid in self.threads.iter().filter_map(|t| *t) {
            all_threads_resumed &= unsafe {
                libc::ptrace(
                    libc::PTRACE_DETACH,
                    tid,
                    std::ptr::null::<u8>(),
                    std::ptr::null::<u8>(),
                ) >= 0
            };
        }

        self.threads_suspended = false;

        if all_threads_resumed {
            Ok(())
        } else {
            Err(Error::AllThreadsNotResumed)
        }
    }

    pub fn late_init(&mut self) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            for mapping in self.mappings.as_mut_slice() {
                // Only consider exec mappings that indicate a file path was
                // mapped, and where the ELF header indicates a mapped shared library.
                if !mapping.has_exec || !mapping.name.as_ref().starts_with('/') {
                    continue;
                }

                compile_error!("implement me");
                // ElfW(Ehdr) ehdr;
                // if (!GetLoadedElfHeader(mapping->start_addr, &ehdr)) {
                //   continue;
                // }
                // if (ehdr.e_type == ET_DYN) {
                //   // Compute the effective load bias for this mapped library, and update
                //   // the mapping to hold that rather than |start_addr|, at the same time
                //   // adjusting |size| to account for the change in |start_addr|. Where
                //   // the library does not contain Android packed relocations,
                //   // GetEffectiveLoadBias() returns |start_addr| and the mapping entry
                //   // is not changed.
                //   const uintptr_t load_bias = GetEffectiveLoadBias(&ehdr,
                //                                                    mapping->start_addr);
                //   mapping->size += mapping->start_addr - load_bias;
                //   mapping->start_addr = load_bias;
                // }
            }
        }

        Ok(())
    }

    pub fn get_thread_info(tid: u32) -> Result<super::ThreadInfo, Error> {
        let mut path = FixedCStr::<32>::new();
        write!(&mut path, "/proc/{}/status", tid)?;

        let mut oo = fs::OpenOptions::new();
        oo.read(true);
        let sfile = fs::open(&path, oo)?;

        let line_reader = utils::LineReader::<_, 512>::new(sfile);

        let mut tgid = None;
        let mut ppid = None;

        for line in line_reader {
            let line = line.as_ref();

            if tgid.is_some() && ppid.is_some() {
                break;
            }

            if let Some(tgids) = line.strip_prefix("Tgid:\t") {
                tgid = tgids.parse::<u32>().ok();
                continue;
            }

            if let Some(ppids) = line.strip_prefix("PPid:\t") {
                ppid = ppids.parse::<u32>().ok();
            }
        }

        let tgid = tgid.ok_or(Error::InvalidStatus)?;
        let ppid = ppid.ok_or(Error::InvalidStatus)?;

        Ok(super::ThreadInfo::new(tid, tgid, ppid)?)
    }

    /// Get information about the stack, given the stack pointer. We don't try to
    /// walk the stack since we might not have all the information needed to do
    /// unwind. So we just grab, up to, 32k of stack.
    pub unsafe fn get_stack_info(&self, stack_pointer: usize) -> Option<&'_ [u8]> {
        // Move the stack pointer to the bottom of the page that it's in.
        let page_size = crate::alloc::get_page_size();
        let stack_ptr = stack_pointer & !(page_size - 1);

        self.mappings.iter().find_map(|mapping| {
            if stack_ptr >= mapping.start_addr && stack_ptr - mapping.start_addr < mapping.size {
                let len = std::cmp::min(mapping.size - stack_ptr - mapping.start_addr, 32 * 1024);

                Some(std::slice::from_raw_parts(stack_ptr as *const u8, len))
            } else {
                None
            }
        })
    }

    pub unsafe fn copy_from_process(&self, child: libc::pid_t, dest: &mut [u8], src: &[u8]) {
        // PTRACE_PEEKDATA works in word sizes
        let mut word = 0usize;
        let word_size = std::mem::size_of::<usize>();

        let mut copied = 0;

        while copied < src.len() {
            let len = if src.len() - copied > word_size {
                word_size
            } else {
                src.len() - copied
            };

            if libc::ptrace(
                libc::PTRACE_PEEKDATA,
                child,
                src.as_ptr().offset(copied as isize),
                &mut word as *mut _,
            ) == -1
            {
                word = 0;
            }

            dest[copied..copied + len].copy_from_slice(&word.to_ne_bytes()[..len]);
            copied += len;
        }
    }

    /// Sanitizes a block of stack memory by overwriting words that are not
    /// pointers with a sentinel value,  `0x0defaced`, to strip potentially
    /// **P**ersonal **I**dentifiable **I**nformation
    pub fn sanitize_stack(&self, stack: &mut [u8], original_stack: usize, offset: usize) {
        cfg_if::cfg_if! {
            if #[cfg(target_pointer_width = "32")] {
                const SENTINEL: usize = 0x0defaced;
            } else if #[cfg(target_pointer_width = "64")] {
                const SENTINEL: usize = 0x0defaced0defaced;
            } else {
                compile_error!("invalid target_pointer_width");
            }
        }

        const TEST_BITS: u32 = 11;
        const ARRAY_SIZE: usize = 1 << (TEST_BITS - 3);
        const ARRAY_MASK: usize = ARRAY_SIZE - 1;
        const SHIFT: u32 = 32 - TEST_BITS;
        const SMALL_INT_MAGNITUDE: isize = 4 * 1024;

        let mut last_hit_mapping: Option<&MappingInfo> = None;
        let stack_mapping = self.find_mapping_no_bias(original_stack);

        let mut could_hit_mapping = [0u8; ARRAY_SIZE];

        for mapping in self.mappings.as_slice() {
            if !mapping.has_exec {
                continue;
            }

            let start = mapping.start_addr >> SHIFT;
            let end = (mapping.start_addr + mapping.size) >> SHIFT;

            for bit in start..=end {
                could_hit_mapping[(bit >> 3) & ARRAY_MASK] |= 1 << (bit & 7);
            }
        }

        // Zero memory that is below the current stack pointer.
        let zero_offset =
            (offset + std::mem::size_of::<usize>() - 1) & !(std::mem::size_of::<usize>() - 1);
        if zero_offset > 0 {
            stack[..zero_offset].fill(0);
        }

        // Apply sanitization to each complete pointer-aligned word in the stack.
        unsafe {
            let mut sp: *mut usize = stack.as_mut_ptr().offset(zero_offset as isize).cast();
            let end: *mut usize = stack
                .as_mut_ptr()
                .offset((stack.len() - std::mem::size_of::<usize>()) as isize)
                .cast();

            while sp <= end {
                let addr = sp.read();

                if addr as isize <= SMALL_INT_MAGNITUDE && addr as isize >= -SMALL_INT_MAGNITUDE {
                    continue;
                }

                if let Some(sm) = stack_mapping {
                    if sm.contains_address(addr) {
                        continue;
                    }
                }

                if let Some(sm) = last_hit_mapping {
                    if sm.contains_address(addr) {
                        continue;
                    }
                }

                let test = addr >> SHIFT;

                if could_hit_mapping[(test >> 3) & ARRAY_MASK] & (1 << (test & 7)) != 0 {
                    if let Some(mapping) = self
                        .find_mapping_no_bias(addr)
                        .filter(|mapping| mapping.has_exec)
                    {
                        last_hit_mapping = Some(mapping);
                        continue;
                    }
                }

                sp.write(SENTINEL);
                sp = sp.offset(1);
            }

            let partial = stack.len() % std::mem::size_of::<usize>();
            if partial > 0 {
                stack[stack.len() - partial..].fill(0);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_maps() {
        {
            let empty = "57942200000-57942300000 rw-p 00000000 00:00 0";

            let empty: MappingInfo = empty.parse().unwrap();
            let start_addr = usize::from_str_radix("57942200000", 16).unwrap();
            let end_addr = usize::from_str_radix("57942300000", 16).unwrap();

            assert_eq!(
                empty,
                MappingInfo {
                    start_addr,
                    size: end_addr - start_addr,
                    sys_start_addr: start_addr,
                    sys_end_addr: end_addr,
                    offset: 0,
                    has_exec: false,
                    name: utils::FixedStr::new(),
                }
            );
        }

        {
            let pthread = "7feca169f000-7feca16a0000 rw-p 0001b000 fd:00 1705088                    /usr/lib64/libpthread-2.33.so";

            let pthread: MappingInfo = pthread.parse().unwrap();
            let start_addr = usize::from_str_radix("7feca169f000", 16).unwrap();
            let end_addr = usize::from_str_radix("7feca16a0000", 16).unwrap();

            let mut name = utils::FixedStr::new();
            name.write_str("/usr/lib64/libpthread-2.33.so").unwrap();

            assert_eq!(
                pthread,
                MappingInfo {
                    start_addr,
                    size: end_addr - start_addr,
                    sys_start_addr: start_addr,
                    sys_end_addr: end_addr,
                    offset: usize::from_str_radix("0001b000", 16).unwrap(),
                    has_exec: false,
                    name,
                }
            );
        }

        {
            let vdso =
                "7fff249fc000-7fff249fe000 r-xp 00000000 00:00 0                          [vdso]";

            let vdso: MappingInfo = vdso.parse().unwrap();
            let start_addr = usize::from_str_radix("7fff249fc000", 16).unwrap();
            let end_addr = usize::from_str_radix("7fff249fe000", 16).unwrap();

            assert_eq!(
                vdso,
                MappingInfo {
                    start_addr,
                    size: end_addr - start_addr,
                    sys_start_addr: start_addr,
                    sys_end_addr: end_addr,
                    offset: 0,
                    has_exec: true,
                    name: utils::FixedStr::new(),
                }
            );
        }
    }
}
