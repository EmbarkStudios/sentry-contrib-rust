fn add_sources(build: &mut cc::Build, root: &str, files: &[&str]) {
    let root = std::path::Path::new(root);
    build.files(files.iter().map(|src| {
        let mut p = root.join(src);
        p.set_extension("cc");
        p
    }));

    build.include(root);
}

fn main() {
    // Breakpad doesn't compile out of the box when targetting musl, better to
    // just convert it Rust
    if let Ok(env) = std::env::var("CARGO_CFG_TARGET_ENV") {
        if env == "musl" {
            panic!("musl is unfortunately not supported right now");
        }
    }

    let mut build = cc::Build::new();

    build
        .cpp(true)
        .warnings(false)
        .flag_if_supported("-std=c++11")
        .flag_if_supported("-fpermissive")
        .include(".")
        .include("breakpad/src")
        .define("BPLOG_MINIMUM_SEVERITY", "SEVERITY_ERROR")
        .define(
            "BPLOG(severity)",
            "1 ? (void)0 : google_breakpad::LogMessageVoidify() & (BPLOG_ERROR)",
        );

    // Our file that implements a small C API that we can easily bind to
    build.file("src/impl.cpp");

    add_sources(
        &mut build,
        "breakpad/src/common",
        &["convert_UTF.cc", "string_conversion.cc"],
    );

    match std::env::var("CARGO_CFG_TARGET_OS")
        .expect("TARGET_OS not specified")
        .as_str()
    {
        "linux" | "android" => {
            build.define("TARGET_OS_LINUX", None).include("lss");

            add_sources(
                &mut build,
                "breakpad/src/client",
                &["minidump_file_writer.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/common/linux",
                &[
                    "elfutils.cc",
                    "file_id.cc",
                    "guid_creator.cc",
                    "linux_libc_support.cc",
                    "memory_mapped_file.cc",
                    "safe_readlink.cc",
                ],
            );

            add_sources(&mut build, "breakpad/src/client/linux/log", &["log.cc"]);

            add_sources(
                &mut build,
                "breakpad/src/client/linux/handler",
                &["exception_handler.cc", "minidump_descriptor.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/linux/crash_generation",
                &["crash_generation_client.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/linux/microdump_writer",
                &["microdump_writer.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/linux/minidump_writer",
                &[
                    "linux_dumper.cc",
                    "linux_ptrace_dumper.cc",
                    "minidump_writer.cc",
                ],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/linux/dump_writer_common",
                &["thread_info.cc", "ucontext_reader.cc"],
            );
        }
        "windows" => {
            build
                .define("TARGET_OS_WINDOWS", None)
                .define("UNICODE", None);

            add_sources(
                &mut build,
                "breakpad/src/common/windows",
                &["guid_string.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/windows/crash_generation",
                &["crash_generation_client.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/windows/handler",
                &["exception_handler.cc"],
            );
        }
        "macos" => {
            build.define("TARGET_OS_MAC", None);

            add_sources(
                &mut build,
                "breakpad/src/client",
                &["minidump_file_writer.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/common/mac",
                &["file_id.cc", "macho_id.cc", "MachIPC.mm"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/mac/crash_generation",
                &["crash_generation_client.cc"],
            );

            add_sources(
                &mut build,
                "breakpad/src/client/mac/handler",
                &[
                    "breakpad_nlist_64.cc",
                    "dynamic_images.cc",
                    "exception_handler.cc",
                    "minidump_generator.cc",
                    "protected_memory_allocator.cc",
                ],
            );
        }
        unsupported => unimplemented!("{} is not a supported target", unsupported),
    }

    build.compile("breakpad");
}
