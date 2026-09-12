//! Current host account lookup. No guest pointer crosses this FFI boundary.
#[cfg(unix)]
pub fn current_username() -> Result<Vec<u16>, String> {
    let uid = unsafe { libc::geteuid() };
    let mut capacity = 1024;
    loop {
        let mut buffer = vec![0u8; capacity];
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && capacity < 1024 * 1024 {
            capacity *= 2;
            continue;
        }
        if status != 0 {
            return Err(format!("host account lookup failed: {status}"));
        }
        if result.is_null() {
            return Err("host effective account was not found".into());
        }
        let record = unsafe { record.assume_init() };
        if record.pw_name.is_null() {
            return Err("host account has no name".into());
        }
        let name = unsafe { std::ffi::CStr::from_ptr(record.pw_name) }
            .to_str()
            .map_err(|_| "host account name is not UTF-8")?;
        return Ok(name.encode_utf16().collect());
    }
}

#[cfg(windows)]
pub fn current_username() -> Result<Vec<u16>, String> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetUserNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    let mut name = vec![0u16; 257];
    let mut size = name.len() as u32;
    if unsafe { GetUserNameW(name.as_mut_ptr(), &mut size) } == 0 {
        return Err(format!(
            "host account lookup failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if size == 0 || size as usize > name.len() || name[size as usize - 1] != 0 {
        return Err("host account lookup returned an invalid length".into());
    }
    name.truncate(size as usize - 1);
    Ok(name)
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn account_lookup_matches_effective_user_command() {
        let output = std::process::Command::new("/usr/bin/id")
            .args(["-un"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let expected = String::from_utf8(output.stdout).unwrap();
        let expected = expected.trim_end_matches('\n');
        assert_eq!(
            String::from_utf16(&super::current_username().unwrap()).unwrap(),
            expected
        );
    }
}
