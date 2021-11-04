use super::{
    file_writer::FileWriter,
    ptrace_dumper::{MappingInfo, PTraceDumper},
};
use crate::{
    alloc::{Allocator, PageVec},
    linux::handler::CrashContext,
    minidump::*,
};
use std::{mem, ptr};

#[derive(thiserror::Error, Debug)]
pub enum WriterError {
    #[error(transparent)]
    ProcTrace(#[from] super::ptrace_dumper::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Alloc(#[from] crate::alloc::AllocError),
}

// Writes a minidump to the filesystem. These functions do not malloc nor use
// libc functions which may. Thus, it can be used in contexts where the state
// of the heap may be corrupt.
//   minidump_path: the path to the file to write to. This is opened O_EXCL and
//     fails open fails.
//   crashing_process: the pid of the crashing process. This must be trusted.
//   blob: a blob of data from the crashing process. See exception_handler.h
//   blob_size: the length of |blob|, in bytes
//
// Returns true iff successful.
// bool WriteMinidump(const char* minidump_path, pid_t crashing_process,
//     const void* blob, size_t blob_size,
//     bool skip_stacks_if_mapping_unreferenced = false,
//     uintptr_t principal_mapping_address = 0,
//     bool sanitize_stacks = false);

pub struct MinidumpSettings {
    pub skip_stacks_if_mapping_is_unreferenced: bool,
    pub size_limit: Option<usize>,
    // If true, apply stack sanitization to stored stack data to remove PII
    pub sanitize_stacks: bool,
}

struct MinidumpWriter<'crash> {
    settings: MinidumpSettings,
    dumper: PTraceDumper,
    crash_context: &'crash CrashContext,
    crashing_thread_context: Option<Location>,
    allocator: Allocator,
    memory_blocks: PageVec<MemoryDescriptor>,
    /// Mappings that have been supplied by the user
    user_mappings: Vec<MappingInfo>,
}

impl<'crash> MinidumpWriter<'crash> {
    fn init(&mut self) -> Result<(), WriterError> {
        self.dumper.init()?;
        unsafe { self.dumper.suspend_threads()? };
        self.dumper.late_init()?;

        if self.settings.skip_stacks_if_mapping_is_unreferenced {
            // self.principal_mapping_address = self
            //     .dumper
            //     .find_mapping_without_bias(self.principal_mapping_address);

            // if !self.crashing_thread_references_principal_mapping() {
            //     return Err(Error::PrincipalMappingUnreferenced);
            // }
        }

        Ok(())
    }

    fn dump(mut self, file: &mut std::fs::File) -> Result<(), WriterError> {
        // A minidump file contains a number of tagged streams. This is the
        // number of stream which we write.
        const NUM_STREAMS: u32 = 13;

        let mut fw = super::file_writer::FileWriter::new(file);

        // Ensure the header gets flushed, as that happens in the destructor.
        // If a crash occurs somewhere below, at least the header will be
        // intact.
        {
            let item = fw.reserve::<Header>()?;
            item.write(
                Header {
                    signature: format::MINIDUMP_SIGNATURE,
                    version: format::MINIDUMP_VERSION,
                    time_date_stamp: unsafe {
                        let time = libc::time(ptr::null_mut());
                        time as u32
                    },
                    stream_count: NUM_STREAMS,
                    stream_directory_rva: mem::size_of::<Header>() as u32,
                    checksum: 0,
                    flags: 0,
                },
                &mut fw,
            )?;

            fw.flush()?;
        }

        let dir = fw.reserve_array(NUM_STREAMS as usize)?;
        let mut dir_index = 0;

        dir.write(dir_index, self.write_thread_list(&mut fw)?, &mut fw)?;
        dir_index += 1;

        dir.write(dir_index, self.write_mappings(&mut fw)?, &mut fw)?;
        dir_index += 1;

        Ok(())
    }

