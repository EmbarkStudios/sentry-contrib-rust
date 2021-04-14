fn main() {
    let cur_dir = std::env::current_dir().unwrap();

    let _handler = breakpad_handler::BreakpadHandler::attach(
        cur_dir,
        breakpad_handler::InstallOptions::BothHandlers,
        Box::new(|minidump_path: std::path::PathBuf| {
            println!("Minidump written to {}", minidump_path.display());

            match std::fs::remove_file(&minidump_path) {
                Ok(_) => {
                    println!("Removed {}", minidump_path.display());
                }
                Err(e) => {
                    println!("Failed to remove {}: {}", minidump_path.display(), e);
                }
            }
        }),
    )
    .unwrap();

    unsafe {
        if std::env::args().any(|a| a == "--crash") {
            let ptr: *mut u8 = std::ptr::null_mut();
            *ptr = 42;
        }
    }
}
