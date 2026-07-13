fn main() {
    #[cfg(windows)]
    if let Some(raw) = std::env::args()
        .skip_while(|arg| arg != "--sentinel-handle")
        .nth(1)
    {
        use windows_sys::Win32::System::Threading::SetEvent;
        let handle =
            raw.parse::<usize>().expect("numeric synthetic sentinel") as *mut core::ffi::c_void;
        // A numeric handle can be reused for an unrelated child handle. SetEvent
        // succeeds only when the inherited object is the synthetic event itself.
        let inherited = unsafe { SetEvent(handle) } != 0;
        println!("sentinel_inherited={inherited}");
    }
    println!("dummy worker completed");
}
