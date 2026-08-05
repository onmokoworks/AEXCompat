#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GpuImportLibrary {
    OpenCl,
    Cuda,
    DirectX,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenClBridgeSymbol {
    CreateProgramWithSource,
    BuildProgram,
    CreateKernel,
    SetKernelArg,
    EnqueueNdRangeKernel,
    ReleaseKernel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyWin64Import {
    Malloc,
    Calloc,
    Free,
    CallNewHandler,
    Strncpy,
    Memset,
    MemoryCopy,
    MemChr,
    StdioVsnprintfS,
    MsvcpMutexInit,
    MsvcpMutexLock,
    MsvcpMutexUnlock,
    MsvcpMutexDestroy,
    MsvcpHardwareConcurrency,
    VcruntimeExceptionCopy,
    VcruntimeExceptionDestroy,
    CxxThrowException,
    CosF,
    ExpF,
    FloorF,
    PowF,
    Pow,
    SinF,
    OmpGetMaxThreads,
    OmpSetDynamic,
    VcompSetNumThreads,
    VcompFork,
    VcompForDynamicInit,
    VcompForDynamicNext,
    VcompForStaticSimpleInit,
    VcompNoOp,
    GetSystemTimeAsFileTime,
    GetCurrentThreadId,
    GetCurrentProcessId,
    QueryPerformanceCounter,
    ExplicitMsvcRuntimeZero,
    CrtInitterm,
    CrtInittermE,
    CrtInitializeOnexitTable,
    CrtRegisterOnexitFunction,
    CrtExecuteOnexitTable,
    CrtGetenv,
    InitializeCriticalSection,
    InitializeCriticalSectionAndSpinCount,
    EnterCriticalSection,
    LeaveCriticalSection,
    DeleteCriticalSection,
    GetModuleHandleW,
    GetProcAddress,
    InitializeSListHead,
    DisableThreadLibraryCalls,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Win64ImportDispatch {
    LegacyImplemented(LegacyWin64Import),
    OpenClBridge(OpenClBridgeSymbol),
    UnsupportedGpuLibrary(GpuImportLibrary),
    UnsupportedLegacyImport,
    UnsupportedVcomp,
}

const MAX_WIN64_IMPORT_ARGUMENTS: usize = 12;

// Keep CUDA classification on explicit DLL names and versioned family stems.
// A broad `cu*`/`nv*` rule would turn unrelated imports into GPU traps.
const EXACT_CUDA_DLLS: &[&str] = &[
    "cuda.dll",
    "nvcuda.dll",
    "nvjitlink.dll",
    "nvfatbin.dll",
    "nvblas.dll",
    "cupti.dll",
];

const VERSIONED_CUDA_DLL_FAMILIES: &[&str] = &["nvjitlink", "nvfatbin"];

const VERSIONED_CUDA_64_DLL_FAMILIES: &[&str] = &[
    "cudart",
    "nvrtc",
    "nvrtc-builtins",
    "cublas",
    "cublaslt",
    "cufft",
    "cufftw",
    "curand",
    "cusolver",
    "cusolvermg",
    "cusparse",
    "nvjpeg",
    "nvtoolsext",
    "cupti",
    "nvvm",
    "nvblas",
];

const VERSIONED_NPP_64_DLL_FAMILIES: &[&str] = &[
    "npp", "nppi", "nppc", "nppial", "nppicc", "nppicom", "nppidei", "nppif", "nppig", "nppim",
    "nppist", "nppisu", "nppitc", "npps",
];

fn normalize_import_library_name(library: &str) -> String {
    library
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn version_suffix_is_numeric(version: &str) -> bool {
    !version.is_empty()
        && version
            .split(['_', '.'])
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn cuda_64_version_suffix<'a>(library: &'a str, family: &str) -> Option<&'a str> {
    library
        .strip_suffix(".dll")?
        .strip_prefix(family)?
        .strip_prefix("64_")
}

fn is_versioned_cuda_dll(library: &str, family: &str) -> bool {
    cuda_64_version_suffix(library, family).is_some_and(version_suffix_is_numeric)
}

fn is_versioned_cuda_alt_dll(library: &str, family: &str) -> bool {
    cuda_64_version_suffix(library, family)
        .and_then(|version| version.strip_suffix(".alt"))
        .is_some_and(version_suffix_is_numeric)
}

fn is_versioned_cuda_dll_without_arch(library: &str, family: &str) -> bool {
    library
        .strip_suffix(".dll")
        .and_then(|stem| stem.strip_prefix(family))
        .and_then(|suffix| suffix.strip_prefix('_'))
        .is_some_and(version_suffix_is_numeric)
}

fn is_versioned_cudnn_dll(library: &str) -> bool {
    if is_versioned_cuda_dll(library, "cudnn") {
        return true;
    }
    let Some(stem) = library.strip_suffix(".dll") else {
        return false;
    };
    let Some(component_and_version) = stem.strip_prefix("cudnn_") else {
        return false;
    };
    let Some((component, version)) = component_and_version.rsplit_once("64_") else {
        return false;
    };
    !component.is_empty()
        && component
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && version_suffix_is_numeric(version)
}

fn classify_gpu_import_library(library: &str) -> Option<GpuImportLibrary> {
    let library = normalize_import_library_name(library);
    if library == "opencl.dll" {
        return Some(GpuImportLibrary::OpenCl);
    }
    if EXACT_CUDA_DLLS.contains(&library.as_str())
        || VERSIONED_CUDA_DLL_FAMILIES
            .iter()
            .any(|family| is_versioned_cuda_dll_without_arch(&library, family))
        || VERSIONED_CUDA_64_DLL_FAMILIES
            .iter()
            .any(|family| is_versioned_cuda_dll(&library, family))
        || VERSIONED_NPP_64_DLL_FAMILIES
            .iter()
            .any(|family| is_versioned_cuda_dll(&library, family))
        || is_versioned_cuda_alt_dll(&library, "nvrtc")
        || is_versioned_cudnn_dll(&library)
    {
        return Some(GpuImportLibrary::Cuda);
    }
    if matches!(
        library.as_str(),
        "d3d9.dll"
            | "d3d10.dll"
            | "d3d10_1.dll"
            | "d3d11.dll"
            | "d3d12.dll"
            | "dxgi.dll"
            | "dxcore.dll"
            | "dxcompiler.dll"
            | "dxil.dll"
            | "directml.dll"
    ) || (library.starts_with("d3dcompiler_") && library.ends_with(".dll"))
    {
        return Some(GpuImportLibrary::DirectX);
    }
    None
}

fn opencl_bridge_symbol(symbol: &str) -> Option<OpenClBridgeSymbol> {
    match symbol {
        "clCreateProgramWithSource" => Some(OpenClBridgeSymbol::CreateProgramWithSource),
        "clBuildProgram" => Some(OpenClBridgeSymbol::BuildProgram),
        "clCreateKernel" => Some(OpenClBridgeSymbol::CreateKernel),
        "clSetKernelArg" => Some(OpenClBridgeSymbol::SetKernelArg),
        "clEnqueueNDRangeKernel" => Some(OpenClBridgeSymbol::EnqueueNdRangeKernel),
        "clReleaseKernel" => Some(OpenClBridgeSymbol::ReleaseKernel),
        _ => None,
    }
}

fn dispatch_win64_import(library: &str, symbol: &str) -> Win64ImportDispatch {
    // GPU libraries take precedence over legacy symbol-only emulation. This
    // prevents, for example, opencl.dll!malloc from accidentally receiving the
    // CRT allocator solely because its export name happens to match.
    let normalized_library = normalize_import_library_name(library);
    match classify_gpu_import_library(&normalized_library) {
        Some(GpuImportLibrary::OpenCl) => {
            return opencl_bridge_symbol(symbol)
                .map(Win64ImportDispatch::OpenClBridge)
                .unwrap_or(Win64ImportDispatch::UnsupportedGpuLibrary(
                    GpuImportLibrary::OpenCl,
                ));
        }
        Some(library) => return Win64ImportDispatch::UnsupportedGpuLibrary(library),
        None => {}
    }
    let legacy = match (normalized_library.as_str(), symbol) {
        ("kernel32.dll", "GetSystemTimeAsFileTime") => {
            LegacyWin64Import::GetSystemTimeAsFileTime
        }
        ("kernel32.dll", "GetCurrentThreadId") => LegacyWin64Import::GetCurrentThreadId,
        ("kernel32.dll", "GetCurrentProcessId") => LegacyWin64Import::GetCurrentProcessId,
        ("kernel32.dll", "QueryPerformanceCounter") => {
            LegacyWin64Import::QueryPerformanceCounter
        }
        ("kernel32.dll", "InitializeSListHead") => LegacyWin64Import::InitializeSListHead,
        ("kernel32.dll", "DisableThreadLibraryCalls") => {
            LegacyWin64Import::DisableThreadLibraryCalls
        }
        ("kernel32.dll", "InitializeCriticalSection") => {
            LegacyWin64Import::InitializeCriticalSection
        }
        ("kernel32.dll", "InitializeCriticalSectionAndSpinCount") => {
            LegacyWin64Import::InitializeCriticalSectionAndSpinCount
        }
        ("kernel32.dll", "EnterCriticalSection") => LegacyWin64Import::EnterCriticalSection,
        ("kernel32.dll", "LeaveCriticalSection") => LegacyWin64Import::LeaveCriticalSection,
        ("kernel32.dll", "DeleteCriticalSection") => LegacyWin64Import::DeleteCriticalSection,
        ("kernel32.dll", "GetModuleHandleW") => LegacyWin64Import::GetModuleHandleW,
        ("kernel32.dll", "GetProcAddress") => LegacyWin64Import::GetProcAddress,
        (
            _,
            "InitializeCriticalSection"
            | "InitializeCriticalSectionAndSpinCount"
            | "EnterCriticalSection"
            | "LeaveCriticalSection"
            | "DeleteCriticalSection",
        ) => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-runtime-l1-1-0.dll", symbol)
            if matches!(
                symbol,
                "_initialize_narrow_environment"
                    | "_configure_narrow_argv"
                    | "_cexit"
            ) =>
        {
            LegacyWin64Import::ExplicitMsvcRuntimeZero
        }
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_initialize_onexit_table") => {
            LegacyWin64Import::CrtInitializeOnexitTable
        }
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_register_onexit_function") => {
            LegacyWin64Import::CrtRegisterOnexitFunction
        }
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_execute_onexit_table") => {
            LegacyWin64Import::CrtExecuteOnexitTable
        }
        (_, "_initialize_onexit_table" | "_register_onexit_function" | "_execute_onexit_table") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-environment-l1-1-0.dll", "getenv") => LegacyWin64Import::CrtGetenv,
        (_, "getenv") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_initterm") => LegacyWin64Import::CrtInitterm,
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_initterm_e") => {
            LegacyWin64Import::CrtInittermE
        }
        (_, "_initterm" | "_initterm_e") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcomp140.dll", "_vcomp_set_num_threads") => LegacyWin64Import::VcompSetNumThreads,
        ("vcomp140.dll", "omp_set_dynamic") => LegacyWin64Import::OmpSetDynamic,
        (_, "omp_set_dynamic") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (_, "_vcomp_set_num_threads") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcruntime140.dll", "memchr") => LegacyWin64Import::MemChr,
        (_, "memchr") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll", "__stdio_common_vsnprintf_s") => {
            LegacyWin64Import::StdioVsnprintfS
        }
        (_, "__stdio_common_vsnprintf_s") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("msvcp140.dll", "_Mtx_init_in_situ") => LegacyWin64Import::MsvcpMutexInit,
        ("msvcp140.dll", "_Mtx_lock") => LegacyWin64Import::MsvcpMutexLock,
        ("msvcp140.dll", "_Mtx_unlock") => LegacyWin64Import::MsvcpMutexUnlock,
        ("msvcp140.dll", "_Mtx_destroy_in_situ") => LegacyWin64Import::MsvcpMutexDestroy,
        ("msvcp140.dll", "_Thrd_hardware_concurrency") => {
            LegacyWin64Import::MsvcpHardwareConcurrency
        }
        (_, symbol)
            if matches!(
                symbol,
                "_Mtx_init_in_situ"
                    | "_Mtx_lock"
                    | "_Mtx_unlock"
                    | "_Mtx_destroy_in_situ"
                    | "_Thrd_hardware_concurrency"
            ) =>
        {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcruntime140.dll", "__std_exception_copy") => {
            LegacyWin64Import::VcruntimeExceptionCopy
        }
        ("vcruntime140.dll", "__std_exception_destroy") => {
            LegacyWin64Import::VcruntimeExceptionDestroy
        }
        (_, "__std_exception_copy" | "__std_exception_destroy") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        (_, symbol) => match symbol {
            "malloc" => LegacyWin64Import::Malloc,
            "calloc" => LegacyWin64Import::Calloc,
            "free" => LegacyWin64Import::Free,
            "_callnewh" => LegacyWin64Import::CallNewHandler,
            "strncpy" => LegacyWin64Import::Strncpy,
            "memset" => LegacyWin64Import::Memset,
            "memcpy" | "memmove" => LegacyWin64Import::MemoryCopy,
            "_CxxThrowException" => LegacyWin64Import::CxxThrowException,
            "cosf" => LegacyWin64Import::CosF,
            "expf" => LegacyWin64Import::ExpF,
            "floorf" => LegacyWin64Import::FloorF,
            "powf" => LegacyWin64Import::PowF,
            "pow" => LegacyWin64Import::Pow,
            "sinf" => LegacyWin64Import::SinF,
            "omp_get_max_threads" => LegacyWin64Import::OmpGetMaxThreads,
            "_vcomp_fork" => LegacyWin64Import::VcompFork,
            "_vcomp_for_dynamic_init" => LegacyWin64Import::VcompForDynamicInit,
            "_vcomp_for_dynamic_next" => LegacyWin64Import::VcompForDynamicNext,
            "_vcomp_for_static_simple_init" => LegacyWin64Import::VcompForStaticSimpleInit,
            "_vcomp_enter_critsect"
            | "_vcomp_leave_critsect"
            | "_vcomp_barrier"
            | "_vcomp_for_static_end" => LegacyWin64Import::VcompNoOp,
            name if name.starts_with("_vcomp_") => {
                return Win64ImportDispatch::UnsupportedVcomp;
            }
            name if msvc_udt_by_value_return_import(name) => {
                return Win64ImportDispatch::UnsupportedLegacyImport;
            }
            _ => return Win64ImportDispatch::UnsupportedLegacyImport,
        },
    };
    Win64ImportDispatch::LegacyImplemented(legacy)
}

