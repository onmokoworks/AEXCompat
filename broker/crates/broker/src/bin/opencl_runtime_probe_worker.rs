use aexcompat_broker::opencl_runtime_probe::{
    AggregateLoaderObservation, ApiFailure, BuildLogObservation, BuildLogStatus,
    COMPUTE_ELEMENT_COUNT, ComputeDeviceObservation, ComputeDeviceProbe, ComputeStage,
    MAX_BUILD_LOG_BYTES, OpenClApi, QueueApi, collect_with_compute, missing_symbol_observation,
    no_loader_observation,
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
    use sha2::{Digest, Sha256};
    use std::ffi::c_void;
    use std::mem::{size_of, transmute_copy};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    const CL_SUCCESS: i32 = 0;
    const CL_DEVICE_NOT_FOUND: i32 = -1;
    const CL_PLATFORM_NOT_FOUND_KHR: i32 = -1001;
    const CL_DEVICE_TYPE_ALL: u64 = 0xffff_ffff;
    const CL_MEM_WRITE_ONLY: u64 = 1 << 1;
    const CL_MEM_READ_ONLY: u64 = 1 << 2;
    const CL_TRUE: u32 = 1;
    const CL_PROGRAM_BUILD_LOG: u32 = 0x1183;
    const CL_CONTEXT_PLATFORM: isize = 0x1084;

    type ClPlatformId = *mut c_void;
    type ClDeviceId = *mut c_void;
    type ClContext = *mut c_void;
    type ClCommandQueue = *mut c_void;
    type ClMem = *mut c_void;
    type ClProgram = *mut c_void;
    type ClKernel = *mut c_void;
    type ClEvent = *mut c_void;
    const _: () = assert!(size_of::<isize>() == size_of::<ClPlatformId>());
    type ClGetPlatformIDs = unsafe extern "system" fn(u32, *mut ClPlatformId, *mut u32) -> i32;
    type ClGetPlatformInfo =
        unsafe extern "system" fn(ClPlatformId, u32, usize, *mut c_void, *mut usize) -> i32;
    type ClGetDeviceIDs =
        unsafe extern "system" fn(ClPlatformId, u64, u32, *mut ClDeviceId, *mut u32) -> i32;
    type ClGetDeviceInfo =
        unsafe extern "system" fn(ClDeviceId, u32, usize, *mut c_void, *mut usize) -> i32;
    type ClCreateContext = unsafe extern "system" fn(
        *const isize,
        u32,
        *const ClDeviceId,
        Option<unsafe extern "system" fn(*const i8, *const c_void, usize, *mut c_void)>,
        *mut c_void,
        *mut i32,
    ) -> ClContext;
    type ClCreateCommandQueueWithProperties =
        unsafe extern "system" fn(ClContext, ClDeviceId, *const isize, *mut i32) -> ClCommandQueue;
    type ClCreateCommandQueue =
        unsafe extern "system" fn(ClContext, ClDeviceId, u64, *mut i32) -> ClCommandQueue;
    type ClCreateBuffer =
        unsafe extern "system" fn(ClContext, u64, usize, *mut c_void, *mut i32) -> ClMem;
    type ClEnqueueWriteBuffer = unsafe extern "system" fn(
        ClCommandQueue,
        ClMem,
        u32,
        usize,
        usize,
        *const c_void,
        u32,
        *const ClEvent,
        *mut ClEvent,
    ) -> i32;
    type ClCreateProgramWithSource = unsafe extern "system" fn(
        ClContext,
        u32,
        *const *const i8,
        *const usize,
        *mut i32,
    ) -> ClProgram;
    type ClBuildProgram = unsafe extern "system" fn(
        ClProgram,
        u32,
        *const ClDeviceId,
        *const i8,
        Option<unsafe extern "system" fn(ClProgram, *mut c_void)>,
        *mut c_void,
    ) -> i32;
    type ClGetProgramBuildInfo = unsafe extern "system" fn(
        ClProgram,
        ClDeviceId,
        u32,
        usize,
        *mut c_void,
        *mut usize,
    ) -> i32;
    type ClCreateKernel = unsafe extern "system" fn(ClProgram, *const i8, *mut i32) -> ClKernel;
    type ClSetKernelArg = unsafe extern "system" fn(ClKernel, u32, usize, *const c_void) -> i32;
    type ClEnqueueNDRangeKernel = unsafe extern "system" fn(
        ClCommandQueue,
        ClKernel,
        u32,
        *const usize,
        *const usize,
        *const usize,
        u32,
        *const ClEvent,
        *mut ClEvent,
    ) -> i32;
    type ClFinish = unsafe extern "system" fn(ClCommandQueue) -> i32;
    type ClEnqueueReadBuffer = ClEnqueueWriteBuffer;
    type ClRelease = unsafe extern "system" fn(*mut c_void) -> i32;

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
        compute: Option<ComputeFunctions>,
        missing_compute_symbols: Vec<String>,
    }

    #[derive(Clone, Copy)]
    struct ComputeFunctions {
        create_context: ClCreateContext,
        create_queue_with_properties: Option<ClCreateCommandQueueWithProperties>,
        create_queue: Option<ClCreateCommandQueue>,
        create_buffer: ClCreateBuffer,
        enqueue_write_buffer: ClEnqueueWriteBuffer,
        create_program_with_source: ClCreateProgramWithSource,
        build_program: ClBuildProgram,
        get_program_build_info: ClGetProgramBuildInfo,
        create_kernel: ClCreateKernel,
        set_kernel_arg: ClSetKernelArg,
        enqueue_nd_range_kernel: ClEnqueueNDRangeKernel,
        finish: ClFinish,
        enqueue_read_buffer: ClEnqueueReadBuffer,
        release_kernel: ClRelease,
        release_program: ClRelease,
        release_mem: ClRelease,
        release_queue: ClRelease,
        release_context: ClRelease,
    }

    struct Resource {
        handle: *mut c_void,
        release: ClRelease,
    }

    impl Resource {
        fn new(handle: *mut c_void, release: ClRelease) -> Self {
            Self { handle, release }
        }
    }

    impl Drop for Resource {
        fn drop(&mut self) {
            unsafe {
                (self.release)(self.handle);
            }
        }
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
        let (compute, missing_compute_symbols) = resolve_compute_functions(library.0);
        let api = DynamicOpenCl {
            _library: library,
            get_platform_ids: get_platform_ids.expect("checked symbol"),
            get_platform_info: get_platform_info.expect("checked symbol"),
            get_device_ids: get_device_ids.expect("checked symbol"),
            get_device_info: get_device_info.expect("checked symbol"),
            compute,
            missing_compute_symbols,
        };
        collect_with_compute(&api, &api)
    }

    fn resolve_compute_functions(library: HMODULE) -> (Option<ComputeFunctions>, Vec<String>) {
        let mut missing = Vec::new();
        let create_context = required_symbol(
            library,
            b"clCreateContext\0",
            "clCreateContext",
            &mut missing,
        );
        let create_queue_with_properties =
            optional_symbol(library, b"clCreateCommandQueueWithProperties\0");
        let create_queue = optional_symbol(library, b"clCreateCommandQueue\0");
        if create_queue_with_properties.is_none() && create_queue.is_none() {
            missing.push("clCreateCommandQueueWithProperties|clCreateCommandQueue".into());
        }
        let create_buffer =
            required_symbol(library, b"clCreateBuffer\0", "clCreateBuffer", &mut missing);
        let enqueue_write_buffer = required_symbol(
            library,
            b"clEnqueueWriteBuffer\0",
            "clEnqueueWriteBuffer",
            &mut missing,
        );
        let create_program_with_source = required_symbol(
            library,
            b"clCreateProgramWithSource\0",
            "clCreateProgramWithSource",
            &mut missing,
        );
        let build_program =
            required_symbol(library, b"clBuildProgram\0", "clBuildProgram", &mut missing);
        let get_program_build_info = required_symbol(
            library,
            b"clGetProgramBuildInfo\0",
            "clGetProgramBuildInfo",
            &mut missing,
        );
        let create_kernel =
            required_symbol(library, b"clCreateKernel\0", "clCreateKernel", &mut missing);
        let set_kernel_arg =
            required_symbol(library, b"clSetKernelArg\0", "clSetKernelArg", &mut missing);
        let enqueue_nd_range_kernel = required_symbol(
            library,
            b"clEnqueueNDRangeKernel\0",
            "clEnqueueNDRangeKernel",
            &mut missing,
        );
        let finish = required_symbol(library, b"clFinish\0", "clFinish", &mut missing);
        let enqueue_read_buffer = required_symbol(
            library,
            b"clEnqueueReadBuffer\0",
            "clEnqueueReadBuffer",
            &mut missing,
        );
        let release_kernel = required_symbol(
            library,
            b"clReleaseKernel\0",
            "clReleaseKernel",
            &mut missing,
        );
        let release_program = required_symbol(
            library,
            b"clReleaseProgram\0",
            "clReleaseProgram",
            &mut missing,
        );
        let release_mem = required_symbol(
            library,
            b"clReleaseMemObject\0",
            "clReleaseMemObject",
            &mut missing,
        );
        let release_queue = required_symbol(
            library,
            b"clReleaseCommandQueue\0",
            "clReleaseCommandQueue",
            &mut missing,
        );
        let release_context = required_symbol(
            library,
            b"clReleaseContext\0",
            "clReleaseContext",
            &mut missing,
        );
        if !missing.is_empty() {
            return (None, missing);
        }
        (
            Some(ComputeFunctions {
                create_context: create_context.expect("checked"),
                create_queue_with_properties,
                create_queue,
                create_buffer: create_buffer.expect("checked"),
                enqueue_write_buffer: enqueue_write_buffer.expect("checked"),
                create_program_with_source: create_program_with_source.expect("checked"),
                build_program: build_program.expect("checked"),
                get_program_build_info: get_program_build_info.expect("checked"),
                create_kernel: create_kernel.expect("checked"),
                set_kernel_arg: set_kernel_arg.expect("checked"),
                enqueue_nd_range_kernel: enqueue_nd_range_kernel.expect("checked"),
                finish: finish.expect("checked"),
                enqueue_read_buffer: enqueue_read_buffer.expect("checked"),
                release_kernel: release_kernel.expect("checked"),
                release_program: release_program.expect("checked"),
                release_mem: release_mem.expect("checked"),
                release_queue: release_queue.expect("checked"),
                release_context: release_context.expect("checked"),
            }),
            Vec::new(),
        )
    }

    fn required_symbol<T: Copy>(
        library: HMODULE,
        name: &[u8],
        label: &str,
        missing: &mut Vec<String>,
    ) -> Option<T> {
        let value = optional_symbol(library, name);
        if value.is_none() {
            missing.push(label.into());
        }
        value
    }

    fn optional_symbol<T: Copy>(library: HMODULE, name: &[u8]) -> Option<T> {
        let function = unsafe { GetProcAddress(library, name.as_ptr()) }?;
        assert_eq!(size_of::<T>(), size_of_val(&function));
        Some(unsafe { transmute_copy_function(function) })
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

    impl ComputeDeviceProbe for DynamicOpenCl {
        fn probe_device(
            &self,
            platform: usize,
            device: usize,
            platform_version: &str,
            device_version: &str,
        ) -> ComputeDeviceObservation {
            let Some(functions) = self.compute else {
                return ComputeDeviceObservation::not_attempted(
                    self.missing_compute_symbols.clone(),
                );
            };
            run_compute(
                functions,
                platform as ClPlatformId,
                device as ClDeviceId,
                supports_opencl_2(platform_version) && supports_opencl_2(device_version),
            )
        }
    }

    fn supports_opencl_2(version: &str) -> bool {
        version
            .strip_prefix("OpenCL ")
            .and_then(|rest| rest.split_once('.'))
            .and_then(|(major, rest)| {
                Some((major.parse::<u32>().ok()?, rest.split_whitespace().next()?))
            })
            .and_then(|(major, minor)| Some((major, minor.parse::<u32>().ok()?)))
            .is_some_and(|(major, _)| major >= 2)
    }

    fn platform_property(platform: ClPlatformId) -> isize {
        isize::from_ne_bytes((platform as usize).to_ne_bytes())
    }

    fn run_compute(
        functions: ComputeFunctions,
        platform: ClPlatformId,
        device: ClDeviceId,
        supports_with_properties: bool,
    ) -> ComputeDeviceObservation {
        let mut error = CL_SUCCESS;
        let context_properties = [CL_CONTEXT_PLATFORM, platform_property(platform), 0];
        let context_handle = unsafe {
            (functions.create_context)(
                context_properties.as_ptr(),
                1,
                &device,
                None,
                null_mut(),
                &mut error,
            )
        };
        if context_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed(ComputeStage::Context, Some(error));
        }
        let context = Resource::new(context_handle, functions.release_context);

        error = CL_SUCCESS;
        let queue_handle;
        let queue_api;
        if supports_with_properties && functions.create_queue_with_properties.is_some() {
            queue_api = QueueApi::WithProperties;
            let create = functions
                .create_queue_with_properties
                .expect("checked with-properties symbol");
            let properties = [0isize];
            queue_handle =
                unsafe { create(context.handle, device, properties.as_ptr(), &mut error) };
            if queue_handle.is_null() || error != CL_SUCCESS {
                return ComputeDeviceObservation::failed(ComputeStage::Queue, Some(error));
            }
        } else if let Some(create) = functions.create_queue {
            queue_api = QueueApi::Legacy;
            error = CL_SUCCESS;
            queue_handle = unsafe { create(context.handle, device, 0, &mut error) };
        } else {
            return ComputeDeviceObservation::not_attempted(vec![
                "clCreateCommandQueueWithProperties|clCreateCommandQueue".into(),
            ]);
        }
        if queue_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed(ComputeStage::Queue, Some(error));
        }
        let queue = Resource::new(queue_handle, functions.release_queue);

        let byte_count = COMPUTE_ELEMENT_COUNT * size_of::<u32>();
        error = CL_SUCCESS;
        let input_handle = unsafe {
            (functions.create_buffer)(
                context.handle,
                CL_MEM_READ_ONLY,
                byte_count,
                null_mut(),
                &mut error,
            )
        };
        if input_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Buffer,
                Some(error),
                queue_api,
            );
        }
        let input_buffer = Resource::new(input_handle, functions.release_mem);
        error = CL_SUCCESS;
        let output_handle = unsafe {
            (functions.create_buffer)(
                context.handle,
                CL_MEM_WRITE_ONLY,
                byte_count,
                null_mut(),
                &mut error,
            )
        };
        if output_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Buffer,
                Some(error),
                queue_api,
            );
        }
        let output_buffer = Resource::new(output_handle, functions.release_mem);

        let input = (0..COMPUTE_ELEMENT_COUNT as u32).collect::<Vec<_>>();
        let status = unsafe {
            (functions.enqueue_write_buffer)(
                queue.handle,
                input_buffer.handle,
                CL_TRUE,
                0,
                byte_count,
                input.as_ptr().cast(),
                0,
                null(),
                null_mut(),
            )
        };
        if status != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Buffer,
                Some(status),
                queue_api,
            );
        }

        let source = b"__kernel void affine(__global const uint* input, __global uint* output) { size_t i = get_global_id(0); output[i] = input[i] * 3u + 7u; }";
        let source_pointer = source.as_ptr().cast::<i8>();
        let source_length = source.len();
        error = CL_SUCCESS;
        let program_handle = unsafe {
            (functions.create_program_with_source)(
                context.handle,
                1,
                &source_pointer,
                &source_length,
                &mut error,
            )
        };
        if program_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Build,
                Some(error),
                queue_api,
            );
        }
        let program = Resource::new(program_handle, functions.release_program);
        let status = unsafe {
            (functions.build_program)(program.handle, 1, &device, null(), None, null_mut())
        };
        if status != CL_SUCCESS {
            let mut result = ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Build,
                Some(status),
                queue_api,
            );
            result.build_log = Some(build_log_evidence(functions, program.handle, device));
            return result;
        }

        error = CL_SUCCESS;
        let kernel_handle =
            unsafe { (functions.create_kernel)(program.handle, c"affine".as_ptr(), &mut error) };
        if kernel_handle.is_null() || error != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Kernel,
                Some(error),
                queue_api,
            );
        }
        let kernel = Resource::new(kernel_handle, functions.release_kernel);
        for (index, handle) in [input_buffer.handle, output_buffer.handle]
            .into_iter()
            .enumerate()
        {
            let status = unsafe {
                (functions.set_kernel_arg)(
                    kernel.handle,
                    index as u32,
                    size_of::<ClMem>(),
                    (&handle as *const ClMem).cast(),
                )
            };
            if status != CL_SUCCESS {
                return ComputeDeviceObservation::failed_after_queue(
                    ComputeStage::Kernel,
                    Some(status),
                    queue_api,
                );
            }
        }

        let global = [COMPUTE_ELEMENT_COUNT];
        let status = unsafe {
            (functions.enqueue_nd_range_kernel)(
                queue.handle,
                kernel.handle,
                1,
                null(),
                global.as_ptr(),
                null(),
                0,
                null(),
                null_mut(),
            )
        };
        if status != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Enqueue,
                Some(status),
                queue_api,
            );
        }
        let status = unsafe { (functions.finish)(queue.handle) };
        if status != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Finish,
                Some(status),
                queue_api,
            );
        }

        let mut output = vec![0u32; COMPUTE_ELEMENT_COUNT];
        let status = unsafe {
            (functions.enqueue_read_buffer)(
                queue.handle,
                output_buffer.handle,
                CL_TRUE,
                0,
                byte_count,
                output.as_mut_ptr().cast(),
                0,
                null(),
                null_mut(),
            )
        };
        if status != CL_SUCCESS {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Readback,
                Some(status),
                queue_api,
            );
        }
        if output
            .iter()
            .zip(input)
            .any(|(actual, value)| *actual != value * 3 + 7)
        {
            return ComputeDeviceObservation::failed_after_queue(
                ComputeStage::Mismatch,
                None,
                queue_api,
            );
        }
        ComputeDeviceObservation::passed(queue_api)
    }

    fn build_log_evidence(
        functions: ComputeFunctions,
        program: ClProgram,
        device: ClDeviceId,
    ) -> BuildLogObservation {
        let mut size = 0usize;
        let status = unsafe {
            (functions.get_program_build_info)(
                program,
                device,
                CL_PROGRAM_BUILD_LOG,
                0,
                null_mut(),
                &mut size,
            )
        };
        if status != CL_SUCCESS {
            return BuildLogObservation {
                status: BuildLogStatus::Unavailable,
                sha256: None,
            };
        }
        if size > MAX_BUILD_LOG_BYTES {
            return BuildLogObservation {
                status: BuildLogStatus::Overflow,
                sha256: None,
            };
        }
        let mut bytes = vec![0u8; size];
        let mut returned = size;
        let status = unsafe {
            (functions.get_program_build_info)(
                program,
                device,
                CL_PROGRAM_BUILD_LOG,
                size,
                bytes.as_mut_ptr().cast(),
                &mut returned,
            )
        };
        if status != CL_SUCCESS || returned > size {
            return BuildLogObservation {
                status: BuildLogStatus::Unavailable,
                sha256: None,
            };
        }
        bytes.truncate(returned);
        if std::str::from_utf8(&bytes).is_err() {
            return BuildLogObservation {
                status: BuildLogStatus::Malformed,
                sha256: None,
            };
        }
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        BuildLogObservation {
            status: BuildLogStatus::Redacted,
            sha256: Some(sha256),
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Mutex, OnceLock};

        #[derive(Default)]
        struct DispatchState {
            context_properties: Vec<isize>,
            with_properties_error: i32,
            build_error: i32,
            mismatch: bool,
            fail_stage: Option<ComputeStage>,
            with_properties_calls: usize,
            legacy_calls: usize,
            next_buffer: usize,
            releases: Vec<usize>,
        }

        fn dispatch_state() -> &'static Mutex<DispatchState> {
            static STATE: OnceLock<Mutex<DispatchState>> = OnceLock::new();
            STATE.get_or_init(|| Mutex::new(DispatchState::default()))
        }

        fn dispatch_test_lock() -> &'static Mutex<()> {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
        }

        fn releases() -> &'static Mutex<Vec<usize>> {
            static RELEASES: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
            RELEASES.get_or_init(|| Mutex::new(Vec::new()))
        }

        unsafe extern "system" fn record_release(handle: *mut c_void) -> i32 {
            releases().lock().unwrap().push(handle as usize);
            CL_SUCCESS
        }

        unsafe extern "system" fn stub_create_context(
            properties: *const isize,
            _: u32,
            _: *const ClDeviceId,
            _: Option<unsafe extern "system" fn(*const i8, *const c_void, usize, *mut c_void)>,
            _: *mut c_void,
            error: *mut i32,
        ) -> ClContext {
            let values = unsafe { std::slice::from_raw_parts(properties, 3) };
            dispatch_state().lock().unwrap().context_properties = values.to_vec();
            if dispatch_state().lock().unwrap().fail_stage == Some(ComputeStage::Context) {
                unsafe { *error = -5 };
                null_mut()
            } else {
                unsafe { *error = CL_SUCCESS };
                1usize as ClContext
            }
        }

        unsafe extern "system" fn stub_queue_with(
            _: ClContext,
            _: ClDeviceId,
            _: *const isize,
            error: *mut i32,
        ) -> ClCommandQueue {
            let mut state = dispatch_state().lock().unwrap();
            state.with_properties_calls += 1;
            unsafe { *error = state.with_properties_error };
            if state.with_properties_error == CL_SUCCESS {
                2usize as ClCommandQueue
            } else {
                null_mut()
            }
        }

        unsafe extern "system" fn stub_queue_legacy(
            _: ClContext,
            _: ClDeviceId,
            _: u64,
            error: *mut i32,
        ) -> ClCommandQueue {
            dispatch_state().lock().unwrap().legacy_calls += 1;
            unsafe { *error = CL_SUCCESS };
            2usize as ClCommandQueue
        }

        unsafe extern "system" fn stub_create_buffer(
            _: ClContext,
            _: u64,
            _: usize,
            _: *mut c_void,
            error: *mut i32,
        ) -> ClMem {
            let mut state = dispatch_state().lock().unwrap();
            if state.fail_stage == Some(ComputeStage::Buffer) {
                unsafe { *error = -5 };
                return null_mut();
            }
            state.next_buffer += 1;
            unsafe { *error = CL_SUCCESS };
            (2 + state.next_buffer) as ClMem
        }

        unsafe extern "system" fn stub_transfer(
            _: ClCommandQueue,
            _: ClMem,
            _: u32,
            _: usize,
            _: usize,
            _: *const c_void,
            _: u32,
            _: *const ClEvent,
            _: *mut ClEvent,
        ) -> i32 {
            CL_SUCCESS
        }

        unsafe extern "system" fn stub_read(
            _: ClCommandQueue,
            _: ClMem,
            _: u32,
            _: usize,
            size: usize,
            output: *const c_void,
            _: u32,
            _: *const ClEvent,
            _: *mut ClEvent,
        ) -> i32 {
            if dispatch_state().lock().unwrap().fail_stage == Some(ComputeStage::Readback) {
                return -5;
            }
            let output = unsafe {
                std::slice::from_raw_parts_mut(output as *mut u32, size / size_of::<u32>())
            };
            let mismatch = dispatch_state().lock().unwrap().mismatch;
            for (index, value) in output.iter_mut().enumerate() {
                *value = index as u32 * 3 + 7;
            }
            if mismatch {
                output[0] ^= 1;
            }
            CL_SUCCESS
        }

        unsafe extern "system" fn stub_program(
            _: ClContext,
            _: u32,
            _: *const *const i8,
            _: *const usize,
            error: *mut i32,
        ) -> ClProgram {
            unsafe { *error = CL_SUCCESS };
            5usize as ClProgram
        }

        unsafe extern "system" fn stub_build(
            _: ClProgram,
            _: u32,
            _: *const ClDeviceId,
            _: *const i8,
            _: Option<unsafe extern "system" fn(ClProgram, *mut c_void)>,
            _: *mut c_void,
        ) -> i32 {
            dispatch_state().lock().unwrap().build_error
        }

        unsafe extern "system" fn stub_build_log(
            _: ClProgram,
            _: ClDeviceId,
            _: u32,
            size: usize,
            output: *mut c_void,
            returned: *mut usize,
        ) -> i32 {
            let log = b"C:\\Users\\private\\kernel.cl: build failed\0";
            unsafe { *returned = log.len() };
            if size != 0 {
                unsafe { std::ptr::copy_nonoverlapping(log.as_ptr(), output.cast(), log.len()) };
            }
            CL_SUCCESS
        }

        unsafe extern "system" fn stub_kernel(
            _: ClProgram,
            _: *const i8,
            error: *mut i32,
        ) -> ClKernel {
            if dispatch_state().lock().unwrap().fail_stage == Some(ComputeStage::Kernel) {
                unsafe { *error = -5 };
                null_mut()
            } else {
                unsafe { *error = CL_SUCCESS };
                6usize as ClKernel
            }
        }

        unsafe extern "system" fn stub_set_arg(
            _: ClKernel,
            _: u32,
            _: usize,
            _: *const c_void,
        ) -> i32 {
            CL_SUCCESS
        }

        unsafe extern "system" fn stub_enqueue(
            _: ClCommandQueue,
            _: ClKernel,
            _: u32,
            _: *const usize,
            _: *const usize,
            _: *const usize,
            _: u32,
            _: *const ClEvent,
            _: *mut ClEvent,
        ) -> i32 {
            if dispatch_state().lock().unwrap().fail_stage == Some(ComputeStage::Enqueue) {
                -5
            } else {
                CL_SUCCESS
            }
        }

        unsafe extern "system" fn stub_finish(_: ClCommandQueue) -> i32 {
            if dispatch_state().lock().unwrap().fail_stage == Some(ComputeStage::Finish) {
                -5
            } else {
                CL_SUCCESS
            }
        }

        unsafe extern "system" fn stub_release(handle: *mut c_void) -> i32 {
            dispatch_state()
                .lock()
                .unwrap()
                .releases
                .push(handle as usize);
            CL_SUCCESS
        }

        fn dispatch() -> ComputeFunctions {
            ComputeFunctions {
                create_context: stub_create_context,
                create_queue_with_properties: Some(stub_queue_with),
                create_queue: Some(stub_queue_legacy),
                create_buffer: stub_create_buffer,
                enqueue_write_buffer: stub_transfer,
                create_program_with_source: stub_program,
                build_program: stub_build,
                get_program_build_info: stub_build_log,
                create_kernel: stub_kernel,
                set_kernel_arg: stub_set_arg,
                enqueue_nd_range_kernel: stub_enqueue,
                finish: stub_finish,
                enqueue_read_buffer: stub_read,
                release_kernel: stub_release,
                release_program: stub_release,
                release_mem: stub_release,
                release_queue: stub_release,
                release_context: stub_release,
            }
        }

        fn reset_dispatch() {
            *dispatch_state().lock().unwrap() = DispatchState::default();
        }

        #[test]
        fn dispatch_binds_platform_and_limits_queue_fallback() {
            let _serial = dispatch_test_lock().lock().unwrap();
            reset_dispatch();
            let result = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                true,
            );
            assert_eq!(result.stage, ComputeStage::Passed);
            assert_eq!(result.queue_api, Some(QueueApi::WithProperties));
            let state = dispatch_state().lock().unwrap();
            assert_eq!(state.context_properties, [CL_CONTEXT_PLATFORM, 0x1234, 0]);
            assert_eq!(state.with_properties_calls, 1);
            assert_eq!(state.legacy_calls, 0);
            assert_eq!(state.releases, [6, 5, 4, 3, 2, 1]);
            drop(state);

            reset_dispatch();
            let legacy = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                false,
            );
            assert_eq!(legacy.stage, ComputeStage::Passed);
            assert_eq!(legacy.queue_api, Some(QueueApi::Legacy));
            assert_eq!(dispatch_state().lock().unwrap().legacy_calls, 1);

            reset_dispatch();
            dispatch_state().lock().unwrap().with_properties_error = -30;
            let failed = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                true,
            );
            assert_eq!(failed.stage, ComputeStage::Queue);
            assert_eq!(failed.queue_api, None);
            let state = dispatch_state().lock().unwrap();
            assert_eq!(state.legacy_calls, 0);
            assert_eq!(state.releases, [1]);
        }

        #[test]
        fn dispatch_build_log_and_mismatch_are_fail_closed_with_cleanup() {
            let _serial = dispatch_test_lock().lock().unwrap();
            reset_dispatch();
            dispatch_state().lock().unwrap().build_error = -11;
            let build = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                true,
            );
            assert_eq!(build.stage, ComputeStage::Build);
            assert_eq!(build.queue_api, Some(QueueApi::WithProperties));
            let evidence = build.build_log.expect("bounded build log evidence");
            assert_eq!(evidence.status, BuildLogStatus::Redacted);
            assert_eq!(evidence.sha256.unwrap().len(), 64);
            assert_eq!(dispatch_state().lock().unwrap().releases, [5, 4, 3, 2, 1]);

            reset_dispatch();
            dispatch_state().lock().unwrap().mismatch = true;
            let mismatch = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                true,
            );
            assert_eq!(mismatch.stage, ComputeStage::Mismatch);
            assert_eq!(mismatch.queue_api, Some(QueueApi::WithProperties));
            assert_eq!(
                dispatch_state().lock().unwrap().releases,
                [6, 5, 4, 3, 2, 1]
            );

            reset_dispatch();
            dispatch_state().lock().unwrap().fail_stage = Some(ComputeStage::Readback);
            let legacy_failure = run_compute(
                dispatch(),
                0x1234usize as ClPlatformId,
                0x5678usize as ClDeviceId,
                false,
            );
            assert_eq!(legacy_failure.stage, ComputeStage::Readback);
            assert_eq!(legacy_failure.queue_api, Some(QueueApi::Legacy));
            assert_eq!(
                dispatch_state().lock().unwrap().releases,
                [6, 5, 4, 3, 2, 1]
            );
        }

        #[test]
        fn each_early_failure_releases_exactly_the_owned_prefix() {
            let _serial = dispatch_test_lock().lock().unwrap();
            for (stage, expected_releases) in [
                (ComputeStage::Context, Vec::new()),
                (ComputeStage::Buffer, vec![2, 1]),
                (ComputeStage::Kernel, vec![5, 4, 3, 2, 1]),
                (ComputeStage::Enqueue, vec![6, 5, 4, 3, 2, 1]),
                (ComputeStage::Finish, vec![6, 5, 4, 3, 2, 1]),
                (ComputeStage::Readback, vec![6, 5, 4, 3, 2, 1]),
            ] {
                reset_dispatch();
                dispatch_state().lock().unwrap().fail_stage = Some(stage);
                let result = run_compute(
                    dispatch(),
                    0x1234usize as ClPlatformId,
                    0x5678usize as ClDeviceId,
                    true,
                );
                assert_eq!(result.stage, stage);
                assert_eq!(
                    result.queue_api,
                    if stage == ComputeStage::Context {
                        None
                    } else {
                        Some(QueueApi::WithProperties)
                    },
                    "stage {stage:?}"
                );
                assert_eq!(
                    dispatch_state().lock().unwrap().releases,
                    expected_releases,
                    "stage {stage:?}"
                );
            }
        }

        #[test]
        fn resources_release_exactly_once_in_reverse_ownership_order() {
            releases().lock().unwrap().clear();
            {
                let _context = Resource::new(1usize as *mut c_void, record_release);
                let _queue = Resource::new(2usize as *mut c_void, record_release);
                let _input = Resource::new(3usize as *mut c_void, record_release);
                let _output = Resource::new(4usize as *mut c_void, record_release);
                let _program = Resource::new(5usize as *mut c_void, record_release);
                let _kernel = Resource::new(6usize as *mut c_void, record_release);
            }
            assert_eq!(*releases().lock().unwrap(), [6, 5, 4, 3, 2, 1]);
        }

        #[test]
        fn early_return_releases_each_owned_resource_once() {
            fn fail_after_queue() -> Result<(), ()> {
                let _context = Resource::new(1usize as *mut c_void, record_release);
                let _queue = Resource::new(2usize as *mut c_void, record_release);
                Err(())
            }

            releases().lock().unwrap().clear();
            assert!(fail_after_queue().is_err());
            assert_eq!(*releases().lock().unwrap(), [2, 1]);
        }
    }
}
