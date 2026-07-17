#[cfg(windows)]
fn main() {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;

    const WRITE_DAC: u32 = 0x0004_0000;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn SetThreadToken(thread: *mut c_void, token: *mut c_void) -> i32;
    }

    let mut args = std::env::args_os().skip(1);
    let root = std::path::PathBuf::from(args.next().expect("sealed root"));
    let token = args
        .next()
        .expect("restricted token")
        .to_string_lossy()
        .parse::<usize>()
        .expect("restricted token handle") as *mut c_void;
    if unsafe { SetThreadToken(std::ptr::null_mut(), token) } == 0 {
        std::process::exit(1 << 6);
    }
    let payload = root.join("payload.bin");
    let mut failures = 0u32;

    if !matches!(std::fs::read(&payload), Ok(bytes) if bytes == b"sealed fixture") {
        failures |= 1 << 0;
    }
    if OpenOptions::new()
        .write(true)
        .open(&payload)
        .and_then(|mut file| file.write_all(b"mutated"))
        .is_ok()
    {
        failures |= 1 << 1;
    }
    if OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("created.bin"))
        .is_ok()
    {
        failures |= 1 << 2;
    }
    if std::fs::remove_file(&payload).is_ok() {
        failures |= 1 << 3;
    }
    if OpenOptions::new()
        .access_mode(WRITE_DAC)
        .open(&root)
        .is_ok()
    {
        failures |= 1 << 4;
    }
    if OpenOptions::new()
        .access_mode(WRITE_DAC)
        .open(&payload)
        .is_ok()
    {
        failures |= 1 << 5;
    }

    std::process::exit(failures as i32);
}

#[cfg(not(windows))]
fn main() {}