    fn write_thread_list(&mut self, fw: &mut FileWriter<'_>) -> Result<Directory, WriterError> {
        let num_threads = self.dumper.threads.iter().filter(|t| t.is_some()).count();

        let tlist = fw.reserve_header_array::<u32, Thread>(num_threads)?;
        tlist.write_header(num_threads as u32, fw)?;

        let dir_ent = Directory {
            stream_type: StreamType::ThreadListStream as u32,
            location: tlist.location(),
        };

        // Number of threads whose stack size we don't want to limit.  These base
        // threads will simply be the first N threads returned by the dumper (although
        // the crashing thread will never be limited).  Threads beyond this count are
        // the extra threads.
        const LIMIT_BASE_THREAD_COUNT: usize = 20;

        // If the minidump's total output size is being limited, we try and stay
        // within that limit by reducing the amount of stack data written for "extra"
        // threads beyond the first "base" threads. The crashing thread is never limited.
        let extra_thread_stack_len = self.settings.size_limit.and_then(|md_size_limit| {
            // Estimate for how big each thread's stack will be (in bytes).
            const LIMIT_AVG_STACK_LEN: usize = 8 * 1024;
            // Make sure this number of additional bytes can fit in the minidump
            // (exclude the stack data).
            const FUDGE_FACTOR: usize = 64 * 1024;
            // Maximum stack size to dump for any extra thread (in bytes).
            const MAX_EXTRA_THREAD_STACK: usize = 2 * 1024;

            let estimated_total_stack_size = num_threads * num_threads;
            let estimated_minidump_size =
                fw.position() as usize + estimated_total_stack_size + FUDGE_FACTOR;

            if estimated_minidump_size > md_size_limit {
                Some(MAX_EXTRA_THREAD_STACK)
            } else {
                None
            }
        });

        for (counter, thread_id) in self
            .dumper
            .threads
            .iter()
            .filter_map(|tid| *tid)
            .enumerate()
        {
            // If this is the crashing thread, we need to gather the thread
            // information from the crash context, as otherwise it will just
            // point to our signal handler
            let thread = match &self.crash_context.context {
                Some(uctx)
                    if thread_id == self.crash_context.tid as u32
                        && !self.dumper.is_post_mortem() =>
                {
                    let thread_info = PTraceDumper::get_thread_info(thread_id)?;

                    // We never limit the stack size on the crashing thread since it is the most important one to keep
                    // as much context as we can
                    let mut md_thread =
                        unsafe { self.fill_thread_stack(fw, thread_id, &thread_info, None)? };

                    // Keep 256 bytes of context around the crashing IP
                    const IP_MEM_SIZE: isize = 256;

                    let ip = uctx.instruction_pointer();

                    if let Some(mapping) = self.dumper.find_mapping_no_bias(ip) {
                        let (ip_start, ip_size) = {
                            let start = std::cmp::max(
                                mapping.start_addr as isize,
                                ip as isize - IP_MEM_SIZE / 2,
                            ) as usize;

                            (
                                start,
                                std::cmp::min(
                                    (mapping.start_addr + mapping.size) as isize,
                                    ip as isize + IP_MEM_SIZE / 2,
                                ) as usize
                                    - start,
                            )
                        };

                        let mut ctx_copy = self.alloc_raw(ip_size)?;
                        unsafe {
                            let src = std::slice::from_raw_parts(ip_start as *const u8, ip_size);

                            self.dumper.copy_from_process(
                                thread_id as libc::pid_t,
                                ctx_copy.as_mut(),
                                src,
                            );
                        }

                        let ip_memory = fw.reserve_raw(ip_size as u64)?;
                        fw.write(ip_memory, 0, unsafe { ctx_copy.as_ref() })?;

                        self.memory_blocks.push(MemoryDescriptor {
                            start_of_memory_range: ip_start as u64,
                            memory: ip_memory.into(),
                        });
                    }

                    let md_cpu_ctx = fw.reserve::<super::thread_info::RawContextCpu>()?;
                    md_cpu_ctx.write(
                        self.crash_context.get_cpu_context().expect(
                            "this is infallible, I should make it that way in the type system",
                        ),
                        fw,
                    )?;

                    md_thread.thread_context = md_cpu_ctx.location();
                    self.crashing_thread_context = Some(md_cpu_ctx.location());

                    md_thread
                }
                _ => {
                    let thread_info = PTraceDumper::get_thread_info(thread_id)?;

                    let stack_size_limit =
                        extra_thread_stack_len.filter(|_size| counter >= LIMIT_BASE_THREAD_COUNT);
                    let mut md_thread = unsafe {
                        self.fill_thread_stack(fw, thread_id, &thread_info, stack_size_limit)?
                    };

                    // If the thread stack data was actually filled out, add it to the memory blocks to emit at the end
                    if md_thread.stack.memory.data_size > 0 {
                        self.memory_blocks.push(md_thread.stack);
                    }

                    let md_cpu_ctx = fw.reserve::<super::thread_info::RawContextCpu>()?;
                    md_cpu_ctx.write(thread_info.get_cpu_context(), fw)?;

                    md_thread.thread_context = md_cpu_ctx.location();

                    if thread_id == self.crash_context.tid as u32 {
                        self.crashing_thread_context = Some(md_cpu_ctx.location());

                        if !self.dumper.is_post_mortem() {
                            //self.dumper.set_crash_address(thread_info.get_ip());
                        }
                    }

                    md_thread
                }
            };

            tlist.write(counter, thread, fw)?;
        }

        Ok(dir_ent)
    }

