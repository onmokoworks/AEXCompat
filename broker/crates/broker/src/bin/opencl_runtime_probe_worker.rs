use aexcompat_broker::opencl_runtime_probe::{
    AggregateLoaderObservation, ApiFailure, OpenClApi, collect_with_api,
    missing_symbol_observation, no_loader_observation,
};

fn main() {
    let observation = platform::probe();
    match serde_json::to_string(&observation) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("OpenCL probe serialization failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn probe() -> AggregateLoaderObservation {
        no_loader_observation()
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::mem::{size_of, transmute_copy};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    const CL_SUCCESS: i32 = 0;
    const CL_DEVICE_NOT_FOUND: i32 = -1;
    const CL_PLATFORM_NOT_FOUND_KHR: i32 = -1001;
    const CL_DEVICE_TYPE_ALL: u64 = 0xffff_ffff;

    type ClPlatformId = *mut c_void;
    type ClDeviceId = *mut c_void;
    type ClGetPlatformIDs = unsafe extern "system" fn(u32, *mut ClPlatformId, *mut u32) -> i32;
    type ClGetPlatformInfo =
        unsafe extern "system" fn(ClPlatformId, u32, usize, *mut c_void, *mut usize) -> i32;
    type ClGetDeviceIDs =
        unsafe extern "system" fn(ClPlatformId, u64, u32, *mut ClDeviceId, *mut u32) -> i32;
    type ClGetDeviceInfo =
        unsafe extern "system" fn(ClDeviceId, u32, usize, *mut c_void, *mut usize) -> i32;

    struct Library(HMODULE);

    impl Drop for Library {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }

    struct DynamicOpenCl {
        _library: Library,
        get_platform_ids: ClGetPlatformIDs,
        get_platform_info: ClGetPlatformInfo,
        get_device_ids: ClGetDeviceIDs,
        get_device_info: ClGetDeviceInfo,
    }

    pub fn probe() -> AggregateLoaderObservation {
        let library = match load_system_opencl() {
            Some(library) => library,
            None => return no_loader_observation(),
        };
        let mut missing = Vec::new();
        let get_platform_ids = symbol::<ClGetPlatformIDs>(
            library.0,
            b"clGetPlatformIDs\0",
            "clGetPlatformIDs",
            &mut missing,
        );
        let get_platform_info = symbol::<ClGetPlatformInfo>(
            library.0,
            b"clGetPlatformInfo\0",
            "clGetPlatformInfo",
            &mut missing,
        );
        let get_device_ids = symbol::<ClGetDeviceIDs>(
            library.0,
            b"clGetDeviceIDs\0",
            "clGetDeviceIDs",
            &mut missing,
        );
        let get_device_info = symbol::<ClGetDeviceInfo>(
            library.0,
            b"clGetDeviceInfo\0",
            "clGetDeviceInfo",
            &mut missing,
        );
        if !missing.is_empty() {
            return missing_symbol_observation(&missing);
        }
        let api = DynamicOpenCl {
            _library: library,
            get_platform_ids: get_platform_ids.expect("checked symbol"),
            get_platform_info: get_platform_info.expect("checked symbol"),
            get_device_ids: get_device_ids.expect("checked symbol"),
            get_device_info: get_device_info.expect("checked symbol"),
        };
        collect_with_api(&api)
    }

    fn load_system_opencl() -> Option<Library> {
        let name = "OpenCL.dll\0".encode_utf16().collect::<Vec<_>>();
        let handle =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        (!handle.is_null()).then_some(Library(handle))
    }

    fn symbol<T: Copy>(
        library: HMODULE,
        name: &[u8],
        label: &'static str,
        missing: &mut Vec<&'static str>,
    ) -> Option<T> {
        let function = unsafe { GetProcAddress(library, name.as_ptr()) };
        match function {
            Some(function) => {
                assert_eq!(size_of::<T>(), size_of_val(&function));
                Some(unsafe { transmute_copy_function(function) })
            }
            None => {
                missing.push(label);
                None
            }
        }
    }

    unsafe fn transmute_copy_function<T: Copy>(
        function: unsafe extern "system" fn() -> isize,
    ) -> T {
        unsafe { transmute_copy(&function) }
    }

    impl OpenClApi for DynamicOpenCl {
        fn platform_ids(&self, limit: usize) -> Result<Vec<usize>, ApiFailure> {
            let mut count = 0u32;
            let status = unsafe { (self.get_platform_ids)(0, null_mut(), &mut count) };
            if status == CL_PLATFORM_NOT_FOUND_KHR {
                return Ok(Vec::new());
            }
            if status != CL_SUCCESS {
                return Err(ApiFailure::api("clGetPlatformIDs(count)", status));
            }
            let count = count as usize;
            if count > limit {
                return Err(ApiFailure::overflow("clGetPlatformIDs(count)"));
            }
            if count == 0 {
                return Ok(Vec::new());
            }
            let mut values = vec![null_mut(); count];
            let mut returned = count as u32;
            let status = unsafe {
                (self.get_platform_ids)(count as u32, values.as_mut_ptr(), &mut returned)
            };
            if status != CL_SUCCESS {
                return Err(ApiFailure::api("clGetPlatformIDs(values)", status));
            }
            if returned as usize > count || values.iter().any(|value| value.is_null()) {
                return Err(ApiFailure::malformed("clGetPlatformIDs(values)"));
            }
            values.truncate(returned as usize);
            Ok(values.into_iter().map(|value| value as usize).collect())
        }

        fn platform_string(
            &self,
            platform: usize,
            parameter: u32,
            limit: usize,
        ) -> Result<String, ApiFailure> {
            query_string(
                |size, value, returned| unsafe {
                    (self.get_platform_info)(
                        platform as ClPlatformId,
                        parameter,
                        size,
                        value,
                        returned,
                    )
                },
                "clGetPlatformInfo",
                limit,
            )
        }

        fn device_ids(&self, platform: usize, limit: usize) -> Result<Vec<usize>, ApiFailure> {
            let mut count = 0u32;
            let status = unsafe {
                (self.get_device_ids)(
                    platform as ClPlatformId,
                    CL_DEVICE_TYPE_ALL,
                    0,
                    null_mut(),
                    &mut count,
                )
            };
            if status == CL_DEVICE_NOT_FOUND {
                return Ok(Vec::new());
            }
            if status != CL_SUCCESS {
                return Err(ApiFailure::api("clGetDeviceIDs(count)", status));
            }
            let count = count as usize;
            if count > limit {
                return Err(ApiFailure::overflow("clGetDeviceIDs(count)"));
            }
            if count == 0 {
                return Ok(Vec::new());
            }
            let mut values = vec![null_mut(); count];
            let mut returned = count as u32;
            let status = unsafe {
                (self.get_device_ids)(
                    platform as ClPlatformId,
                    CL_DEVICE_TYPE_ALL,
                    count as u32,
                    values.as_mut_ptr(),
                    &mut returned,
                )
            };
            if status != CL_SUCCESS {
                return Err(ApiFailure::api("clGetDeviceIDs(values)", status));
            }
            if returned as usize > count || values.iter().any(|value| value.is_null()) {
                return Err(ApiFailure::malformed("clGetDeviceIDs(values)"));
            }
            values.truncate(returned as usize);
            Ok(values.into_iter().map(|value| value as usize).collect())
        }

        fn device_string(
            &self,
            device: usize,
            parameter: u32,
            limit: usize,
        ) -> Result<String, ApiFailure> {
            query_string(
                |size, value, returned| unsafe {
                    (self.get_device_info)(device as ClDeviceId, parameter, size, value, returned)
                },
                "clGetDeviceInfo(string)",
                limit,
            )
        }

        fn device_u32(&self, device: usize, parameter: u32) -> Result<u32, ApiFailure> {
            query_scalar(
                self.get_device_info,
                device,
                parameter,
                "clGetDeviceInfo(u32)",
            )
        }

        fn device_u64(&self, device: usize, parameter: u32) -> Result<u64, ApiFailure> {
            query_scalar(
                self.get_device_info,
                device,
                parameter,
                "clGetDeviceInfo(u64)",
            )
        }

        fn device_usize(&self, device: usize, parameter: u32) -> Result<usize, ApiFailure> {
            query_scalar(
                self.get_device_info,
                device,
                parameter,
                "clGetDeviceInfo(size_t)",
            )
        }

        fn device_usizes(
            &self,
            device: usize,
            parameter: u32,
            count: usize,
        ) -> Result<Vec<usize>, ApiFailure> {
            if count == 0
                || count > aexcompat_broker::opencl_runtime_probe::MAX_WORK_ITEM_DIMENSIONS
            {
                return Err(ApiFailure::overflow("clGetDeviceInfo(size_t[])"));
            }
            let byte_count = count
                .checked_mul(size_of::<usize>())
                .ok_or_else(|| ApiFailure::overflow("clGetDeviceInfo(size_t[])"))?;
            let mut values = vec![0usize; count];
            let mut returned = 0usize;
            let status = unsafe {
                (self.get_device_info)(
                    device as ClDeviceId,
                    parameter,
                    byte_count,
                    values.as_mut_ptr().cast(),
                    &mut returned,
                )
            };
            if status != CL_SUCCESS {
                return Err(ApiFailure::api("clGetDeviceInfo(size_t[])", status));
            }
            if returned != byte_count {
                return Err(ApiFailure::malformed("clGetDeviceInfo(size_t[])"));
            }
            Ok(values)
        }
    }

    fn query_string(
        call: impl Fn(usize, *mut c_void, *mut usize) -> i32,
        operation: &'static str,
        limit: usize,
    ) -> Result<String, ApiFailure> {
        let mut size = 0usize;
        let status = call(0, null_mut(), &mut size);
        if status != CL_SUCCESS {
            return Err(ApiFailure::api(format!("{operation}(size)"), status));
        }
        if size == 0 || size > limit {
            return Err(ApiFailure::overflow(format!("{operation}(size)")));
        }
        let mut bytes = vec![0u8; size];
        let mut returned = size;
        let status = call(size, bytes.as_mut_ptr().cast(), &mut returned);
        if status != CL_SUCCESS {
            return Err(ApiFailure::api(format!("{operation}(value)"), status));
        }
        if returned != size || bytes.last() != Some(&0) || bytes[..size - 1].contains(&0) {
            return Err(ApiFailure::malformed(format!("{operation}(value)")));
        }
        String::from_utf8(bytes[..size - 1].to_vec())
            .map_err(|_| ApiFailure::malformed(format!("{operation}(utf8)")))
    }

    fn query_scalar<T: Copy + Default>(
        function: ClGetDeviceInfo,
        device: usize,
        parameter: u32,
        operation: &'static str,
    ) -> Result<T, ApiFailure> {
        let mut value = T::default();
        let mut returned = 0usize;
        let status = unsafe {
            function(
                device as ClDeviceId,
                parameter,
                size_of::<T>(),
                (&mut value as *mut T).cast(),
                &mut returned,
            )
        };
        if status != CL_SUCCESS {
            return Err(ApiFailure::api(operation, status));
        }
        if returned != size_of::<T>() {
            return Err(ApiFailure::malformed(operation));
        }
        Ok(value)
    }
}
