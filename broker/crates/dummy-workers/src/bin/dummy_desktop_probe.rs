//! Prints the name of the desktop this process was started on, so a test can
//! observe from inside the worker which desktop the broker put it on
//! (issue #1194: every dedicated worker must land on one shared desktop).

use std::mem::size_of;
use windows_sys::Win32::System::StationsAndDesktops::{
    GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

fn main() {
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    if desktop.is_null() {
        eprintln!("no thread desktop");
        std::process::exit(1);
    }
    let mut required_bytes = 0u32;
    unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            std::ptr::null_mut(),
            0,
            &mut required_bytes,
        );
    }
    let mut name = vec![0u16; (required_bytes as usize).div_ceil(size_of::<u16>()).max(1)];
    if unsafe {
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * size_of::<u16>()) as u32,
            &mut required_bytes,
        )
    } == 0
    {
        eprintln!("desktop name query failed");
        std::process::exit(1);
    }
    let end = name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(name.len());
    println!("{}", String::from_utf16_lossy(&name[..end]));
}
