#[cfg(windows)]
fn main() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        RaiseException, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };
    unsafe {
        SetErrorMode(SEM_NOGPFAULTERRORBOX);
        RaiseException(0xC000_0005, 1, 0, core::ptr::null());
    }
}

#[cfg(not(windows))]
fn main() {
    std::process::abort();
}
