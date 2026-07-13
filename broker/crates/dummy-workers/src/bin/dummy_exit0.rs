fn main() {
    #[cfg(windows)]
    if let Some(raw) = std::env::args()
        .skip_while(|arg| arg != "--sentinel-handle")
        .nth(1)
    {
        use windows_sys::Win32::Foundation::GetHandleInformation;
        let handle =
            raw.parse::<usize>().expect("numeric synthetic sentinel") as *mut core::ffi::c_void;
        let mut flags = 0;
        let inherited = unsafe { GetHandleInformation(handle, &mut flags) } != 0;
        println!("sentinel_inherited={inherited}");
    }
    println!("dummy worker completed");
}