    unsafe fn fill_thread_stack(
        &self,
        fw: &mut FileWriter<'_>,
        thread_id: u32,
        thread_info: &crate::linux::ThreadInfo,
        max_stack_len: Option<usize>,
    ) -> Result<Thread, WriterError> {
        let mut thread: Thread = std::mem::zeroed();

        thread.stack.start_of_memory_range = thread_info.stack_pointer as u64;
        thread.stack.memory.data_size = 0;
        thread.stack.memory.rva = fw.position() as u32;

        if let Some(mut stack) = self.dumper.get_stack_info(thread_info.stack_pointer) {
            // Shorten the stack if the user has set a max length
            if let Some(max_len) = max_stack_len {
                if stack.len() > max_len {
                    let mut stack_ptr = stack.as_ptr();
                    loop {
                        let chunk_ptr = stack_ptr.offset(max_len as isize);

                        if (chunk_ptr as usize) >= thread_info.stack_pointer {
                            break;
                        }

                        stack_ptr = chunk_ptr;
                    }

                    stack = std::slice::from_raw_parts(stack_ptr, max_len);
                }
            }

            let mut stack_copy = self.alloc_raw(stack.len())?;

            self.dumper
                .copy_from_process(thread_id as libc::pid_t, stack_copy.as_mut(), stack);

            let stack_pointer_offset = thread_info.stack_pointer - stack.as_ptr() as usize;

            if self.settings.skip_stacks_if_mapping_is_unreferenced {
                // TODO: Skip if unreferenced
            }

            if self.settings.sanitize_stacks {
                self.dumper.sanitize_stack(
                    stack_copy.as_mut(),
                    stack.as_ptr() as usize,
                    stack_pointer_offset,
                );
            }

            let memory_res = fw.reserve_raw(stack_copy.as_ref().len() as u64)?;
            fw.write(memory_res, 0, stack_copy.as_ref())?;

            thread.stack.start_of_memory_range = stack_copy.as_ref().as_ptr() as u64;
            thread.stack.memory = memory_res.into();
        }

        Ok(thread)
    }

