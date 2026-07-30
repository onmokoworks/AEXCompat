use aexcompat_broker::cuda_compute_probe::{
    collect_with_api, device_name, missing_symbol_observation, no_driver_observation,
    uuid_fingerprint, ApiFailure, CudaAggregateObservation, CudaDeviceObservation, CudaProbeApi,
    CudaStage, JitLogObservation, PciLocation, COMPUTE_ELEMENT_COUNT, MAX_DEVICE_NAME_BYTES,
    MAX_JIT_LOG_BYTES,
};

fn main() {
    let observation = platform::probe();
    match serde_json::to_string(&observation) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("CUDA probe serialization failed: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn probe() -> CudaAggregateObservation {
        no_driver_observation()
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::mem::{size_of, size_of_val, transmute_copy};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    const CUDA_SUCCESS: i32 = 0;
    const CUDA_ERROR_NO_BINARY_FOR_GPU: i32 = 209;
    const CUDA_ERROR_INVALID_PTX: i32 = 218;
    const CUDA_ERROR_JIT_COMPILER_NOT_FOUND: i32 = 221;
    const CUDA_ERROR_UNSUPPORTED_PTX_VERSION: i32 = 222;

    const CU_DEVICE_ATTRIBUTE_PCI_BUS_ID: i32 = 33;
    const CU_DEVICE_ATTRIBUTE_PCI_DEVICE_ID: i32 = 34;
    const CU_DEVICE_ATTRIBUTE_PCI_DOMAIN_ID: i32 = 50;
    const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: i32 = 75;
    const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: i32 = 76;

    const CU_JIT_INFO_LOG_BUFFER: i32 = 3;
    const CU_JIT_INFO_LOG_BUFFER_SIZE_BYTES: i32 = 4;
    const CU_JIT_ERROR_LOG_BUFFER: i32 = 5;
    const CU_JIT_ERROR_LOG_BUFFER_SIZE_BYTES: i32 = 6;

    const EMBEDDED_PTX: &[u8] = b".version 6.0\n\
.target sm_30\n\
.address_size 64\n\
\n\
.visible .entry affine(\n\
    .param .u64 input_ptr,\n\
    .param .u64 output_ptr\n\
)\n\
{\n\
    .reg .pred %p;\n\
    .reg .b32 %r<4>;\n\
    .reg .b64 %rd<6>;\n\
    ld.param.u64 %rd1, [input_ptr];\n\
    ld.param.u64 %rd2, [output_ptr];\n\
    mov.u32 %r1, %tid.x;\n\
    setp.ge.u32 %p, %r1, 64;\n\
    @%p bra DONE;\n\
    mul.wide.u32 %rd3, %r1, 4;\n\
    add.s64 %rd4, %rd1, %rd3;\n\
    ld.global.u32 %r2, [%rd4];\n\
    mad.lo.u32 %r3, %r2, 3, 7;\n\
    add.s64 %rd5, %rd2, %rd3;\n\
    st.global.u32 [%rd5], %r3;\n\
DONE:\n\
    ret;\n\
}\n\0";

    type CuResult = i32;
    type CuDevice = i32;
    type CuDevicePtr = u64;
    type CuContext = *mut c_void;
    type CuModule = *mut c_void;
    type CuFunction = *mut c_void;
    type CuStream = *mut c_void;

    #[repr(C)]
    struct CuUuid {
        bytes: [u8; 16],
    }

    type CuInit = unsafe extern "system" fn(u32) -> CuResult;
    type CuDriverGetVersion = unsafe extern "system" fn(*mut i32) -> CuResult;
    type CuDeviceGetCount = unsafe extern "system" fn(*mut i32) -> CuResult;
    type CuDeviceGet = unsafe extern "system" fn(*mut CuDevice, i32) -> CuResult;
    type CuDeviceGetName = unsafe extern "system" fn(*mut i8, i32, CuDevice) -> CuResult;
    type CuDeviceGetUuid = unsafe extern "system" fn(*mut CuUuid, CuDevice) -> CuResult;
    type CuDeviceGetAttribute = unsafe extern "system" fn(*mut i32, i32, CuDevice) -> CuResult;
    type CuDeviceTotalMem = unsafe extern "system" fn(*mut usize, CuDevice) -> CuResult;
    type CuCtxCreate = unsafe extern "system" fn(*mut CuContext, u32, CuDevice) -> CuResult;
    type CuCtxDestroy = unsafe extern "system" fn(CuContext) -> CuResult;
    type CuCtxGetCurrent = unsafe extern "system" fn(*mut CuContext) -> CuResult;
    type CuMemAlloc = unsafe extern "system" fn(*mut CuDevicePtr, usize) -> CuResult;
    type CuMemFree = unsafe extern "system" fn(CuDevicePtr) -> CuResult;
    type CuMemcpyHtoD = unsafe extern "system" fn(CuDevicePtr, *const c_void, usize) -> CuResult;
    type CuMemcpyDtoH = unsafe extern "system" fn(*mut c_void, CuDevicePtr, usize) -> CuResult;
    type CuModuleLoadDataEx = unsafe extern "system" fn(
        *mut CuModule,
        *const c_void,
        u32,
        *mut i32,
        *mut *mut c_void,
    ) -> CuResult;
    type CuModuleUnload = unsafe extern "system" fn(CuModule) -> CuResult;
    type CuModuleGetFunction =
        unsafe extern "system" fn(*mut CuFunction, CuModule, *const i8) -> CuResult;
    type CuLaunchKernel = unsafe extern "system" fn(
        CuFunction,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        CuStream,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> CuResult;
    type CuCtxSynchronize = unsafe extern "system" fn() -> CuResult;

    #[derive(Clone, Copy)]
    struct CudaFunctions {
        init: CuInit,
        driver_get_version: CuDriverGetVersion,
        device_get_count: CuDeviceGetCount,
        device_get: CuDeviceGet,
        device_get_name: CuDeviceGetName,
        device_get_uuid: CuDeviceGetUuid,
        device_get_attribute: CuDeviceGetAttribute,
        device_total_mem: CuDeviceTotalMem,
        ctx_create: CuCtxCreate,
        ctx_destroy: CuCtxDestroy,
        ctx_get_current: CuCtxGetCurrent,
        mem_alloc: CuMemAlloc,
        mem_free: CuMemFree,
        memcpy_htod: CuMemcpyHtoD,
        memcpy_dtoh: CuMemcpyDtoH,
        module_load_data_ex: CuModuleLoadDataEx,
        module_unload: CuModuleUnload,
        module_get_function: CuModuleGetFunction,
        launch_kernel: CuLaunchKernel,
        ctx_synchronize: CuCtxSynchronize,
    }

    struct Library(HMODULE);

    impl Drop for Library {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }

    type RawFunction = unsafe extern "system" fn() -> isize;

    trait SymbolResolver {
        fn resolve(&self, name: &[u8]) -> Option<RawFunction>;
    }

    trait CudaLibraryLoader {
        type Loaded: SymbolResolver;

        fn load(&self) -> Option<Self::Loaded>;
    }

    struct SystemCudaLoader;

    struct DynamicCuda<L> {
        _library: L,
        functions: CudaFunctions,
    }

    struct OwnedContext {
        handle: CuContext,
        release: CuCtxDestroy,
    }

    impl Drop for OwnedContext {
        fn drop(&mut self) {
            unsafe {
                (self.release)(self.handle);
            }
        }
    }

    struct DeviceMemory {
        handle: CuDevicePtr,
        release: CuMemFree,
    }

    impl Drop for DeviceMemory {
        fn drop(&mut self) {
            unsafe {
                (self.release)(self.handle);
            }
        }
    }

    struct OwnedModule {
        handle: CuModule,
        release: CuModuleUnload,
    }

    impl Drop for OwnedModule {
        fn drop(&mut self) {
            unsafe {
                (self.release)(self.handle);
            }
        }
    }

    pub fn probe() -> CudaAggregateObservation {
        probe_with_loader(&SystemCudaLoader)
    }

    fn probe_with_loader<L: CudaLibraryLoader>(loader: &L) -> CudaAggregateObservation {
        let library = match loader.load() {
            Some(library) => library,
            None => return no_driver_observation(),
        };
        let (functions, missing) = resolve_functions(&library);
        if !missing.is_empty() {
            return missing_symbol_observation(&missing);
        }
        let api = DynamicCuda {
            _library: library,
            functions: functions.expect("all required CUDA symbols resolved"),
        };
        collect_with_api(&api)
    }

    impl CudaLibraryLoader for SystemCudaLoader {
        type Loaded = Library;

        fn load(&self) -> Option<Self::Loaded> {
            let name = "nvcuda.dll\0".encode_utf16().collect::<Vec<_>>();
            let handle =
                unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
            (!handle.is_null()).then_some(Library(handle))
        }
    }

    impl SymbolResolver for Library {
        fn resolve(&self, name: &[u8]) -> Option<RawFunction> {
            unsafe { GetProcAddress(self.0, name.as_ptr()) }
        }
    }

    fn resolve_functions(resolver: &impl SymbolResolver) -> (Option<CudaFunctions>, Vec<String>) {
        let mut missing = Vec::new();
        let init = required_symbol(resolver, &[b"cuInit\0"], "cuInit", &mut missing);
        let driver_get_version = required_symbol(
            resolver,
            &[b"cuDriverGetVersion\0"],
            "cuDriverGetVersion",
            &mut missing,
        );
        let device_get_count = required_symbol(
            resolver,
            &[b"cuDeviceGetCount\0"],
            "cuDeviceGetCount",
            &mut missing,
        );
        let device_get =
            required_symbol(resolver, &[b"cuDeviceGet\0"], "cuDeviceGet", &mut missing);
        let device_get_name = required_symbol(
            resolver,
            &[b"cuDeviceGetName\0"],
            "cuDeviceGetName",
            &mut missing,
        );
        // ABI-width-sensitive entry points are intentionally pinned to the
        // exported v2 names. Header macro aliases are not available to a
        // runtime resolver and falling back would silently select a different
        // ABI on older drivers.
        let device_get_uuid = required_symbol(
            resolver,
            &[b"cuDeviceGetUuid_v2\0"],
            "cuDeviceGetUuid_v2",
            &mut missing,
        );
        let device_get_attribute = required_symbol(
            resolver,
            &[b"cuDeviceGetAttribute\0"],
            "cuDeviceGetAttribute",
            &mut missing,
        );
        let device_total_mem = required_symbol(
            resolver,
            &[b"cuDeviceTotalMem_v2\0"],
            "cuDeviceTotalMem_v2",
            &mut missing,
        );
        let ctx_create = required_symbol(
            resolver,
            &[b"cuCtxCreate_v2\0"],
            "cuCtxCreate_v2",
            &mut missing,
        );
        let ctx_destroy = required_symbol(
            resolver,
            &[b"cuCtxDestroy_v2\0"],
            "cuCtxDestroy_v2",
            &mut missing,
        );
        let ctx_get_current = required_symbol(
            resolver,
            &[b"cuCtxGetCurrent\0"],
            "cuCtxGetCurrent",
            &mut missing,
        );
        let mem_alloc = required_symbol(
            resolver,
            &[b"cuMemAlloc_v2\0"],
            "cuMemAlloc_v2",
            &mut missing,
        );
        let mem_free =
            required_symbol(resolver, &[b"cuMemFree_v2\0"], "cuMemFree_v2", &mut missing);
        let memcpy_htod = required_symbol(
            resolver,
            &[b"cuMemcpyHtoD_v2\0"],
            "cuMemcpyHtoD_v2",
            &mut missing,
        );
        let memcpy_dtoh = required_symbol(
            resolver,
            &[b"cuMemcpyDtoH_v2\0"],
            "cuMemcpyDtoH_v2",
            &mut missing,
        );
        let module_load_data_ex = required_symbol(
            resolver,
            &[b"cuModuleLoadDataEx\0"],
            "cuModuleLoadDataEx",
            &mut missing,
        );
        let module_unload = required_symbol(
            resolver,
            &[b"cuModuleUnload\0"],
            "cuModuleUnload",
            &mut missing,
        );
        let module_get_function = required_symbol(
            resolver,
            &[b"cuModuleGetFunction\0"],
            "cuModuleGetFunction",
            &mut missing,
        );
        let launch_kernel = required_symbol(
            resolver,
            &[b"cuLaunchKernel\0"],
            "cuLaunchKernel",
            &mut missing,
        );
        let ctx_synchronize = required_symbol(
            resolver,
            &[b"cuCtxSynchronize\0"],
            "cuCtxSynchronize",
            &mut missing,
        );
        if !missing.is_empty() {
            return (None, missing);
        }
        (
            Some(CudaFunctions {
                init: init.expect("checked"),
                driver_get_version: driver_get_version.expect("checked"),
                device_get_count: device_get_count.expect("checked"),
                device_get: device_get.expect("checked"),
                device_get_name: device_get_name.expect("checked"),
                device_get_uuid: device_get_uuid.expect("checked"),
                device_get_attribute: device_get_attribute.expect("checked"),
                device_total_mem: device_total_mem.expect("checked"),
                ctx_create: ctx_create.expect("checked"),
                ctx_destroy: ctx_destroy.expect("checked"),
                ctx_get_current: ctx_get_current.expect("checked"),
                mem_alloc: mem_alloc.expect("checked"),
                mem_free: mem_free.expect("checked"),
                memcpy_htod: memcpy_htod.expect("checked"),
                memcpy_dtoh: memcpy_dtoh.expect("checked"),
                module_load_data_ex: module_load_data_ex.expect("checked"),
                module_unload: module_unload.expect("checked"),
                module_get_function: module_get_function.expect("checked"),
                launch_kernel: launch_kernel.expect("checked"),
                ctx_synchronize: ctx_synchronize.expect("checked"),
            }),
            Vec::new(),
        )
    }

    fn required_symbol<T: Copy>(
        resolver: &impl SymbolResolver,
        aliases: &[&[u8]],
        label: &str,
        missing: &mut Vec<String>,
    ) -> Option<T> {
        for alias in aliases {
            if let Some(value) = optional_symbol(resolver, alias) {
                return Some(value);
            }
        }
        missing.push(label.into());
        None
    }

    fn optional_symbol<T: Copy>(resolver: &impl SymbolResolver, name: &[u8]) -> Option<T> {
        let function = resolver.resolve(name)?;
        assert_eq!(size_of::<T>(), size_of_val(&function));
        Some(unsafe { transmute_copy_function(function) })
    }

    unsafe fn transmute_copy_function<T: Copy>(
        function: unsafe extern "system" fn() -> isize,
    ) -> T {
        unsafe { transmute_copy(&function) }
    }

    impl<L> CudaProbeApi for DynamicCuda<L> {
        fn initialize(&self) -> Result<(), ApiFailure> {
            let status = unsafe { (self.functions.init)(0) };
            cuda_result("cuInit", status)
        }

        fn driver_version(&self) -> Result<u32, ApiFailure> {
            let mut version = 0i32;
            let status = unsafe { (self.functions.driver_get_version)(&mut version) };
            cuda_result("cuDriverGetVersion", status)?;
            u32::try_from(version).map_err(|_| ApiFailure::malformed("cuDriverGetVersion"))
        }

        fn device_count(&self, _limit: usize) -> Result<usize, ApiFailure> {
            let mut count = 0i32;
            let status = unsafe { (self.functions.device_get_count)(&mut count) };
            cuda_result("cuDeviceGetCount", status)?;
            let count =
                usize::try_from(count).map_err(|_| ApiFailure::malformed("cuDeviceGetCount"))?;
            Ok(count)
        }

        fn probe_device(&self, ordinal: u32) -> CudaDeviceObservation {
            probe_device(self.functions, ordinal)
        }
    }

    fn probe_device(functions: CudaFunctions, ordinal: u32) -> CudaDeviceObservation {
        let mut device = 0;
        let status = unsafe { (functions.device_get)(&mut device, ordinal as i32) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::metadata_failure(ordinal, "cuDeviceGet", Some(status));
        }

        let mut name_bytes = [0u8; MAX_DEVICE_NAME_BYTES];
        let status = unsafe {
            (functions.device_get_name)(
                name_bytes.as_mut_ptr().cast(),
                name_bytes.len() as i32,
                device,
            )
        };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::metadata_failure(
                ordinal,
                "cuDeviceGetName",
                Some(status),
            );
        }
        let name = match device_name(&name_bytes) {
            Ok(name) => name,
            Err(_) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    "cuDeviceGetName.privacy",
                    None,
                );
            }
        };

        let mut uuid = CuUuid { bytes: [0; 16] };
        let status = unsafe { (functions.device_get_uuid)(&mut uuid, device) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::metadata_failure(
                ordinal,
                "cuDeviceGetUuid",
                Some(status),
            );
        }

        let domain = match device_attribute(
            functions,
            device,
            CU_DEVICE_ATTRIBUTE_PCI_DOMAIN_ID,
            "cuDeviceGetAttribute.pci_domain",
        ) {
            Ok(value) => value,
            Err(error) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    error.operation,
                    error.api_error,
                );
            }
        };
        let bus = match device_attribute(
            functions,
            device,
            CU_DEVICE_ATTRIBUTE_PCI_BUS_ID,
            "cuDeviceGetAttribute.pci_bus",
        ) {
            Ok(value) => value,
            Err(error) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    error.operation,
                    error.api_error,
                );
            }
        };
        let pci_device = match device_attribute(
            functions,
            device,
            CU_DEVICE_ATTRIBUTE_PCI_DEVICE_ID,
            "cuDeviceGetAttribute.pci_device",
        ) {
            Ok(value) => value,
            Err(error) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    error.operation,
                    error.api_error,
                );
            }
        };
        let major = match device_attribute(
            functions,
            device,
            CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
            "cuDeviceGetAttribute.compute_major",
        ) {
            Ok(value) => value,
            Err(error) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    error.operation,
                    error.api_error,
                );
            }
        };
        let minor = match device_attribute(
            functions,
            device,
            CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
            "cuDeviceGetAttribute.compute_minor",
        ) {
            Ok(value) => value,
            Err(error) => {
                return CudaDeviceObservation::metadata_failure(
                    ordinal,
                    error.operation,
                    error.api_error,
                );
            }
        };
        let mut total_memory = 0usize;
        let status = unsafe { (functions.device_total_mem)(&mut total_memory, device) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::metadata_failure(
                ordinal,
                "cuDeviceTotalMem",
                Some(status),
            );
        }
        let metadata = CudaDeviceObservation::passed(
            ordinal,
            name,
            uuid_fingerprint(&uuid.bytes),
            PciLocation {
                domain,
                bus,
                device: pci_device,
            },
            major,
            minor,
            total_memory as u64,
        );
        if domain > 0xffff
            || bus > 0xff
            || pci_device > 0x1f
            || major == 0
            || major > 1024
            || minor > 1024
            || total_memory == 0
        {
            return CudaDeviceObservation::metadata_failure(
                ordinal,
                "device_metadata_bounds",
                None,
            );
        }
        run_compute(functions, device, metadata)
    }

    fn device_attribute(
        functions: CudaFunctions,
        device: CuDevice,
        attribute: i32,
        operation: &'static str,
    ) -> Result<u32, ApiFailure> {
        let mut value = 0i32;
        let status = unsafe { (functions.device_get_attribute)(&mut value, attribute, device) };
        cuda_result(operation, status)?;
        u32::try_from(value).map_err(|_| ApiFailure::malformed(operation))
    }

    fn cuda_result(operation: &'static str, status: CuResult) -> Result<(), ApiFailure> {
        if status == CUDA_SUCCESS {
            Ok(())
        } else {
            Err(ApiFailure::api(operation, status))
        }
    }

    fn run_compute(
        functions: CudaFunctions,
        device: CuDevice,
        metadata: CudaDeviceObservation,
    ) -> CudaDeviceObservation {
        // This dedicated child starts without an application-owned CUDA
        // context. cuCtxCreate_v2 makes the owned context current and
        // cuCtxDestroy_v2 restores the previous (null) current context. Check
        // the boundary before every device so a failed cleanup cannot
        // contaminate the next device probe.
        let mut previous_context = null_mut();
        let status = unsafe { (functions.ctx_get_current)(&mut previous_context) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Context,
                "cuCtxGetCurrent",
                Some(status),
                None,
            );
        }
        if !previous_context.is_null() {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Context,
                "cuCtxGetCurrent.preexisting_context",
                None,
                None,
            );
        }

        let mut context_handle = null_mut();
        let status = unsafe { (functions.ctx_create)(&mut context_handle, 0, device) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Context,
                "cuCtxCreate_v2",
                Some(status),
                None,
            );
        }
        if context_handle.is_null() {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Context,
                "cuCtxCreate_v2.null_handle",
                None,
                None,
            );
        }
        let _context = OwnedContext {
            handle: context_handle,
            release: functions.ctx_destroy,
        };

        let byte_count = COMPUTE_ELEMENT_COUNT * size_of::<u32>();
        let mut input_handle = 0;
        let status = unsafe { (functions.mem_alloc)(&mut input_handle, byte_count) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Allocation,
                "cuMemAlloc_v2.input",
                Some(status),
                None,
            );
        }
        if input_handle == 0 {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Allocation,
                "cuMemAlloc_v2.input.null_handle",
                None,
                None,
            );
        }
        let input_memory = DeviceMemory {
            handle: input_handle,
            release: functions.mem_free,
        };

        let mut output_handle = 0;
        let status = unsafe { (functions.mem_alloc)(&mut output_handle, byte_count) };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Allocation,
                "cuMemAlloc_v2.output",
                Some(status),
                None,
            );
        }
        if output_handle == 0 {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Allocation,
                "cuMemAlloc_v2.output.null_handle",
                None,
                None,
            );
        }
        let output_memory = DeviceMemory {
            handle: output_handle,
            release: functions.mem_free,
        };

        let input = (0..COMPUTE_ELEMENT_COUNT as u32).collect::<Vec<_>>();
        let status = unsafe {
            (functions.memcpy_htod)(input_memory.handle, input.as_ptr().cast(), byte_count)
        };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::HostToDevice,
                "cuMemcpyHtoD_v2",
                Some(status),
                None,
            );
        }

        let mut info_log = [0u8; MAX_JIT_LOG_BYTES];
        let mut error_log = [0u8; MAX_JIT_LOG_BYTES];
        let mut info_size = info_log.len() as u32;
        let mut error_size = error_log.len() as u32;
        let mut options = [
            CU_JIT_INFO_LOG_BUFFER,
            CU_JIT_INFO_LOG_BUFFER_SIZE_BYTES,
            CU_JIT_ERROR_LOG_BUFFER,
            CU_JIT_ERROR_LOG_BUFFER_SIZE_BYTES,
        ];
        let mut option_values = [
            info_log.as_mut_ptr().cast(),
            (&mut info_size as *mut u32).cast(),
            error_log.as_mut_ptr().cast(),
            (&mut error_size as *mut u32).cast(),
        ];
        let mut module_handle = null_mut();
        let status = unsafe {
            (functions.module_load_data_ex)(
                &mut module_handle,
                EMBEDDED_PTX.as_ptr().cast(),
                options.len() as u32,
                options.as_mut_ptr(),
                option_values.as_mut_ptr(),
            )
        };
        if status != CUDA_SUCCESS {
            let jit_log = jit_log_observation(&info_log, info_size, &error_log, error_size);
            let jit_failure = is_jit_failure(status)
                || !matches!(
                    jit_log.status,
                    aexcompat_broker::cuda_compute_probe::JitLogStatus::Empty
                        | aexcompat_broker::cuda_compute_probe::JitLogStatus::Unavailable
                );
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                if jit_failure {
                    CudaStage::Jit
                } else {
                    CudaStage::Module
                },
                "cuModuleLoadDataEx",
                Some(status),
                jit_failure.then_some(jit_log),
            );
        }
        if module_handle.is_null() {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Module,
                "cuModuleLoadDataEx.null_handle",
                None,
                None,
            );
        }
        let module = OwnedModule {
            handle: module_handle,
            release: functions.module_unload,
        };

        let mut function_handle = null_mut();
        let status = unsafe {
            (functions.module_get_function)(&mut function_handle, module.handle, c"affine".as_ptr())
        };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Function,
                "cuModuleGetFunction",
                Some(status),
                None,
            );
        }
        if function_handle.is_null() {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Function,
                "cuModuleGetFunction.null_handle",
                None,
                None,
            );
        }

        let mut input_argument = input_memory.handle;
        let mut output_argument = output_memory.handle;
        let mut arguments = [
            (&mut input_argument as *mut CuDevicePtr).cast::<c_void>(),
            (&mut output_argument as *mut CuDevicePtr).cast::<c_void>(),
        ];
        let status = unsafe {
            (functions.launch_kernel)(
                function_handle,
                1,
                1,
                1,
                COMPUTE_ELEMENT_COUNT as u32,
                1,
                1,
                0,
                null_mut(),
                arguments.as_mut_ptr(),
                null_mut(),
            )
        };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Launch,
                "cuLaunchKernel",
                Some(status),
                None,
            );
        }

        let status = unsafe { (functions.ctx_synchronize)() };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Synchronize,
                "cuCtxSynchronize",
                Some(status),
                None,
            );
        }

        let mut output = [0u32; COMPUTE_ELEMENT_COUNT];
        let status = unsafe {
            (functions.memcpy_dtoh)(output.as_mut_ptr().cast(), output_memory.handle, byte_count)
        };
        if status != CUDA_SUCCESS {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::DeviceToHost,
                "cuMemcpyDtoH_v2",
                Some(status),
                None,
            );
        }
        if output
            .iter()
            .zip(input)
            .any(|(actual, value)| *actual != value * 3 + 7)
        {
            return CudaDeviceObservation::with_compute_failure(
                metadata,
                CudaStage::Mismatch,
                "readback_compare",
                None,
                None,
            );
        }
        metadata
    }

    fn is_jit_failure(status: CuResult) -> bool {
        matches!(
            status,
            CUDA_ERROR_NO_BINARY_FOR_GPU
                | CUDA_ERROR_INVALID_PTX
                | CUDA_ERROR_JIT_COMPILER_NOT_FOUND
                | CUDA_ERROR_UNSUPPORTED_PTX_VERSION
        )
    }

    fn jit_log_observation(
        info: &[u8],
        info_size: u32,
        error: &[u8],
        error_size: u32,
    ) -> JitLogObservation {
        let (info, info_overflow) = bounded_log(info, info_size);
        let (error, error_overflow) = bounded_log(error, error_size);
        JitLogObservation::from_bounded_logs(info, error, info_overflow || error_overflow)
    }

    fn bounded_log(buffer: &[u8], reported_size: u32) -> (&[u8], bool) {
        let Ok(reported_size) = usize::try_from(reported_size) else {
            return (&[], true);
        };
        if reported_size > buffer.len() {
            return (&[], true);
        }
        let candidate = &buffer[..reported_size];
        let length = candidate
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(candidate.len());
        (&candidate[..length], false)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::RefCell;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::sync::{Mutex, OnceLock};

        struct MockLoadedResolver {
            missing: Option<&'static [u8]>,
            drops: Arc<AtomicUsize>,
        }

        impl SymbolResolver for MockLoadedResolver {
            fn resolve(&self, name: &[u8]) -> Option<RawFunction> {
                if self.missing.is_some_and(|missing| missing == name) {
                    None
                } else {
                    Some(dummy_symbol)
                }
            }
        }

        impl Drop for MockLoadedResolver {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        struct MockLoader {
            loaded: RefCell<Option<MockLoadedResolver>>,
        }

        impl CudaLibraryLoader for MockLoader {
            type Loaded = MockLoadedResolver;

            fn load(&self) -> Option<Self::Loaded> {
                self.loaded.borrow_mut().take()
            }
        }

        unsafe extern "system" fn dummy_symbol() -> isize {
            0
        }

        struct DispatchState {
            fail_stage: Option<CudaStage>,
            fail_operation: Option<&'static str>,
            null_operation: Option<&'static str>,
            device_count: i32,
            allocation_call: u32,
            allocation_failure_call: Option<u32>,
            input: Vec<u32>,
            mismatch: bool,
            jit_log: Vec<u8>,
            jit_overflow: bool,
            releases: Vec<u64>,
            current_context: bool,
            context_events: Vec<String>,
        }

        impl Default for DispatchState {
            fn default() -> Self {
                Self {
                    fail_stage: None,
                    fail_operation: None,
                    null_operation: None,
                    device_count: 1,
                    allocation_call: 0,
                    allocation_failure_call: None,
                    input: Vec::new(),
                    mismatch: false,
                    jit_log: Vec::new(),
                    jit_overflow: false,
                    releases: Vec::new(),
                    current_context: false,
                    context_events: Vec::new(),
                }
            }
        }

        fn state() -> &'static Mutex<DispatchState> {
            static STATE: OnceLock<Mutex<DispatchState>> = OnceLock::new();
            STATE.get_or_init(|| Mutex::new(DispatchState::default()))
        }

        fn serial() -> &'static Mutex<()> {
            static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
            SERIAL.get_or_init(|| Mutex::new(()))
        }

        fn reset() {
            *state().lock().unwrap() = DispatchState::default();
        }

        #[test]
        fn loader_and_required_symbol_resolution_fail_closed_and_drop_once() {
            let no_driver = MockLoader {
                loaded: RefCell::new(None),
            };
            assert_eq!(
                probe_with_loader(&no_driver).status,
                aexcompat_broker::cuda_compute_probe::CudaAggregateStatus::NoDriver
            );

            for symbol in [
                b"cuInit\0".as_slice(),
                b"cuDriverGetVersion\0".as_slice(),
                b"cuDeviceGetCount\0".as_slice(),
                b"cuDeviceGet\0".as_slice(),
                b"cuDeviceGetName\0".as_slice(),
                b"cuDeviceGetUuid_v2\0".as_slice(),
                b"cuDeviceGetAttribute\0".as_slice(),
                b"cuDeviceTotalMem_v2\0".as_slice(),
                b"cuCtxCreate_v2\0".as_slice(),
                b"cuCtxDestroy_v2\0".as_slice(),
                b"cuCtxGetCurrent\0".as_slice(),
                b"cuMemAlloc_v2\0".as_slice(),
                b"cuMemFree_v2\0".as_slice(),
                b"cuMemcpyHtoD_v2\0".as_slice(),
                b"cuMemcpyDtoH_v2\0".as_slice(),
                b"cuModuleLoadDataEx\0".as_slice(),
                b"cuModuleUnload\0".as_slice(),
                b"cuModuleGetFunction\0".as_slice(),
                b"cuLaunchKernel\0".as_slice(),
                b"cuCtxSynchronize\0".as_slice(),
            ] {
                let drops = Arc::new(AtomicUsize::new(0));
                let loader = MockLoader {
                    loaded: RefCell::new(Some(MockLoadedResolver {
                        missing: Some(symbol),
                        drops: Arc::clone(&drops),
                    })),
                };
                let result = probe_with_loader(&loader);
                assert_eq!(
                    result.status,
                    aexcompat_broker::cuda_compute_probe::CudaAggregateStatus::MissingSymbol,
                    "{symbol:?}"
                );
                assert_eq!(drops.load(Ordering::SeqCst), 1, "{symbol:?}");
            }
        }

        unsafe extern "system" fn init(_: u32) -> i32 {
            CUDA_SUCCESS
        }

        unsafe extern "system" fn driver_version(output: *mut i32) -> i32 {
            if state().lock().unwrap().fail_operation == Some("cuDriverGetVersion") {
                return 3;
            }
            unsafe { *output = 13_000 };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_count(output: *mut i32) -> i32 {
            let state = state().lock().unwrap();
            if state.fail_operation == Some("cuDeviceGetCount") {
                return 3;
            }
            unsafe { *output = state.device_count };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_get(output: *mut CuDevice, ordinal: i32) -> i32 {
            if state().lock().unwrap().fail_operation == Some("cuDeviceGet") {
                return 3;
            }
            unsafe { *output = ordinal };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_get_name(
            output: *mut i8,
            length: i32,
            _: CuDevice,
        ) -> i32 {
            if state().lock().unwrap().fail_operation == Some("cuDeviceGetName") {
                return 3;
            }
            let name = b"NVIDIA Fixture\0";
            assert!(length as usize >= name.len());
            unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), output.cast(), name.len()) };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_get_uuid(output: *mut CuUuid, _: CuDevice) -> i32 {
            if state().lock().unwrap().fail_operation == Some("cuDeviceGetUuid") {
                return 3;
            }
            unsafe { (*output).bytes = [7; 16] };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_get_attribute(
            output: *mut i32,
            attribute: i32,
            _: CuDevice,
        ) -> i32 {
            let failure = state().lock().unwrap().fail_operation;
            if matches!(
                (failure, attribute),
                (
                    Some("cuDeviceGetAttribute.pci_domain"),
                    CU_DEVICE_ATTRIBUTE_PCI_DOMAIN_ID
                ) | (
                    Some("cuDeviceGetAttribute.pci_bus"),
                    CU_DEVICE_ATTRIBUTE_PCI_BUS_ID
                ) | (
                    Some("cuDeviceGetAttribute.pci_device"),
                    CU_DEVICE_ATTRIBUTE_PCI_DEVICE_ID
                ) | (
                    Some("cuDeviceGetAttribute.compute_major"),
                    CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR
                ) | (
                    Some("cuDeviceGetAttribute.compute_minor"),
                    CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR
                )
            ) {
                return 3;
            }
            let value = match attribute {
                CU_DEVICE_ATTRIBUTE_PCI_DOMAIN_ID => 0,
                CU_DEVICE_ATTRIBUTE_PCI_BUS_ID => 1,
                CU_DEVICE_ATTRIBUTE_PCI_DEVICE_ID => 0,
                CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR => 8,
                CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR => 9,
                _ => return 1,
            };
            unsafe { *output = value };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn device_total_mem(output: *mut usize, _: CuDevice) -> i32 {
            if state().lock().unwrap().fail_operation == Some("cuDeviceTotalMem") {
                return 3;
            }
            unsafe { *output = 12 * 1024 * 1024 * 1024usize };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn ctx_get_current(output: *mut CuContext) -> i32 {
            let state = state().lock().unwrap();
            if state.fail_operation == Some("cuCtxGetCurrent") {
                return 201;
            }
            unsafe {
                *output = if state.current_context {
                    99usize as CuContext
                } else {
                    null_mut()
                }
            };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn ctx_create(
            output: *mut CuContext,
            _: u32,
            device: CuDevice,
        ) -> i32 {
            let mut state = state().lock().unwrap();
            if state.fail_stage == Some(CudaStage::Context) {
                return 201;
            }
            if state.null_operation == Some("cuCtxCreate_v2") {
                return CUDA_SUCCESS;
            }
            assert!(
                !state.current_context,
                "device context leaked across probes"
            );
            state.current_context = true;
            state.allocation_call = 0;
            state.context_events.push(format!("create:{device}"));
            unsafe { *output = (device as usize + 1) as CuContext };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn ctx_destroy(handle: CuContext) -> i32 {
            let mut state = state().lock().unwrap();
            state
                .context_events
                .push(format!("destroy:{}", handle as usize));
            state.current_context = false;
            state.releases.push(handle as usize as u64);
            CUDA_SUCCESS
        }

        unsafe extern "system" fn mem_alloc(output: *mut CuDevicePtr, _: usize) -> i32 {
            let mut state = state().lock().unwrap();
            state.allocation_call += 1;
            if state.fail_stage == Some(CudaStage::Allocation)
                && state
                    .allocation_failure_call
                    .is_none_or(|call| call == state.allocation_call)
            {
                return 2;
            }
            if (state.null_operation == Some("cuMemAlloc_v2.input") && state.allocation_call == 1)
                || (state.null_operation == Some("cuMemAlloc_v2.output")
                    && state.allocation_call == 2)
            {
                return CUDA_SUCCESS;
            }
            unsafe { *output = u64::from(state.allocation_call + 1) };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn mem_free(handle: CuDevicePtr) -> i32 {
            state().lock().unwrap().releases.push(handle);
            CUDA_SUCCESS
        }

        unsafe extern "system" fn memcpy_htod(
            _: CuDevicePtr,
            input: *const c_void,
            length: usize,
        ) -> i32 {
            let mut state = state().lock().unwrap();
            if state.fail_stage == Some(CudaStage::HostToDevice) {
                return 700;
            }
            state.input = unsafe {
                std::slice::from_raw_parts(input.cast::<u32>(), length / size_of::<u32>()).to_vec()
            };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn module_load_data_ex(
            output: *mut CuModule,
            image: *const c_void,
            option_count: u32,
            options: *mut i32,
            values: *mut *mut c_void,
        ) -> i32 {
            let ptx = unsafe { std::ffi::CStr::from_ptr(image.cast()).to_bytes().to_vec() };
            assert!(ptx.starts_with(b".version 6.0"));
            assert_eq!(option_count, 4);
            let option_slice =
                unsafe { std::slice::from_raw_parts(options, option_count as usize) };
            let value_slice = unsafe { std::slice::from_raw_parts(values, option_count as usize) };
            assert_eq!(
                option_slice,
                [
                    CU_JIT_INFO_LOG_BUFFER,
                    CU_JIT_INFO_LOG_BUFFER_SIZE_BYTES,
                    CU_JIT_ERROR_LOG_BUFFER,
                    CU_JIT_ERROR_LOG_BUFFER_SIZE_BYTES
                ]
            );
            let state = state().lock().unwrap();
            if matches!(state.fail_stage, Some(CudaStage::Module | CudaStage::Jit)) {
                if state.fail_stage == Some(CudaStage::Jit) {
                    let buffer = value_slice[2].cast::<u8>();
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            state.jit_log.as_ptr(),
                            buffer,
                            state.jit_log.len(),
                        );
                        *value_slice[3].cast::<u32>() = if state.jit_overflow {
                            (MAX_JIT_LOG_BYTES + 1) as u32
                        } else {
                            state.jit_log.len() as u32
                        };
                        *value_slice[1].cast::<u32>() = 0;
                    }
                    return CUDA_ERROR_INVALID_PTX;
                }
                return 1;
            }
            if state.null_operation == Some("cuModuleLoadDataEx") {
                return CUDA_SUCCESS;
            }
            drop(state);
            unsafe { *output = 4usize as CuModule };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn module_unload(_: CuModule) -> i32 {
            state().lock().unwrap().releases.push(4);
            CUDA_SUCCESS
        }

        unsafe extern "system" fn module_get_function(
            output: *mut CuFunction,
            _: CuModule,
            _: *const i8,
        ) -> i32 {
            if state().lock().unwrap().fail_stage == Some(CudaStage::Function) {
                return 500;
            }
            if state().lock().unwrap().null_operation == Some("cuModuleGetFunction") {
                return CUDA_SUCCESS;
            }
            unsafe { *output = 5usize as CuFunction };
            CUDA_SUCCESS
        }

        unsafe extern "system" fn launch_kernel(
            _: CuFunction,
            _: u32,
            _: u32,
            _: u32,
            block_x: u32,
            _: u32,
            _: u32,
            _: u32,
            _: CuStream,
            arguments: *mut *mut c_void,
            extra: *mut *mut c_void,
        ) -> i32 {
            assert_eq!(block_x, COMPUTE_ELEMENT_COUNT as u32);
            assert!(extra.is_null());
            let args = unsafe { std::slice::from_raw_parts(arguments, 2) };
            assert_eq!(unsafe { *args[0].cast::<u64>() }, 2);
            assert_eq!(unsafe { *args[1].cast::<u64>() }, 3);
            if state().lock().unwrap().fail_stage == Some(CudaStage::Launch) {
                719
            } else {
                CUDA_SUCCESS
            }
        }

        unsafe extern "system" fn ctx_synchronize() -> i32 {
            if state().lock().unwrap().fail_stage == Some(CudaStage::Synchronize) {
                719
            } else {
                CUDA_SUCCESS
            }
        }

        unsafe extern "system" fn memcpy_dtoh(
            output: *mut c_void,
            _: CuDevicePtr,
            length: usize,
        ) -> i32 {
            let state = state().lock().unwrap();
            if state.fail_stage == Some(CudaStage::DeviceToHost) {
                return 700;
            }
            let target = unsafe {
                std::slice::from_raw_parts_mut(output.cast::<u32>(), length / size_of::<u32>())
            };
            for (target, input) in target.iter_mut().zip(&state.input) {
                *target = *input * 3 + 7;
            }
            if state.mismatch {
                target[0] ^= 1;
            }
            CUDA_SUCCESS
        }

        fn dispatch() -> CudaFunctions {
            CudaFunctions {
                init,
                driver_get_version: driver_version,
                device_get_count: device_count,
                device_get,
                device_get_name,
                device_get_uuid,
                device_get_attribute,
                device_total_mem,
                ctx_create,
                ctx_destroy,
                ctx_get_current,
                mem_alloc,
                mem_free,
                memcpy_htod,
                memcpy_dtoh,
                module_load_data_ex,
                module_unload,
                module_get_function,
                launch_kernel,
                ctx_synchronize,
            }
        }

        fn metadata() -> CudaDeviceObservation {
            metadata_for(0)
        }

        fn metadata_for(ordinal: u32) -> CudaDeviceObservation {
            CudaDeviceObservation::passed(
                ordinal,
                "NVIDIA Fixture".into(),
                "ab".repeat(32),
                PciLocation {
                    domain: 0,
                    bus: 1,
                    device: 0,
                },
                8,
                9,
                12 * 1024 * 1024 * 1024,
            )
        }

        struct DispatchApi;

        impl CudaProbeApi for DispatchApi {
            fn initialize(&self) -> Result<(), ApiFailure> {
                cuda_result("cuInit", unsafe { (dispatch().init)(0) })
            }

            fn driver_version(&self) -> Result<u32, ApiFailure> {
                DynamicCuda {
                    _library: (),
                    functions: dispatch(),
                }
                .driver_version()
            }

            fn device_count(&self, _: usize) -> Result<usize, ApiFailure> {
                DynamicCuda {
                    _library: (),
                    functions: dispatch(),
                }
                .device_count(aexcompat_broker::cuda_compute_probe::MAX_CUDA_DEVICES)
            }

            fn probe_device(&self, ordinal: u32) -> CudaDeviceObservation {
                probe_device(dispatch(), ordinal)
            }
        }

        #[test]
        fn compute_dispatch_passes_and_releases_reverse_exactly_once() {
            let _guard = serial().lock().unwrap();
            reset();
            let result = run_compute(dispatch(), 0, metadata());
            assert_eq!(result.stage, CudaStage::Passed);
            assert_eq!(state().lock().unwrap().releases, [4, 3, 2, 1]);
        }

        #[test]
        fn every_failure_stage_is_fail_closed_with_owned_prefix_cleanup() {
            let _guard = serial().lock().unwrap();
            for (stage, expected_releases) in [
                (CudaStage::Context, Vec::new()),
                (CudaStage::Allocation, vec![1]),
                (CudaStage::HostToDevice, vec![3, 2, 1]),
                (CudaStage::Module, vec![3, 2, 1]),
                (CudaStage::Function, vec![4, 3, 2, 1]),
                (CudaStage::Launch, vec![4, 3, 2, 1]),
                (CudaStage::Synchronize, vec![4, 3, 2, 1]),
                (CudaStage::DeviceToHost, vec![4, 3, 2, 1]),
            ] {
                reset();
                state().lock().unwrap().fail_stage = Some(stage);
                let result = run_compute(dispatch(), 0, metadata());
                assert_eq!(result.stage, stage);
                assert_ne!(result.stage, CudaStage::Passed);
                assert_eq!(
                    state().lock().unwrap().releases,
                    expected_releases,
                    "stage {stage:?}"
                );
            }

            reset();
            {
                let mut state = state().lock().unwrap();
                state.fail_stage = Some(CudaStage::Allocation);
                state.allocation_failure_call = Some(2);
            }
            let second_allocation = run_compute(dispatch(), 0, metadata());
            assert_eq!(second_allocation.stage, CudaStage::Allocation);
            assert_eq!(
                second_allocation.operation.as_deref(),
                Some("cuMemAlloc_v2.output")
            );
            assert_eq!(state().lock().unwrap().releases, [2, 1]);
        }

        #[test]
        fn success_with_null_handles_is_structural_failure_through_parent_contract() {
            let _guard = serial().lock().unwrap();
            for (source, stage, operation, releases) in [
                (
                    "cuCtxCreate_v2",
                    CudaStage::Context,
                    "cuCtxCreate_v2.null_handle",
                    Vec::new(),
                ),
                (
                    "cuMemAlloc_v2.input",
                    CudaStage::Allocation,
                    "cuMemAlloc_v2.input.null_handle",
                    vec![1],
                ),
                (
                    "cuMemAlloc_v2.output",
                    CudaStage::Allocation,
                    "cuMemAlloc_v2.output.null_handle",
                    vec![2, 1],
                ),
                (
                    "cuModuleLoadDataEx",
                    CudaStage::Module,
                    "cuModuleLoadDataEx.null_handle",
                    vec![3, 2, 1],
                ),
                (
                    "cuModuleGetFunction",
                    CudaStage::Function,
                    "cuModuleGetFunction.null_handle",
                    vec![4, 3, 2, 1],
                ),
            ] {
                reset();
                state().lock().unwrap().null_operation = Some(source);
                let aggregate = collect_with_api(&DispatchApi);
                assert_eq!(
                    aggregate.status,
                    aexcompat_broker::cuda_compute_probe::CudaAggregateStatus::Failed
                );
                let observed = &aggregate.devices[0];
                assert_eq!(observed.stage, stage);
                assert_eq!(observed.operation.as_deref(), Some(operation));
                assert_eq!(observed.api_error, None);
                let json = serde_json::to_string(&aggregate).unwrap();
                let decoded: CudaAggregateObservation = serde_json::from_str(&json).unwrap();
                assert!(decoded.validate(), "{operation}");
                assert_eq!(state().lock().unwrap().releases, releases, "{operation}");
            }
        }

        #[test]
        fn metadata_failures_use_the_actual_probe_state_machine() {
            let _guard = serial().lock().unwrap();
            for operation in [
                "cuDeviceGet",
                "cuDeviceGetName",
                "cuDeviceGetUuid",
                "cuDeviceGetAttribute.pci_domain",
                "cuDeviceGetAttribute.pci_bus",
                "cuDeviceGetAttribute.pci_device",
                "cuDeviceGetAttribute.compute_major",
                "cuDeviceGetAttribute.compute_minor",
                "cuDeviceTotalMem",
            ] {
                reset();
                state().lock().unwrap().fail_operation = Some(operation);
                let result = probe_device(dispatch(), 0);
                assert_eq!(result.stage, CudaStage::Metadata, "{operation}");
                assert_eq!(result.operation.as_deref(), Some(operation), "{operation}");
                assert!(result.api_error.is_some(), "{operation}");
                assert!(state().lock().unwrap().releases.is_empty(), "{operation}");
            }
        }

        #[test]
        fn two_devices_destroy_each_owned_context_before_the_next_probe() {
            let _guard = serial().lock().unwrap();
            reset();
            state().lock().unwrap().device_count = 2;
            let aggregate = collect_with_api(&DispatchApi);
            assert_eq!(
                aggregate.status,
                aexcompat_broker::cuda_compute_probe::CudaAggregateStatus::Observed
            );
            assert!(aggregate.cuda_compute_ready);
            let state = state().lock().unwrap();
            assert_eq!(
                state.context_events,
                ["create:0", "destroy:1", "create:1", "destroy:2"]
            );
            assert!(!state.current_context);
        }

        #[test]
        fn jit_log_is_hash_only_and_overflow_is_distinct() {
            let _guard = serial().lock().unwrap();
            reset();
            {
                let mut state = state().lock().unwrap();
                state.fail_stage = Some(CudaStage::Jit);
                state.jit_log = b"C:\\Users\\name\\private compiler output".to_vec();
            }
            let result = run_compute(dispatch(), 0, metadata());
            assert_eq!(result.stage, CudaStage::Jit);
            let log = result.jit_log.expect("JIT hash evidence");
            assert_eq!(
                log.status,
                aexcompat_broker::cuda_compute_probe::JitLogStatus::Redacted
            );
            assert_eq!(log.sha256.unwrap().len(), 64);

            reset();
            {
                let mut state = state().lock().unwrap();
                state.fail_stage = Some(CudaStage::Jit);
                state.jit_log = b"bounded".to_vec();
                state.jit_overflow = true;
            }
            let overflow = run_compute(dispatch(), 0, metadata());
            assert_eq!(
                overflow.jit_log.unwrap().status,
                aexcompat_broker::cuda_compute_probe::JitLogStatus::Overflow
            );
        }

        #[test]
        fn readback_mismatch_is_not_success_and_still_cleans_up() {
            let _guard = serial().lock().unwrap();
            reset();
            state().lock().unwrap().mismatch = true;
            let result = run_compute(dispatch(), 0, metadata());
            assert_eq!(result.stage, CudaStage::Mismatch);
            assert_eq!(result.api_error, None);
            assert_eq!(state().lock().unwrap().releases, [4, 3, 2, 1]);
        }

        #[test]
        fn embedded_ptx_and_driver_pointer_abi_are_fixed_and_bounded() {
            assert!(EMBEDDED_PTX.ends_with(b"\0"));
            assert!(EMBEDDED_PTX.len() < 4096);
            let affine_instruction = b"mad.lo.u32 %r3";
            assert!(EMBEDDED_PTX
                .windows(affine_instruction.len())
                .any(|item| item == affine_instruction));
            assert_eq!(size_of::<CuDevicePtr>(), 8);
            assert_eq!(COMPUTE_ELEMENT_COUNT, 64);
        }
    }
}
