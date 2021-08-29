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

pub(crate) fn write_minidump(
    output: &crate::minidump::MinidumpOutput,
    pid: libc::pid_t,
    context: &super::handler::CrashContext,
) {
}