    fn write_mappings(&self, fw: &mut FileWriter<'_>) -> Result<Directory, WriterError> {
        unimplemented!()
        // let should_include = |mapping: &MappingInfo| {
        //     // we only want modules with filenames
        //     !mapping.name.as_ref().is_empty() &&
        //     // We only want one mapping per shared lib
        //     mapping.offset == 0 &&
        //     // The mapping should be executable
        //     mapping.has_exec &&
        //     // Ensure it's a minimum size that we can actually get signatures for it
        //     mapping.size >= 4 * 1024
        // };

        // // Ignore mappings that are wholly contained within a mapping supplied
        // // by the user
        // let overlaps = |mapping: &MappingInfo| {
        //     self.user_mappings.iter().any(|um| {
        //         mapping.start_addr >= um.start_addr
        //             && mapping.start_addr + mapping.size <= um.start_addr + um.size
        //     })
        // };

        // let num_mappings = self.user_mappings.len()
        //     + self
        //         .dumper
        //         .mappings
        //         .iter()
        //         .filter(|mapping| should_include(mapping) && !overlaps(mapping))
        //         .count();

        // let md_module_list = fw.reserve_header_array::<u32, Module>(num_mappings)?;

        // let dir_ent = Directory {
        //     stream_type: StreamType::ModuleListStream as u32,
        //     location: md_module_list.location(),
        // };

        // for (i, mapping) in self
        //     .dumper
        //     .mappings
        //     .iter()
        //     .filter(|mapping| should_include(mapping) && !overlaps(mapping))
        //     .chain(self.user_mappings.iter())
        //     .enumerate()
        // {}

        // Ok(md_module_list)
    }

    fn fill_module(
        &self,
        mapping: &MappingInfo,
        identifier: Option<&[u8]>,
    ) -> Result<Module, WriterError> {
        unimplemented!()
    }

    #[inline]
    fn alloc_raw(&self, size: usize) -> Result<std::ptr::NonNull<[u8]>, WriterError> {
        use crate::alloc::AllocRef;
        self.allocator
            .alloc(std::alloc::Layout::array::<u8>(size).unwrap())
            .map_err(WriterError::Alloc)
    }
}

impl<'crash> Drop for MinidumpWriter<'crash> {
    fn drop(&mut self) {
        self.dumper.resume_threads().ok();
    }
}

pub(crate) fn write_minidump(
    output: &crate::minidump::MinidumpOutput,
    pid: libc::pid_t,
    context: &CrashContext,
) -> Result<(), WriterError> {
    unimplemented!()
    // let pid = if pid <= 0 {
    //     return Err(Error::InvalidArgs);
    // } else {
    //     std::num::NonZeroU32::new(pid as u32).unwrap()
    // };

    // let allocator = Allocator::new();

    // let ptd = PTraceDumper::new(allocator.clone(), pid, context);

    // let mut mdw = MinidumpWriter {
    //     settings: MinidumpSettings {
    //         skip_stacks_if_mapping_is_unreferenced: false,
    //     },
    //     dumper: ptd,
    //     context,
    //     memory_blocks: PageVec::new_in(allocator.clone()),
    //     allocator,
    // };

    // mdw.init()?;

    //     LinuxPtraceDumper dumper(crashing_process);
    //   const ExceptionHandler::CrashContext* context = NULL;
    //   if (blob) {
    //     if (blob_size != sizeof(ExceptionHandler::CrashContext))
    //       return false;
    //     context = reinterpret_cast<const ExceptionHandler::CrashContext*>(blob);
    //     dumper.SetCrashInfoFromSigInfo(context->siginfo);
    //     dumper.set_crash_thread(context->tid);
    //   }
    //   MinidumpWriter writer(minidump_path, minidump_fd, context, mappings,
    //                         appmem, skip_stacks_if_mapping_unreferenced,
    //                         principal_mapping_address, sanitize_stacks, &dumper);
    //   // Set desired limit for file size of minidump (-1 means no limit).
    //   writer.set_minidump_size_limit(minidump_size_limit);
    //   if (!writer.Init())
    //     return false;
    //   return writer.Dump();
}