fn canonical_import_trace_label(library: &str, symbol: &str) -> String {
    format!("{}!{symbol}", normalize_import_library_name(library))
}

fn install_win64_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    stub: u64,
    library: &str,
    symbol: &str,
) -> Result<Win64ImportDispatch, GuestError> {
    let dispatch = dispatch_win64_import(library, symbol);
    match dispatch {
        Win64ImportDispatch::LegacyImplemented(implementation) => match implementation {
            LegacyWin64Import::Malloc => {
                uc("write malloc return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install malloc import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_malloc(unicorn, false);
                    }),
                )?;
            }
            LegacyWin64Import::Calloc => {
                uc("write calloc return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install calloc import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_malloc(unicorn, true);
                    }),
                )?;
            }
            LegacyWin64Import::Free => {
                uc("write free return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install free import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_free(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::CallNewHandler => {
                // No new-handler is installed by this bounded host. Returning
                // zero tells the MSVC allocation path not to retry.
            }
            LegacyWin64Import::Strncpy => {
                uc(
                    "install strncpy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_strncpy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::Memset => {
                uc(
                    "install memset import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_memset(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MemoryCopy => {
                uc(
                    "write CRT memory-copy return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install CRT memory-copy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_memory_copy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MemChr => {
                uc("write memchr return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install memchr import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_memchr(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::StdioVsnprintfS => {
                uc("write stdio formatter return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install stdio formatter import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_stdio_common_vsnprintf_s(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpMutexInit => {
                uc("write mutex init return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install mutex init import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_msvcp_mutex_init(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpMutexLock => {
                uc("write mutex lock return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install mutex lock import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_msvcp_mutex_lock(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpMutexUnlock => {
                uc("write mutex unlock return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install mutex unlock import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_msvcp_mutex_unlock(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpMutexDestroy => {
                uc("write mutex destroy return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install mutex destroy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_msvcp_mutex_destroy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpHardwareConcurrency => {
                uc(
                    "install deterministic hardware concurrency import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(1)),
                )?;
            }
            LegacyWin64Import::VcruntimeExceptionCopy => {
                uc("write exception copy return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install exception copy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcruntime_exception_copy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcruntimeExceptionDestroy => {
                uc("write exception destroy return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install exception destroy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcruntime_exception_destroy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::CxxThrowException => {
                uc("write C++ throw trap", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install C++ throw trap",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_cxx_throw_exception(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::CosF => {
                install_float_import(unicorn, stub, "cosf", f32::cos)?;
            }
            LegacyWin64Import::ExpF => {
                install_float_import(unicorn, stub, "expf", f32::exp)?;
            }
            LegacyWin64Import::FloorF => {
                install_float_import(unicorn, stub, "floorf", f32::floor)?;
            }
            LegacyWin64Import::PowF => {
                install_float_binary_import(unicorn, stub, "powf", f32::powf)?;
            }
            LegacyWin64Import::Pow => {
                install_double_binary_import(unicorn, stub, "pow", f64::powf)?;
            }
            LegacyWin64Import::SinF => {
                install_float_import(unicorn, stub, "sinf", f32::sin)?;
            }
            LegacyWin64Import::OmpGetMaxThreads => {
                let value = deterministic_import_i32("omp_get_max_threads")
                    .expect("known deterministic import");
                uc(
                    "install omp_get_max_threads import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(value)),
                )?;
            }
            LegacyWin64Import::OmpSetDynamic => {
                uc("write omp_set_dynamic return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install omp_set_dynamic import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        let enabled = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
                        if enabled > 1 {
                            unicorn.get_data_mut().callback_error = Some(format!(
                                "OpenMP dynamic scheduling request {enabled} is invalid; expected 0 or 1"
                            ));
                            let _ = unicorn.emu_stop();
                        } else {
                            // Record the guest policy while retaining the
                            // deterministic single-worker execution model.
                            unicorn.get_data_mut().omp_dynamic_requested = Some(enabled != 0);
                        }
                        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                    }),
                )?;
            }
            LegacyWin64Import::VcompSetNumThreads => {
                uc(
                    "write _vcomp_set_num_threads return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _vcomp_set_num_threads import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcomp_set_num_threads(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcompFork => {
                // Marshal captured arguments, then tail-jump into the outlined
                // worker so it returns directly to the caller.
                uc(
                    "write _vcomp_fork tail jump",
                    unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                )?;
                uc(
                    "install _vcomp_fork import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcomp_fork(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcompForDynamicInit => {
                uc(
                    "write _vcomp_for_dynamic_init return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _vcomp_for_dynamic_init import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcomp_for_dynamic_init(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcompForDynamicNext => {
                uc(
                    "write _vcomp_for_dynamic_next return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _vcomp_for_dynamic_next import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcomp_for_dynamic_next(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcompForStaticSimpleInit => {
                uc(
                    "write _vcomp_for_static_simple_init return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _vcomp_for_static_simple_init import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcomp_for_static_simple_init(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcompNoOp => {
                // The worker is deliberately single-threaded, so these
                // synchronization/end helpers are deterministic no-ops.
            }
            LegacyWin64Import::GetSystemTimeAsFileTime => {
                uc(
                    "write GetSystemTimeAsFileTime return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetSystemTimeAsFileTime import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_system_time_as_file_time(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetCurrentThreadId | LegacyWin64Import::GetCurrentProcessId => {
                uc(
                    "install deterministic Windows identity import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(1)),
                )?;
            }
            LegacyWin64Import::QueryPerformanceCounter => {
                uc(
                    "write QueryPerformanceCounter return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install QueryPerformanceCounter import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_query_performance_counter(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::ExplicitMsvcRuntimeZero => {
                // This finite allowlist mirrors the prior deterministic-zero
                // behavior for CRT startup/teardown only. Every other unknown
                // import remains a typed trap.
            }
            LegacyWin64Import::CrtInitterm | LegacyWin64Import::CrtInittermE => {
                uc("write CRT initializer tail jump", unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]))?;
                let stop_on_error = implementation == LegacyWin64Import::CrtInittermE;
                uc(
                    "install CRT initializer import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_crt_initterm(unicorn, stop_on_error);
                    }),
                )?;
            }
            LegacyWin64Import::CrtInitializeOnexitTable
            | LegacyWin64Import::CrtRegisterOnexitFunction => {
                uc("write CRT onexit return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install CRT onexit import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_crt_onexit(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::CrtExecuteOnexitTable => {
                uc("write CRT onexit tail jump", unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]))?;
                uc(
                    "install CRT onexit execution import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_crt_onexit(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::CrtGetenv => {
                uc("write deterministic getenv return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install deterministic getenv import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_getenv(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::InitializeCriticalSection
            | LegacyWin64Import::InitializeCriticalSectionAndSpinCount
            | LegacyWin64Import::EnterCriticalSection
            | LegacyWin64Import::LeaveCriticalSection
            | LegacyWin64Import::DeleteCriticalSection => {
                uc("write critical-section return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install critical-section import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_windows_critical_section(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::GetModuleHandleW => {
                uc("write GetModuleHandleW return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install GetModuleHandleW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_module_handle_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetProcAddress => {
                uc("write GetProcAddress return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install GetProcAddress import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        let module = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
                        let pointer = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
                        let mut bytes = Vec::new();
                        let mut terminated = false;
                        for index in 0..128u64 {
                            let Some(address) = pointer.checked_add(index) else {
                                break;
                            };
                            let Ok(value) = unicorn.mem_read_as_vec(address, 1) else {
                                break;
                            };
                            if value[0] == 0 {
                                terminated = true;
                                break;
                            }
                            bytes.push(value[0]);
                        }
                        let dynamic = match bytes.as_slice() {
                            b"InitializeConditionVariable" => {
                                Some(HOST_INITIALIZE_CONDITION_VARIABLE)
                            }
                            b"SleepConditionVariableCS" => Some(HOST_SLEEP_CONDITION_VARIABLE_CS),
                            b"WakeConditionVariable" => Some(HOST_WAKE_CONDITION_VARIABLE),
                            b"WakeAllConditionVariable" => Some(HOST_WAKE_ALL_CONDITION_VARIABLE),
                            _ => None,
                        };
                        if module == WINDOWS_KERNEL32_MODULE_TOKEN
                            && terminated
                            && let Some(dynamic) = dynamic
                        {
                            let _ = unicorn.reg_write(RegisterX86::RAX, dynamic);
                        } else {
                            if unicorn.get_data().callback_error.is_none() {
                                unicorn.get_data_mut().callback_error = Some(format!(
                                    "GetProcAddress module={module:#x} name={:?} is unsupported",
                                    String::from_utf8_lossy(&bytes)
                                ));
                            }
                            let _ = unicorn.emu_stop();
                        }
                    }),
                )?;
            }
            LegacyWin64Import::InitializeSListHead => {
                uc(
                    "write InitializeSListHead return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install InitializeSListHead import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_initialize_slist_head(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::DisableThreadLibraryCalls => {
                uc(
                    "install deterministic DisableThreadLibraryCalls import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(1)),
                )?;
            }
        },
        Win64ImportDispatch::OpenClBridge(symbol) => {
            install_opencl_import_bridge(unicorn, stub, symbol)?;
        }
        Win64ImportDispatch::UnsupportedGpuLibrary(_)
        | Win64ImportDispatch::UnsupportedLegacyImport => {
            install_unsupported_import_trap(
                unicorn,
                stub,
                normalize_import_library_name(library),
                symbol.to_string(),
            )?;
        }
        Win64ImportDispatch::UnsupportedVcomp => {
            return Err(GuestError::Callback(format!(
                "unsupported VCOMP import: {symbol}"
            )));
        }
    }
    Ok(dispatch)
}

fn fail_crt_initterm(unicorn: &mut Unicorn<'_, GuestState>, error: String) {
    unicorn.get_data_mut().pending_crt_initterm = None;
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(error);
    }
    let _ = unicorn.emu_stop();
}

fn dispatch_crt_initializer(
    unicorn: &mut Unicorn<'_, GuestState>,
    function: u64,
    stack_pointer: u64,
) -> Result<(), String> {
    let call_rsp = stack_pointer
        .checked_sub(8)
        .ok_or_else(|| "CRT initializer stack underflow".to_string())?;
    unicorn
        .mem_write(call_rsp, &HOST_CRT_INITTERM_CONTINUE.to_le_bytes())
        .map_err(|error| format!("CRT initializer continuation stack write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RSP, call_rsp)
        .map_err(|error| format!("CRT initializer stack register write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::R11, function)
        .map_err(|error| format!("CRT initializer target register write failed: {error}"))
}

fn finish_crt_initterm(
    unicorn: &mut Unicorn<'_, GuestState>,
    pending: PendingCrtInitterm,
    returned: u64,
) -> Result<(), String> {
    unicorn
        .reg_write(RegisterX86::RSP, pending.continuation_rsp)
        .map_err(|error| format!("CRT initializer final stack write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::R11, pending.return_address)
        .map_err(|error| format!("CRT initializer return target write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RAX, returned)
        .map_err(|error| format!("CRT initializer return value write failed: {error}"))
}

fn start_crt_function_sequence(
    unicorn: &mut Unicorn<'_, GuestState>,
    functions: Vec<u64>,
    stop_on_error: bool,
) -> Result<(), String> {
    if unicorn.get_data().pending_crt_initterm.is_some() {
        return Err("nested CRT function sequence is unsupported".into());
    }
    let rsp = unicorn
        .reg_read(RegisterX86::RSP)
        .map_err(|error| format!("CRT function-sequence stack read failed: {error}"))?;
    let return_address = read_vcomp_u64(unicorn, rsp)
        .map_err(|error| format!("CRT function-sequence return address read failed: {error}"))?;
    if return_address != RETURN_ADDRESS
        && !image_executable_address(unicorn.get_data(), return_address)
    {
        return Err(format!(
            "CRT function-sequence caller return {return_address:#x} is outside the executable image"
        ));
    }
    let continuation_rsp = rsp
        .checked_add(8)
        .ok_or_else(|| "CRT function-sequence continuation stack overflow".to_string())?;
    let mut pending = PendingCrtInitterm {
        functions,
        next: 0,
        return_address,
        continuation_rsp,
        stop_on_error,
    };
    let Some(function) = pending.functions.first().copied() else {
        return finish_crt_initterm(unicorn, pending, 0);
    };
    pending.next = 1;
    unicorn.get_data_mut().pending_crt_initterm = Some(pending);
    dispatch_crt_initializer(unicorn, function, continuation_rsp)
}

fn emulate_crt_initterm(unicorn: &mut Unicorn<'_, GuestState>, stop_on_error: bool) {
    let result = (|| -> Result<(), String> {
        let first = read_win64_import_argument(unicorn, 0)?;
        let last = read_win64_import_argument(unicorn, 1)?;
        if first % 8 != 0 || last % 8 != 0 {
            return Err(format!(
                "CRT initializer table bounds {first:#x}..{last:#x} are not pointer-aligned"
            ));
        }
        let byte_count = last.checked_sub(first).ok_or_else(|| {
            format!("CRT initializer table end {last:#x} precedes start {first:#x}")
        })?;
        if byte_count % 8 != 0 {
            return Err("CRT initializer table byte count is not divisible by 8".into());
        }
        let entry_count = usize::try_from(byte_count / 8)
            .map_err(|_| "CRT initializer count does not fit usize".to_string())?;
        if entry_count > MAX_CRT_INITIALIZERS {
            return Err(format!(
                "CRT initializer count {entry_count} exceeds {MAX_CRT_INITIALIZERS}"
            ));
        }
        let bytes = unicorn
            .mem_read_as_vec(first, entry_count * 8)
            .map_err(|error| format!("CRT initializer table read failed: {error}"))?;
        let mut functions = Vec::with_capacity(entry_count);
        for (index, chunk) in bytes.chunks_exact(8).enumerate() {
            let function = u64::from_le_bytes(
                chunk
                    .try_into()
                    .map_err(|_| "CRT initializer entry has wrong size".to_string())?,
            );
            if function == 0 {
                continue;
            }
            if !image_executable_address(unicorn.get_data(), function) {
                return Err(format!(
                    "CRT initializer entry {} target {function:#x} is outside the executable image",
                    index + 1
                ));
            }
            functions.push(function);
        }

        start_crt_function_sequence(unicorn, functions, stop_on_error)
    })();
    if let Err(error) = result {
        fail_crt_initterm(unicorn, error);
    }
}

fn continue_crt_initterm(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| -> Result<(), String> {
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("CRT initializer continuation stack read failed: {error}"))?;
        let returned = unicorn
            .reg_read(RegisterX86::RAX)
            .map_err(|error| format!("CRT initializer result read failed: {error}"))?;
        let mut pending = unicorn
            .get_data_mut()
            .pending_crt_initterm
            .take()
            .ok_or_else(|| "CRT initializer continuation has no pending table".to_string())?;
        if rsp != pending.continuation_rsp {
            return Err(format!(
                "CRT initializer continuation stack {rsp:#x} does not match {:#x}",
                pending.continuation_rsp
            ));
        }
        if pending.stop_on_error && returned as u32 != 0 {
            return finish_crt_initterm(unicorn, pending, returned as u32 as u64);
        }
        if let Some(function) = pending.functions.get(pending.next).copied() {
            pending.next += 1;
            let continuation_rsp = pending.continuation_rsp;
            unicorn.get_data_mut().pending_crt_initterm = Some(pending);
            return dispatch_crt_initializer(unicorn, function, continuation_rsp);
        }
        finish_crt_initterm(unicorn, pending, 0)
    })();
    if let Err(error) = result {
        fail_crt_initterm(unicorn, error);
    }
}

fn emulate_crt_onexit(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<(), String> {
        let table = read_win64_import_argument(unicorn, 0)?;
        if table == 0 {
            return Err("CRT onexit table pointer is null".into());
        }
        match operation {
            LegacyWin64Import::CrtInitializeOnexitTable => {
                if unicorn.get_data().crt_onexit_tables.contains_key(&table) {
                    return Err(format!("CRT onexit table {table:#x} is already initialized"));
                }
                if unicorn.get_data().crt_onexit_tables.len() >= MAX_CRT_ONEXIT_TABLES {
                    return Err(format!(
                        "CRT onexit table count exceeds {MAX_CRT_ONEXIT_TABLES}"
                    ));
                }
                unicorn.mem_write(table, &[0; 24]).map_err(|error| {
                    format!("CRT onexit table {table:#x} is not writable: {error}")
                })?;
                unicorn
                    .get_data_mut()
                    .crt_onexit_tables
                    .insert(table, Vec::new());
                unicorn
                    .reg_write(RegisterX86::RAX, 0)
                    .map_err(|error| format!("CRT onexit initialize result write failed: {error}"))
            }
            LegacyWin64Import::CrtRegisterOnexitFunction => {
                let function = read_win64_import_argument(unicorn, 1)?;
                if function == 0 || !image_executable_address(unicorn.get_data(), function) {
                    return Err(format!(
                        "CRT onexit function {function:#x} is outside the executable image"
                    ));
                }
                let functions = unicorn
                    .get_data_mut()
                    .crt_onexit_tables
                    .get_mut(&table)
                    .ok_or_else(|| format!("CRT onexit table {table:#x} is not initialized"))?;
                if functions.len() >= MAX_CRT_INITIALIZERS {
                    return Err(format!(
                        "CRT onexit function count exceeds {MAX_CRT_INITIALIZERS}"
                    ));
                }
                functions.push(function);
                unicorn
                    .reg_write(RegisterX86::RAX, 0)
                    .map_err(|error| format!("CRT onexit register result write failed: {error}"))
            }
            LegacyWin64Import::CrtExecuteOnexitTable => {
                let mut functions = unicorn
                    .get_data_mut()
                    .crt_onexit_tables
                    .remove(&table)
                    .ok_or_else(|| format!("CRT onexit table {table:#x} is not initialized"))?;
                functions.reverse();
                start_crt_function_sequence(unicorn, functions, false)
            }
            _ => Err("invalid CRT onexit operation".into()),
        }
    })();
    if let Err(error) = result {
        fail_crt_initterm(unicorn, error);
    }
}

fn emulate_crt_getenv(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        if pointer == 0 {
            return Err("CRT getenv name pointer is null".into());
        }
        let mut terminated = false;
        for index in 0..256u64 {
            let address = pointer
                .checked_add(index)
                .ok_or_else(|| "CRT getenv name address overflow".to_string())?;
            let byte = unicorn
                .mem_read_as_vec(address, 1)
                .map_err(|error| format!("CRT getenv name read failed: {error}"))?[0];
            if byte == 0 {
                terminated = true;
                break;
            }
        }
        if !terminated {
            return Err("CRT getenv name exceeds 255 bytes".into());
        }
        // Do not leak the macOS host environment into the deterministic guest.
        unicorn
            .reg_write(RegisterX86::RAX, 0)
            .map_err(|error| format!("CRT getenv return write failed: {error}"))
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_windows_critical_section(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    let result = (|| -> Result<u64, String> {
        let address = read_win64_import_argument(unicorn, 0)?;
        if address == 0 {
            return Err("Windows critical-section pointer is null".into());
        }
        match operation {
            LegacyWin64Import::InitializeCriticalSection
            | LegacyWin64Import::InitializeCriticalSectionAndSpinCount => {
                if unicorn
                    .get_data()
                    .windows_critical_sections
                    .contains_key(&address)
                {
                    return Err(format!(
                        "Windows critical section {address:#x} is already initialized"
                    ));
                }
                if unicorn.get_data().windows_critical_sections.len()
                    >= MAX_WINDOWS_CRITICAL_SECTIONS
                {
                    return Err(format!(
                        "Windows critical-section count exceeds {MAX_WINDOWS_CRITICAL_SECTIONS}"
                    ));
                }
                unicorn
                    .mem_write(address, &[0; WINDOWS_CRITICAL_SECTION_BYTES])
                    .map_err(|error| {
                        format!("Windows critical-section object {address:#x} is not writable: {error}")
                    })?;
                unicorn
                    .get_data_mut()
                    .windows_critical_sections
                    .insert(address, 0);
                Ok(u64::from(matches!(
                    operation,
                    LegacyWin64Import::InitializeCriticalSectionAndSpinCount
                )))
            }
            LegacyWin64Import::EnterCriticalSection => {
                let lock_count = unicorn
                    .get_data_mut()
                    .windows_critical_sections
                    .get_mut(&address)
                    .ok_or_else(|| {
                        format!("Windows critical section {address:#x} is not initialized")
                    })?;
                *lock_count = lock_count.checked_add(1).ok_or_else(|| {
                    "Windows critical-section recursion overflow".to_string()
                })?;
                if *lock_count > MAX_WINDOWS_CRITICAL_SECTION_RECURSION {
                    *lock_count -= 1;
                    return Err(format!(
                        "Windows critical-section recursion exceeds {MAX_WINDOWS_CRITICAL_SECTION_RECURSION}"
                    ));
                }
                Ok(0)
            }
            LegacyWin64Import::LeaveCriticalSection => {
                let lock_count = unicorn
                    .get_data_mut()
                    .windows_critical_sections
                    .get_mut(&address)
                    .ok_or_else(|| {
                        format!("Windows critical section {address:#x} is not initialized")
                    })?;
                if *lock_count == 0 {
                    return Err(format!(
                        "Windows critical section {address:#x} has an unbalanced leave"
                    ));
                }
                *lock_count -= 1;
                Ok(0)
            }
            LegacyWin64Import::DeleteCriticalSection => {
                let lock_count = unicorn
                    .get_data()
                    .windows_critical_sections
                    .get(&address)
                    .copied()
                    .ok_or_else(|| {
                        format!("Windows critical section {address:#x} is not initialized")
                    })?;
                if lock_count != 0 {
                    return Err(format!(
                        "Windows critical section {address:#x} is still locked {lock_count} times"
                    ));
                }
                unicorn
                    .get_data_mut()
                    .windows_critical_sections
                    .remove(&address);
                Ok(0)
            }
            _ => Err("invalid Windows critical-section operation".into()),
        }
    })();
    match result {
        Ok(returned) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_get_module_handle_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        if pointer == 0 {
            return Err("GetModuleHandleW(NULL) host-executable lookup is unsupported".into());
        }
        let mut units = Vec::new();
        let mut terminated = false;
        for index in 0..128u64 {
            let address = pointer
                .checked_add(index * 2)
                .ok_or_else(|| "GetModuleHandleW name address overflow".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(address, 2)
                .map_err(|error| format!("GetModuleHandleW name read failed: {error}"))?;
            let unit = u16::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| "GetModuleHandleW name unit has wrong size".to_string())?,
            );
            if unit == 0 {
                terminated = true;
                break;
            }
            units.push(unit);
        }
        if !terminated {
            return Err("GetModuleHandleW name exceeds 127 UTF-16 code units".into());
        }
        let name = String::from_utf16(&units)
            .map_err(|_| "GetModuleHandleW name is not valid UTF-16".to_string())?;
        let returned = if name.eq_ignore_ascii_case("api-ms-win-core-synch-l1-2-0.dll") {
            // This API-set lookup is an optional capability probe. Expose it as
            // unavailable so the guest takes its critical-section fallback.
            0
        } else if name.eq_ignore_ascii_case("kernel32.dll") {
            WINDOWS_KERNEL32_MODULE_TOKEN
        } else {
            return Err(format!("GetModuleHandleW module {name:?} is unsupported"));
        };
        unicorn
            .reg_write(RegisterX86::RAX, returned)
            .map_err(|error| format!("GetModuleHandleW return write failed: {error}"))
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn install_windows_condition_variable_callbacks(
    unicorn: &mut Unicorn<'static, GuestState>,
) -> Result<(), GuestError> {
    for address in [
        HOST_INITIALIZE_CONDITION_VARIABLE,
        HOST_SLEEP_CONDITION_VARIABLE_CS,
        HOST_WAKE_CONDITION_VARIABLE,
        HOST_WAKE_ALL_CONDITION_VARIABLE,
    ] {
        uc(
            "write condition-variable callback return",
            unicorn.mem_write(address, &[0xc3]),
        )?;
    }
    uc(
        "install InitializeConditionVariable callback",
        unicorn.add_code_hook(
            HOST_INITIALIZE_CONDITION_VARIABLE,
            HOST_INITIALIZE_CONDITION_VARIABLE,
            |unicorn, _, _| emulate_windows_condition_variable(unicorn, 0),
        ),
    )?;
    uc(
        "install SleepConditionVariableCS callback",
        unicorn.add_code_hook(
            HOST_SLEEP_CONDITION_VARIABLE_CS,
            HOST_SLEEP_CONDITION_VARIABLE_CS,
            |unicorn, _, _| emulate_windows_condition_variable(unicorn, 1),
        ),
    )?;
    uc(
        "install WakeConditionVariable callback",
        unicorn.add_code_hook(
            HOST_WAKE_CONDITION_VARIABLE,
            HOST_WAKE_CONDITION_VARIABLE,
            |unicorn, _, _| emulate_windows_condition_variable(unicorn, 2),
        ),
    )?;
    uc(
        "install WakeAllConditionVariable callback",
        unicorn.add_code_hook(
            HOST_WAKE_ALL_CONDITION_VARIABLE,
            HOST_WAKE_ALL_CONDITION_VARIABLE,
            |unicorn, _, _| emulate_windows_condition_variable(unicorn, 3),
        ),
    )?;
    Ok(())
}

fn emulate_windows_condition_variable(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: u8,
) {
    let result = (|| -> Result<(), String> {
        let address = read_win64_import_argument(unicorn, 0)?;
        if address == 0 {
            return Err("Windows condition-variable pointer is null".into());
        }
        match operation {
            0 => {
                if unicorn
                    .get_data()
                    .windows_condition_variables
                    .contains(&address)
                {
                    return Err(format!(
                        "Windows condition variable {address:#x} is already initialized"
                    ));
                }
                if unicorn.get_data().windows_condition_variables.len()
                    >= MAX_WINDOWS_CONDITION_VARIABLES
                {
                    return Err(format!(
                        "Windows condition-variable count exceeds {MAX_WINDOWS_CONDITION_VARIABLES}"
                    ));
                }
                unicorn.mem_write(address, &[0; 8]).map_err(|error| {
                    format!("Windows condition variable {address:#x} is not writable: {error}")
                })?;
                unicorn
                    .get_data_mut()
                    .windows_condition_variables
                    .insert(address);
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                Ok(())
            }
            1 => {
                if !unicorn
                    .get_data()
                    .windows_condition_variables
                    .contains(&address)
                {
                    return Err(format!(
                        "Windows condition variable {address:#x} is not initialized"
                    ));
                }
                Err("blocking SleepConditionVariableCS is unsupported in the serial backend".into())
            }
            2 | 3 => {
                if !unicorn
                    .get_data()
                    .windows_condition_variables
                    .contains(&address)
                {
                    return Err(format!(
                        "Windows condition variable {address:#x} is not initialized"
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                Ok(())
            }
            _ => Err("invalid Windows condition-variable operation".into()),
        }
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_get_system_time_as_file_time(unicorn: &mut Unicorn<'_, GuestState>) {
    // A fixed, nonzero Windows FILETIME keeps compatibility deterministic and
    // avoids exposing host wall-clock state to the emulated guest.
    const FIXED_FILETIME: u64 = 132_223_104_000_000_000;
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("GetSystemTimeAsFileTime output pointer is null".to_string());
        }
        unicorn
            .mem_write(output, &FIXED_FILETIME.to_le_bytes())
            .map_err(|error| {
                format!("GetSystemTimeAsFileTime output {output:#x} is not writable: {error}")
            })
    });
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
    }
}

fn emulate_query_performance_counter(unicorn: &mut Unicorn<'_, GuestState>) {
    const FIXED_COUNTER: u64 = 1;
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("QueryPerformanceCounter output pointer is null".to_string());
        }
        unicorn
            .mem_write(output, &FIXED_COUNTER.to_le_bytes())
            .map_err(|error| {
                format!("QueryPerformanceCounter output {output:#x} is not writable: {error}")
            })
    });
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    }
}

fn emulate_initialize_slist_head(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("InitializeSListHead output pointer is null".to_string());
        }
        unicorn.mem_write(output, &[0; 16]).map_err(|error| {
            format!("InitializeSListHead output {output:#x} is not writable: {error}")
        })
    });
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
    }
}

fn read_win64_import_argument(
    unicorn: &Unicorn<'_, GuestState>,
    index: usize,
) -> Result<u64, String> {
    if index >= MAX_WIN64_IMPORT_ARGUMENTS {
        return Err(format!(
            "Win64 import argument {} exceeds the supported 1..={MAX_WIN64_IMPORT_ARGUMENTS} range",
            index + 1
        ));
    }
    let register = match index {
        0 => Some(RegisterX86::RCX),
        1 => Some(RegisterX86::RDX),
        2 => Some(RegisterX86::R8),
        3 => Some(RegisterX86::R9),
        _ => None,
    };
    if let Some(register) = register {
        return unicorn
            .reg_read(register)
            .map_err(|error| format!("read Win64 import argument {}: {error}", index + 1));
    }
    let rsp = unicorn
        .reg_read(RegisterX86::RSP)
        .map_err(|error| format!("read Win64 import stack pointer: {error}"))?;
    let offset = 0x28 + ((index - 4) as u64 * 8);
    let address = rsp
        .checked_add(offset)
        .ok_or_else(|| "Win64 import stack argument address overflow".to_string())?;
    let bytes = unicorn
        .mem_read_as_vec(address, 8)
        .map_err(|error| format!("read Win64 import argument {}: {error}", index + 1))?;
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| {
        "Win64 import stack argument returned the wrong size".to_string()
    })?))
}
