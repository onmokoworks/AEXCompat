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
    CrtStrdup,
    CrtToLower,
    CrtToUpper,
    AlignedMalloc,
    AlignedFree,
    CallNewHandler,
    Strncpy,
    Memset,
    MemoryCopy,
    MemChr,
    MemCmp,
    StdioVsnprintfS,
    StdioVsprintf,
    MsvcpMutexInit,
    MsvcpMutexLock,
    MsvcpMutexUnlock,
    MsvcpMutexDestroy,
    MsvcpHardwareConcurrency,
    MsvcpExceptionPtrCreate,
    MsvcpExceptionPtrCopy,
    MsvcpExceptionPtrAssign,
    MsvcpExceptionPtrDestroy,
    MsvcpExceptionPtrCurrentException,
    MsvcpExceptionPtrRethrow,
    VcruntimeExceptionCopy,
    VcruntimeExceptionDestroy,
    CxxThrowException,
    Cos,
    CosF,
    Ceil,
    CeilF,
    ExpF,
    Floor,
    FloorF,
    LRound,
    Round,
    RoundF,
    PowF,
    Pow,
    Sin,
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
    GetSystemInfo,
    GetStartupInfoW,
    RtlCaptureContext,
    GetStdHandle,
    GetConsoleMode,
    GetFileType,
    GetCommandLineA,
    GetCommandLineW,
    GetACP,
    GetCPInfo,
    IsDebuggerPresent,
    GetCurrentThreadId,
    GetCurrentProcessId,
    QueryPerformanceCounter,
    QueryPerformanceFrequency,
    GetEnvironmentVariableA,
    GetEnvironmentVariableW,
    WideCharToMultiByte,
    MultiByteToWideChar,
    GetStringTypeW,
    LCMapStringW,
    GetLastError,
    SetLastError,
    SetThreadErrorMode,
    LoadLibraryA,
    LoadLibraryExW,
    FlsAlloc,
    FlsGetValue,
    FlsSetValue,
    FlsFree,
    TlsAlloc,
    TlsGetValue,
    TlsSetValue,
    TlsFree,
    ExplicitMsvcRuntimeZero,
    CrtInitterm,
    CrtInittermE,
    CrtInitializeOnexitTable,
    CrtRegisterOnexitFunction,
    CrtExecuteOnexitTable,
    CrtSetTerminate,
    CrtGetenv,
    InitializeCriticalSection,
    InitializeCriticalSectionAndSpinCount,
    EnterCriticalSection,
    LeaveCriticalSection,
    DeleteCriticalSection,
    GetModuleHandleW,
    GetModuleHandleExA,
    GetModuleFileNameW,
    GetProcAddress,
    InitializeSListHead,
    DisableThreadLibraryCalls,
    ProcessPrng,
    GetProcessHeap,
    HeapAlloc,
    HeapFree,
    HeapReAlloc,
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
        ("kernel32.dll", "GetSystemTimeAsFileTime") => LegacyWin64Import::GetSystemTimeAsFileTime,
        ("kernel32.dll", "GetSystemInfo") => LegacyWin64Import::GetSystemInfo,
        (_, "GetSystemInfo") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetStartupInfoW") => LegacyWin64Import::GetStartupInfoW,
        (_, "GetStartupInfoW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "RtlCaptureContext") => LegacyWin64Import::RtlCaptureContext,
        (_, "RtlCaptureContext") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetStdHandle") => LegacyWin64Import::GetStdHandle,
        (_, "GetStdHandle") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetConsoleMode") => LegacyWin64Import::GetConsoleMode,
        (_, "GetConsoleMode") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetFileType") => LegacyWin64Import::GetFileType,
        (_, "GetFileType") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetCommandLineA") => LegacyWin64Import::GetCommandLineA,
        (_, "GetCommandLineA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetCommandLineW") => LegacyWin64Import::GetCommandLineW,
        (_, "GetCommandLineW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetACP") => LegacyWin64Import::GetACP,
        (_, "GetACP") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetCPInfo") => LegacyWin64Import::GetCPInfo,
        (_, "GetCPInfo") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "IsDebuggerPresent") => LegacyWin64Import::IsDebuggerPresent,
        (_, "IsDebuggerPresent") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetCurrentThreadId") => LegacyWin64Import::GetCurrentThreadId,
        ("kernel32.dll", "GetCurrentProcessId") => LegacyWin64Import::GetCurrentProcessId,
        ("kernel32.dll", "QueryPerformanceCounter") => LegacyWin64Import::QueryPerformanceCounter,
        ("kernel32.dll", "QueryPerformanceFrequency") => {
            LegacyWin64Import::QueryPerformanceFrequency
        }
        ("kernel32.dll", "GetEnvironmentVariableA") => LegacyWin64Import::GetEnvironmentVariableA,
        ("kernel32.dll", "GetEnvironmentVariableW") => LegacyWin64Import::GetEnvironmentVariableW,
        (_, "GetEnvironmentVariableW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "WideCharToMultiByte") => LegacyWin64Import::WideCharToMultiByte,
        (_, "WideCharToMultiByte") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "MultiByteToWideChar") => LegacyWin64Import::MultiByteToWideChar,
        (_, "MultiByteToWideChar") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetStringTypeW") => LegacyWin64Import::GetStringTypeW,
        (_, "GetStringTypeW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "LCMapStringW") => LegacyWin64Import::LCMapStringW,
        (_, "LCMapStringW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetLastError") => LegacyWin64Import::GetLastError,
        ("kernel32.dll", "SetLastError") => LegacyWin64Import::SetLastError,
        ("kernel32.dll", "SetThreadErrorMode") => LegacyWin64Import::SetThreadErrorMode,
        (_, "SetThreadErrorMode") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "LoadLibraryA") => LegacyWin64Import::LoadLibraryA,
        (_, "LoadLibraryA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "LoadLibraryExW") => LegacyWin64Import::LoadLibraryExW,
        (_, "LoadLibraryExW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "FlsAlloc") => LegacyWin64Import::FlsAlloc,
        ("kernel32.dll", "FlsGetValue") => LegacyWin64Import::FlsGetValue,
        ("kernel32.dll", "FlsSetValue") => LegacyWin64Import::FlsSetValue,
        ("kernel32.dll", "FlsFree") => LegacyWin64Import::FlsFree,
        ("kernel32.dll", "TlsAlloc") => LegacyWin64Import::TlsAlloc,
        ("kernel32.dll", "TlsGetValue") => LegacyWin64Import::TlsGetValue,
        ("kernel32.dll", "TlsSetValue") => LegacyWin64Import::TlsSetValue,
        ("kernel32.dll", "TlsFree") => LegacyWin64Import::TlsFree,
        (_, "TlsAlloc" | "TlsGetValue" | "TlsSetValue" | "TlsFree") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("kernel32.dll", "InitializeSListHead") => LegacyWin64Import::InitializeSListHead,
        ("kernel32.dll", "DisableThreadLibraryCalls") => {
            LegacyWin64Import::DisableThreadLibraryCalls
        }
        ("bcryptprimitives.dll", "ProcessPrng") => LegacyWin64Import::ProcessPrng,
        (_, "ProcessPrng") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "GetProcessHeap") => LegacyWin64Import::GetProcessHeap,
        ("kernel32.dll", "HeapAlloc") => LegacyWin64Import::HeapAlloc,
        ("kernel32.dll", "HeapFree") => LegacyWin64Import::HeapFree,
        ("kernel32.dll", "HeapReAlloc") => LegacyWin64Import::HeapReAlloc,
        (_, "GetProcessHeap" | "HeapAlloc" | "HeapFree" | "HeapReAlloc") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
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
        ("kernel32.dll", "GetModuleHandleExA") => LegacyWin64Import::GetModuleHandleExA,
        ("kernel32.dll", "GetModuleFileNameW") => LegacyWin64Import::GetModuleFileNameW,
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
                "_initialize_narrow_environment" | "_configure_narrow_argv" | "_cexit"
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
        ("api-ms-win-crt-runtime-l1-1-0.dll", "set_terminate") => {
            LegacyWin64Import::CrtSetTerminate
        }
        (_, "set_terminate") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-environment-l1-1-0.dll", "getenv") => LegacyWin64Import::CrtGetenv,
        (_, "getenv") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll", "cos") => LegacyWin64Import::Cos,
        (_, "cos") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "ceil") => LegacyWin64Import::Ceil,
        (_, "ceil") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "ceilf") => LegacyWin64Import::CeilF,
        (_, "ceilf") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "floor") => LegacyWin64Import::Floor,
        (_, "floor") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "lround") => LegacyWin64Import::LRound,
        (_, "lround") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "round") => LegacyWin64Import::Round,
        (_, "round") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "roundf") => LegacyWin64Import::RoundF,
        (_, "roundf") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll", "sin") => LegacyWin64Import::Sin,
        (_, "sin") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_initterm") => LegacyWin64Import::CrtInitterm,
        ("api-ms-win-crt-runtime-l1-1-0.dll", "_initterm_e") => LegacyWin64Import::CrtInittermE,
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
        ("vcruntime140.dll", "memcmp") => LegacyWin64Import::MemCmp,
        (_, "memcmp") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll", "__stdio_common_vsnprintf_s") => {
            LegacyWin64Import::StdioVsnprintfS
        }
        (_, "__stdio_common_vsnprintf_s") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "__stdio_common_vsprintf") => {
            LegacyWin64Import::StdioVsprintf
        }
        (_, "__stdio_common_vsprintf") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "_strdup") => {
            LegacyWin64Import::CrtStrdup
        }
        (_, "_strdup") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "tolower") => {
            LegacyWin64Import::CrtToLower
        }
        (_, "tolower") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "toupper") => {
            LegacyWin64Import::CrtToUpper
        }
        (_, "toupper") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-heap-l1-1-0.dll" | "ucrtbase.dll", "_aligned_malloc") => {
            LegacyWin64Import::AlignedMalloc
        }
        ("api-ms-win-crt-heap-l1-1-0.dll" | "ucrtbase.dll", "_aligned_free") => {
            LegacyWin64Import::AlignedFree
        }
        (_, "_aligned_malloc" | "_aligned_free") => {
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
        ("msvcp140.dll", "?__ExceptionPtrCreate@@YAXPEAX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrCreate
        }
        ("msvcp140.dll", "?__ExceptionPtrCopy@@YAXPEAXPEBX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrCopy
        }
        ("msvcp140.dll", "?__ExceptionPtrAssign@@YAXPEAXPEBX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrAssign
        }
        ("msvcp140.dll", "?__ExceptionPtrDestroy@@YAXPEAX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrDestroy
        }
        ("msvcp140.dll", "?__ExceptionPtrCurrentException@@YAXPEAX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrCurrentException
        }
        ("msvcp140.dll", "?__ExceptionPtrRethrow@@YAXPEBX@Z") => {
            LegacyWin64Import::MsvcpExceptionPtrRethrow
        }
        (_, symbol) if symbol.contains("__ExceptionPtr") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcruntime140.dll", "__std_exception_copy") => LegacyWin64Import::VcruntimeExceptionCopy,
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
            LegacyWin64Import::CrtStrdup => {
                uc("write _strdup return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install _strdup import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_strdup(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::CrtToLower => {
                uc("write tolower return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install tolower import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        let input = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX) as u32;
                        let result = if (u32::from(b'A')..=u32::from(b'Z')).contains(&input) {
                            input + u32::from(b'a' - b'A')
                        } else {
                            input
                        };
                        let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(result));
                    }),
                )?;
            }
            LegacyWin64Import::CrtToUpper => {
                uc("write toupper return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install toupper import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        let input = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX) as u32;
                        let result = if (u32::from(b'a')..=u32::from(b'z')).contains(&input) {
                            input - u32::from(b'a' - b'A')
                        } else {
                            input
                        };
                        let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(result));
                    }),
                )?;
            }
            LegacyWin64Import::AlignedMalloc => {
                uc(
                    "write _aligned_malloc return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _aligned_malloc import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_aligned_malloc(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::AlignedFree => {
                uc(
                    "write _aligned_free return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install _aligned_free import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_aligned_free(unicorn);
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
            LegacyWin64Import::MemCmp => {
                uc("write memcmp return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install memcmp import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_memcmp(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::StdioVsnprintfS => {
                uc(
                    "write stdio formatter return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install stdio formatter import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_stdio_common_vsnprintf_s(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::StdioVsprintf => {
                uc("write vsprintf return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install vsprintf import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_stdio_common_vsprintf(unicorn);
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
                uc(
                    "write mutex unlock return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install mutex unlock import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_msvcp_mutex_unlock(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MsvcpMutexDestroy => {
                uc(
                    "write mutex destroy return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
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
            LegacyWin64Import::MsvcpExceptionPtrCreate
            | LegacyWin64Import::MsvcpExceptionPtrCopy
            | LegacyWin64Import::MsvcpExceptionPtrAssign
            | LegacyWin64Import::MsvcpExceptionPtrDestroy
            | LegacyWin64Import::MsvcpExceptionPtrCurrentException
            | LegacyWin64Import::MsvcpExceptionPtrRethrow => {
                uc(
                    "write MSVC exception_ptr return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install MSVC exception_ptr import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_msvcp_exception_ptr(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::VcruntimeExceptionCopy => {
                uc(
                    "write exception copy return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install exception copy import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_vcruntime_exception_copy(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::VcruntimeExceptionDestroy => {
                uc(
                    "write exception destroy return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
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
            LegacyWin64Import::Cos => {
                install_double_import(unicorn, stub, "cos", f64::cos)?;
            }
            LegacyWin64Import::CosF => {
                install_float_import(unicorn, stub, "cosf", f32::cos)?;
            }
            LegacyWin64Import::Ceil => {
                install_double_import(unicorn, stub, "ceil", f64::ceil)?;
            }
            LegacyWin64Import::CeilF => {
                install_float_import(unicorn, stub, "ceilf", f32::ceil)?;
            }
            LegacyWin64Import::ExpF => {
                install_float_import(unicorn, stub, "expf", f32::exp)?;
            }
            LegacyWin64Import::Floor => {
                install_double_import(unicorn, stub, "floor", f64::floor)?;
            }
            LegacyWin64Import::FloorF => {
                install_float_import(unicorn, stub, "floorf", f32::floor)?;
            }
            LegacyWin64Import::LRound => {
                install_lround_import(unicorn, stub)?;
            }
            LegacyWin64Import::Round => {
                install_double_import(unicorn, stub, "round", f64::round)?;
            }
            LegacyWin64Import::RoundF => {
                install_float_import(unicorn, stub, "roundf", f32::round)?;
            }
            LegacyWin64Import::PowF => {
                install_float_binary_import(unicorn, stub, "powf", f32::powf)?;
            }
            LegacyWin64Import::Pow => {
                install_double_binary_import(unicorn, stub, "pow", f64::powf)?;
            }
            LegacyWin64Import::Sin => {
                install_double_import(unicorn, stub, "sin", f64::sin)?;
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
                uc(
                    "write omp_set_dynamic return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
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
            LegacyWin64Import::GetSystemInfo => {
                uc(
                    "write GetSystemInfo return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetSystemInfo import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_system_info(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetStartupInfoW => {
                uc(
                    "write GetStartupInfoW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetStartupInfoW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_startup_info_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::RtlCaptureContext => {
                uc(
                    "write RtlCaptureContext return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install RtlCaptureContext import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_rtl_capture_context(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetStdHandle => {
                uc(
                    "write GetStdHandle return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetStdHandle import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_std_handle(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetConsoleMode => {
                uc(
                    "write GetConsoleMode return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetConsoleMode import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_console_mode(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetFileType => {
                uc("write GetFileType return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install GetFileType import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_file_type(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetCommandLineA => {
                uc(
                    "write GetCommandLineA return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetCommandLineA import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_command_line_a(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetCommandLineW => {
                uc(
                    "write GetCommandLineW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetCommandLineW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_command_line_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetACP => {
                uc(
                    "install deterministic GetACP import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(932)),
                )?;
            }
            LegacyWin64Import::GetCPInfo => {
                uc("write GetCPInfo return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install GetCPInfo import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_cp_info(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::IsDebuggerPresent => {
                uc(
                    "install deterministic IsDebuggerPresent import",
                    unicorn.mem_write(stub, &deterministic_i32_stub(0)),
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
            LegacyWin64Import::QueryPerformanceFrequency => {
                uc(
                    "write QueryPerformanceFrequency return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install QueryPerformanceFrequency import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_query_performance_frequency(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetEnvironmentVariableA => {
                uc(
                    "write deterministic guest environment value",
                    unicorn.mem_write(HOST_ENVIRONMENT_VALUE, b"1\0"),
                )?;
                uc(
                    "write GetEnvironmentVariableA return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetEnvironmentVariableA import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_environment_variable_a(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetEnvironmentVariableW => {
                uc(
                    "write GetEnvironmentVariableW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetEnvironmentVariableW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_environment_variable_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::WideCharToMultiByte => {
                uc(
                    "write WideCharToMultiByte return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install WideCharToMultiByte import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_wide_char_to_multi_byte(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::MultiByteToWideChar => {
                uc(
                    "write MultiByteToWideChar return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install MultiByteToWideChar import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_multi_byte_to_wide_char(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetStringTypeW => {
                uc(
                    "write GetStringTypeW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetStringTypeW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_string_type_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::LCMapStringW => {
                uc(
                    "write LCMapStringW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install LCMapStringW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_lc_map_string_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetLastError | LegacyWin64Import::SetLastError => {
                uc(
                    "write last-error import return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install last-error import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_windows_last_error(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::SetThreadErrorMode => {
                uc(
                    "write SetThreadErrorMode return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install SetThreadErrorMode import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_set_thread_error_mode(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::LoadLibraryExW => {
                uc(
                    "write LoadLibraryExW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install LoadLibraryExW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_load_library_ex_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::LoadLibraryA => {
                uc(
                    "write LoadLibraryA return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install LoadLibraryA import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_load_library_a(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::FlsAlloc
            | LegacyWin64Import::FlsGetValue
            | LegacyWin64Import::FlsSetValue => {
                uc("write FLS import return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install FLS import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_fls(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::FlsFree => {
                uc(
                    "write FlsFree callback tail jump",
                    unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                )?;
                uc(
                    "install FlsFree import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_fls_free(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::TlsAlloc
            | LegacyWin64Import::TlsGetValue
            | LegacyWin64Import::TlsSetValue
            | LegacyWin64Import::TlsFree => {
                uc("write TLS import return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install TLS import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_tls(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::ExplicitMsvcRuntimeZero => {
                // This finite allowlist mirrors the prior deterministic-zero
                // behavior for CRT startup/teardown only. Every other unknown
                // import remains a typed trap.
            }
            LegacyWin64Import::CrtInitterm | LegacyWin64Import::CrtInittermE => {
                uc(
                    "write CRT initializer tail jump",
                    unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                )?;
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
                uc(
                    "write CRT onexit tail jump",
                    unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                )?;
                uc(
                    "install CRT onexit execution import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_crt_onexit(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::CrtGetenv => {
                uc(
                    "write deterministic guest environment value",
                    unicorn.mem_write(HOST_ENVIRONMENT_VALUE, b"1\0"),
                )?;
                uc(
                    "write deterministic getenv return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install deterministic getenv import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_getenv(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::CrtSetTerminate => {
                uc(
                    "write CRT set_terminate return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install CRT set_terminate import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_crt_set_terminate(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::InitializeCriticalSection
            | LegacyWin64Import::InitializeCriticalSectionAndSpinCount
            | LegacyWin64Import::EnterCriticalSection
            | LegacyWin64Import::LeaveCriticalSection
            | LegacyWin64Import::DeleteCriticalSection => {
                uc(
                    "write critical-section return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install critical-section import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_windows_critical_section(unicorn, implementation);
                    }),
                )?;
            }
            LegacyWin64Import::GetModuleHandleW => {
                uc(
                    "write GetModuleHandleW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetModuleHandleW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_module_handle_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetModuleHandleExA => {
                uc(
                    "write GetModuleHandleExA return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetModuleHandleExA import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_module_handle_ex_a(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetModuleFileNameW => {
                uc(
                    "write GetModuleFileNameW return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install GetModuleFileNameW import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_get_module_file_name_w(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetProcAddress => {
                uc(
                    "write GetProcAddress return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
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
            LegacyWin64Import::ProcessPrng => {
                uc("write ProcessPrng return", unicorn.mem_write(stub, &[0xc3]))?;
                uc(
                    "install ProcessPrng import",
                    unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                        emulate_process_prng(unicorn);
                    }),
                )?;
            }
            LegacyWin64Import::GetProcessHeap => {
                uc(
                    "install deterministic GetProcessHeap import",
                    unicorn.mem_write(stub, &deterministic_u64_stub(PROCESS_HEAP_HANDLE)),
                )?;
            }
            LegacyWin64Import::HeapAlloc
            | LegacyWin64Import::HeapFree
            | LegacyWin64Import::HeapReAlloc => {
                uc(
                    "write process heap return",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install process heap import",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        emulate_process_heap(unicorn, implementation);
                    }),
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
                    return Err(format!(
                        "CRT onexit table {table:#x} is already initialized"
                    ));
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
        let name = read_windows_environment_name(unicorn, pointer, "CRT getenv")?;
        let returned = if let Some(value) = deterministic_guest_environment_value(&name) {
            let mut terminated = Vec::with_capacity(value.len() + 1);
            terminated.extend_from_slice(value);
            terminated.push(0);
            unicorn
                .mem_write(HOST_ENVIRONMENT_VALUE, &terminated)
                .map_err(|error| format!("CRT getenv value write failed: {error}"))?;
            HOST_ENVIRONMENT_VALUE
        } else {
            0
        };
        unicorn
            .reg_write(RegisterX86::RAX, returned)
            .map_err(|error| format!("CRT getenv return write failed: {error}"))
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn read_windows_environment_name(
    unicorn: &mut Unicorn<'_, GuestState>,
    pointer: u64,
    function: &str,
) -> Result<Vec<u8>, String> {
    if pointer == 0 {
        return Err(format!("{function} name pointer is null"));
    }
    let mut name = Vec::new();
    for index in 0..=MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
        let address = pointer
            .checked_add(index as u64)
            .ok_or_else(|| format!("{function} name address overflow"))?;
        let byte = unicorn
            .mem_read_as_vec(address, 1)
            .map_err(|error| format!("{function} name read failed: {error}"))?[0];
        if byte == 0 {
            return Ok(name);
        }
        if index == MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
            break;
        }
        name.push(byte);
    }
    Err(format!(
        "{function} name exceeds {MAX_WINDOWS_ENVIRONMENT_NAME_BYTES} bytes"
    ))
}

fn deterministic_guest_environment_value(name: &[u8]) -> Option<&'static [u8]> {
    if name.eq_ignore_ascii_case(b"OPENCV_FOR_THREADS_NUM") {
        Some(b"1")
    } else {
        None
    }
}

fn emulate_wide_char_to_multi_byte(unicorn: &mut Unicorn<'_, GuestState>) {
    const CP_ACP: u32 = 0;
    const CP_OEMCP: u32 = 1;
    const CP_UTF8: u32 = 65_001;
    const WC_ERR_INVALID_CHARS: u32 = 0x80;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const ERROR_NO_UNICODE_TRANSLATION: u32 = 1113;

    let result = (|| -> Result<(u64, Option<u32>), String> {
        let code_page = read_win64_import_argument(unicorn, 0)? as u32;
        let flags = read_win64_import_argument(unicorn, 1)? as u32;
        let source = read_win64_import_argument(unicorn, 2)?;
        let source_length = read_win64_import_argument(unicorn, 3)? as u32 as i32;
        let destination = read_win64_import_argument(unicorn, 4)?;
        let destination_length = read_win64_import_argument(unicorn, 5)? as u32 as i32;
        let default_character = read_win64_import_argument(unicorn, 6)?;
        let used_default_character = read_win64_import_argument(unicorn, 7)?;

        if !matches!(code_page, CP_ACP | CP_OEMCP | CP_UTF8)
            || (code_page == CP_UTF8 && flags & !WC_ERR_INVALID_CHARS != 0)
            || (code_page != CP_UTF8 && flags != 0)
            || (code_page == CP_UTF8 && (default_character != 0 || used_default_character != 0))
            || source == 0
            || source_length == 0
            || source_length < -1
            || destination_length < 0
        {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }

        let include_terminator = source_length == -1;
        let mut units = Vec::new();
        if include_terminator {
            for index in 0..=(MAX_CRT_STRING_BYTES / 2) {
                let address = source
                    .checked_add(index * 2)
                    .ok_or_else(|| "WideCharToMultiByte source address overflow".to_string())?;
                let bytes = unicorn
                    .mem_read_as_vec(address, 2)
                    .map_err(|error| format!("WideCharToMultiByte source read failed: {error}"))?;
                let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
                if unit == 0 {
                    break;
                }
                if index == MAX_CRT_STRING_BYTES / 2 {
                    return Err("WideCharToMultiByte source is unterminated".into());
                }
                units.push(unit);
            }
        } else if source_length > 0 {
            let byte_length = u64::try_from(source_length)
                .ok()
                .and_then(|length| length.checked_mul(2))
                .filter(|length| *length <= MAX_CRT_STRING_BYTES)
                .ok_or_else(|| "WideCharToMultiByte source is too large".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(source, byte_length as usize)
                .map_err(|error| format!("WideCharToMultiByte source read failed: {error}"))?;
            units.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
            );
        } else {
            return Err("WideCharToMultiByte received invalid source length".into());
        }

        let mut unicode = String::new();
        for character in char::decode_utf16(units) {
            match character {
                Ok(character) => unicode.push(character),
                Err(_) if flags & WC_ERR_INVALID_CHARS == 0 => {
                    unicode.push(char::REPLACEMENT_CHARACTER)
                }
                Err(_) => return Ok((0, Some(ERROR_NO_UNICODE_TRANSLATION))),
            }
        }
        let (mut bytes, used_default) = if code_page == CP_UTF8 {
            (unicode.into_bytes(), false)
        } else {
            let default = if default_character == 0 {
                b'?'
            } else {
                let byte = unicorn
                    .mem_read_as_vec(default_character, 1)
                    .map_err(|error| {
                        format!("WideCharToMultiByte default character read failed: {error}")
                    })?[0];
                if byte >= 0x80 {
                    return Err(
                        "WideCharToMultiByte only supports a single-byte default character".into(),
                    );
                }
                byte
            };
            encode_shift_jis_with_default(&unicode, default)
        };
        if include_terminator {
            bytes.push(0);
        }
        let required = u32::try_from(bytes.len())
            .map_err(|_| "WideCharToMultiByte output length exceeds DWORD".to_string())?;
        if used_default_character != 0 {
            unicorn
                .mem_write(used_default_character, &[u8::from(used_default)])
                .map_err(|error| {
                    format!(
                        "WideCharToMultiByte used-default output {used_default_character:#x} is not writable: {error}"
                    )
                })?;
        }
        if destination_length == 0 {
            return Ok((u64::from(required), None));
        }
        if destination == 0 || destination_length < required as i32 {
            return Ok((0, Some(ERROR_INSUFFICIENT_BUFFER)));
        }
        unicorn.mem_write(destination, &bytes).map_err(|error| {
            format!("WideCharToMultiByte output {destination:#x} is not writable: {error}")
        })?;
        Ok((u64::from(required), None))
    })();

    match result {
        Ok((returned, error)) => {
            if let Some(error) = error {
                unicorn.get_data_mut().windows_last_error = error;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_multi_byte_to_wide_char(unicorn: &mut Unicorn<'_, GuestState>) {
    use unicode_normalization::UnicodeNormalization;
    const CP_ACP: u32 = 0;
    const CP_SHIFT_JIS: u32 = 932;
    const CP_UTF8: u32 = 65_001;
    const MB_PRECOMPOSED: u32 = 0x01;
    const MB_COMPOSITE: u32 = 0x02;
    const MB_USEGLYPHCHARS: u32 = 0x04;
    const MB_ERR_INVALID_CHARS: u32 = 0x08;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const ERROR_NO_UNICODE_TRANSLATION: u32 = 1113;

    let result = (|| -> Result<(u64, Option<u32>), String> {
        let code_page = read_win64_import_argument(unicorn, 0)? as u32;
        let flags = read_win64_import_argument(unicorn, 1)? as u32;
        let source = read_win64_import_argument(unicorn, 2)?;
        let source_length = read_win64_import_argument(unicorn, 3)? as u32 as i32;
        let destination = read_win64_import_argument(unicorn, 4)?;
        let destination_length = read_win64_import_argument(unicorn, 5)? as u32 as i32;

        let valid_flags = if code_page == CP_UTF8 {
            flags == 0 || flags == MB_ERR_INVALID_CHARS
        } else {
            flags & !(MB_PRECOMPOSED | MB_COMPOSITE | MB_USEGLYPHCHARS | MB_ERR_INVALID_CHARS) == 0
                && flags & (MB_PRECOMPOSED | MB_COMPOSITE) != (MB_PRECOMPOSED | MB_COMPOSITE)
        };
        if !matches!(code_page, CP_ACP | CP_SHIFT_JIS | CP_UTF8)
            || !valid_flags
            || source == 0
            || source_length == 0
            || source_length < -1
            || destination_length < 0
            || (destination_length > 0 && destination == 0)
            || (destination_length > 0 && destination == source)
        {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }

        let include_terminator = source_length == -1;
        let bytes = if include_terminator {
            let mut bytes = Vec::new();
            for index in 0..=MAX_CRT_STRING_BYTES {
                let address = source
                    .checked_add(index)
                    .ok_or_else(|| "MultiByteToWideChar source address overflow".to_string())?;
                let byte = unicorn
                    .mem_read_as_vec(address, 1)
                    .map_err(|error| format!("MultiByteToWideChar source read failed: {error}"))?
                    [0];
                if byte == 0 {
                    break;
                }
                if index == MAX_CRT_STRING_BYTES {
                    return Err("MultiByteToWideChar source is unterminated".into());
                }
                bytes.push(byte);
            }
            bytes
        } else {
            let length = usize::try_from(source_length)
                .ok()
                .filter(|length| *length as u64 <= MAX_CRT_STRING_BYTES)
                .ok_or_else(|| "MultiByteToWideChar source is too large".to_string())?;
            unicorn
                .mem_read_as_vec(source, length)
                .map_err(|error| format!("MultiByteToWideChar source read failed: {error}"))?
        };

        let (unicode, malformed) = if code_page == CP_UTF8 {
            match std::str::from_utf8(&bytes) {
                Ok(text) => (text.to_owned(), false),
                Err(_) if flags & MB_ERR_INVALID_CHARS != 0 => {
                    return Ok((0, Some(ERROR_NO_UNICODE_TRANSLATION)));
                }
                Err(_) => (String::from_utf8_lossy(&bytes).into_owned(), true),
            }
        } else {
            let (decoded, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&bytes);
            if malformed && flags & MB_ERR_INVALID_CHARS != 0 {
                return Ok((0, Some(ERROR_NO_UNICODE_TRANSLATION)));
            }
            (decoded.into_owned(), malformed)
        };
        // With MB_ERR_INVALID_CHARS clear, Windows replaces malformed input.
        // `encoding_rs` and `from_utf8_lossy` deterministically use U+FFFD.
        let _ = malformed;
        let mut units = if flags & MB_COMPOSITE != 0 {
            unicode
                .nfd()
                .collect::<String>()
                .encode_utf16()
                .collect::<Vec<_>>()
        } else {
            unicode.encode_utf16().collect::<Vec<_>>()
        };
        if include_terminator {
            units.push(0);
        }
        let required = u32::try_from(units.len())
            .map_err(|_| "MultiByteToWideChar output length exceeds DWORD".to_string())?;
        if destination_length == 0 {
            return Ok((u64::from(required), None));
        }
        if destination_length < required as i32 {
            return Ok((0, Some(ERROR_INSUFFICIENT_BUFFER)));
        }

        let output_size = units
            .len()
            .checked_mul(2)
            .ok_or_else(|| "MultiByteToWideChar output byte length overflows".to_string())?;
        let source_size = bytes.len() + usize::from(include_terminator);
        let source_end = match source.checked_add(source_size as u64) {
            Some(end) => end,
            None => return Ok((0, Some(ERROR_INVALID_PARAMETER))),
        };
        let output_end_exclusive = match destination.checked_add(output_size as u64) {
            Some(end) => end,
            None => return Ok((0, Some(ERROR_INVALID_PARAMETER))),
        };
        if source < output_end_exclusive && destination < source_end {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }
        if output_size != 0 {
            let output_end = destination
                .checked_add(output_size as u64 - 1)
                .ok_or_else(|| "MultiByteToWideChar output range overflows".to_string())?;
            let regions = unicorn
                .mem_regions()
                .map_err(|error| format!("MultiByteToWideChar memory-map query failed: {error}"))?;
            let mut cursor = destination;
            while cursor <= output_end {
                let region = regions
                    .iter()
                    .find(|region| {
                        region.begin <= cursor
                            && cursor <= region.end
                            && region.perms & Prot::WRITE.0 as u32 != 0
                    })
                    .ok_or_else(|| {
                        format!(
                            "MultiByteToWideChar output {destination:#x}..={output_end:#x} is not fully writable"
                        )
                    })?;
                if region.end >= output_end {
                    break;
                }
                cursor = region
                    .end
                    .checked_add(1)
                    .ok_or_else(|| "MultiByteToWideChar writable region overflows".to_string())?;
            }
        }
        let output = units
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        unicorn.mem_write(destination, &output).map_err(|error| {
            format!("MultiByteToWideChar output {destination:#x} is not writable: {error}")
        })?;
        Ok((u64::from(required), None))
    })();

    match result {
        Ok((returned, error)) => {
            if let Some(error) = error {
                unicorn.get_data_mut().windows_last_error = error;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_get_string_type_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const CT_CTYPE1: u32 = 1;
    const CT_CTYPE2: u32 = 2;
    const CT_CTYPE3: u32 = 3;

    let result = (|| -> Result<(u64, Option<u32>), String> {
        let info_type = read_win64_import_argument(unicorn, 0)? as u32;
        let source = read_win64_import_argument(unicorn, 1)?;
        let source_length = read_win64_import_argument(unicorn, 2)? as u32 as i32;
        let destination = read_win64_import_argument(unicorn, 3)?;
        if !matches!(info_type, CT_CTYPE1 | CT_CTYPE2 | CT_CTYPE3)
            || source == 0
            || destination == 0
            || source_length == 0
        {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }

        let mut units = Vec::new();
        if source_length < 0 {
            for index in 0..=(MAX_CRT_STRING_BYTES / 2) {
                let address = source
                    .checked_add(index * 2)
                    .ok_or_else(|| "GetStringTypeW source address overflow".to_string())?;
                let bytes = unicorn
                    .mem_read_as_vec(address, 2)
                    .map_err(|error| format!("GetStringTypeW source read failed: {error}"))?;
                let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
                units.push(unit);
                if unit == 0 {
                    break;
                }
                if index == MAX_CRT_STRING_BYTES / 2 {
                    return Err("GetStringTypeW source is unterminated".into());
                }
            }
        } else {
            let byte_length = u64::try_from(source_length)
                .ok()
                .and_then(|length| length.checked_mul(2))
                .filter(|length| *length <= MAX_CRT_STRING_BYTES)
                .ok_or_else(|| "GetStringTypeW source is too large".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(source, byte_length as usize)
                .map_err(|error| format!("GetStringTypeW source read failed: {error}"))?;
            units.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
            );
        }

        let output_size = units
            .len()
            .checked_mul(2)
            .ok_or_else(|| "GetStringTypeW output byte length overflows".to_string())?;
        let source_end = source
            .checked_add(output_size as u64 - 1)
            .ok_or_else(|| "GetStringTypeW source range overflows".to_string())?;
        let output_end = destination
            .checked_add(output_size as u64 - 1)
            .ok_or_else(|| "GetStringTypeW output range overflows".to_string())?;
        if source <= output_end && destination <= source_end {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }
        let regions = unicorn
            .mem_regions()
            .map_err(|error| format!("GetStringTypeW memory-map query failed: {error}"))?;
        let mut cursor = destination;
        while cursor <= output_end {
            let region = regions
                .iter()
                .find(|region| {
                    region.begin <= cursor
                        && cursor <= region.end
                        && region.perms & Prot::WRITE.0 as u32 != 0
                })
                .ok_or_else(|| {
                    format!(
                        "GetStringTypeW output {destination:#x}..={output_end:#x} is not fully writable"
                    )
                })?;
            if region.end >= output_end {
                break;
            }
            cursor = region
                .end
                .checked_add(1)
                .ok_or_else(|| "GetStringTypeW writable region overflows".to_string())?;
        }

        let output = units
            .into_iter()
            .flat_map(|unit| classify_utf16_unit(info_type, unit).to_le_bytes())
            .collect::<Vec<_>>();
        unicorn.mem_write(destination, &output).map_err(|error| {
            format!("GetStringTypeW output {destination:#x} is not writable: {error}")
        })?;
        Ok((1, None))
    })();

    match result {
        Ok((returned, error)) => {
            if let Some(error) = error {
                unicorn.get_data_mut().windows_last_error = error;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_lc_map_string_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const LCMAP_LOWERCASE: u32 = 0x0000_0100;
    const LCMAP_UPPERCASE: u32 = 0x0000_0200;
    const LCMAP_SORTKEY: u32 = 0x0000_0400;
    const LCMAP_LINGUISTIC_CASING: u32 = 0x0100_0000;
    const NORM_IGNORECASE: u32 = 0x0000_0001;
    const NORM_IGNOREKANATYPE: u32 = 0x0001_0000;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const ERROR_INVALID_FLAGS: u32 = 1004;

    let result = (|| -> Result<(u64, Option<u32>), String> {
        let locale = read_win64_import_argument(unicorn, 0)? as u32;
        let flags = read_win64_import_argument(unicorn, 1)? as u32;
        let source = read_win64_import_argument(unicorn, 2)?;
        let source_length = read_win64_import_argument(unicorn, 3)? as u32 as i32;
        let destination = read_win64_import_argument(unicorn, 4)?;
        let destination_length = read_win64_import_argument(unicorn, 5)? as u32 as i32;

        // Keep the locale contract intentionally small and deterministic. The
        // pseudo-default LCIDs use the same invariant Unicode policy as 0x007f;
        // 0x0411 is admitted for the worker's CP932/Japanese execution path.
        if !matches!(locale, 0 | 0x0400 | 0x0800 | 0x007f | 0x0409 | 0x0411)
            || source == 0
            || source_length == 0
            || destination_length < 0
            || (destination_length != 0 && destination == 0)
        {
            return Ok((0, Some(ERROR_INVALID_PARAMETER)));
        }

        let case_flags = flags & (LCMAP_LOWERCASE | LCMAP_UPPERCASE);
        let is_sort_key = flags & LCMAP_SORTKEY != 0;
        let valid_flags = if is_sort_key {
            LCMAP_SORTKEY | NORM_IGNORECASE | NORM_IGNOREKANATYPE
        } else {
            LCMAP_LOWERCASE | LCMAP_UPPERCASE | LCMAP_LINGUISTIC_CASING
        };
        if flags == 0
            || flags & !valid_flags != 0
            || (!is_sort_key && !matches!(case_flags, LCMAP_LOWERCASE | LCMAP_UPPERCASE))
            || (is_sort_key && case_flags != 0)
        {
            return Ok((0, Some(ERROR_INVALID_FLAGS)));
        }

        let mut source_units = Vec::new();
        if source_length < 0 {
            for index in 0..=(MAX_CRT_STRING_BYTES / 2) {
                let address = source
                    .checked_add(index * 2)
                    .ok_or_else(|| "LCMapStringW source address overflow".to_string())?;
                let bytes = unicorn
                    .mem_read_as_vec(address, 2)
                    .map_err(|error| format!("LCMapStringW source read failed: {error}"))?;
                let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
                source_units.push(unit);
                if unit == 0 {
                    break;
                }
                if index == MAX_CRT_STRING_BYTES / 2 {
                    return Err("LCMapStringW source is unterminated".into());
                }
            }
        } else {
            let byte_length = u64::try_from(source_length)
                .ok()
                .and_then(|length| length.checked_mul(2))
                .filter(|length| *length <= MAX_CRT_STRING_BYTES)
                .ok_or_else(|| "LCMapStringW source is too large".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(source, byte_length as usize)
                .map_err(|error| format!("LCMapStringW source read failed: {error}"))?;
            source_units.extend(
                bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
            );
        }

        let mapping_source = if is_sort_key && source_length < 0 {
            &source_units[..source_units.len().saturating_sub(1)]
        } else {
            &source_units
        };
        let output = if is_sort_key {
            let mut key = make_lcmap_sort_key(locale, flags, mapping_source)?;
            // LCMapStringW's LCMAP_SORTKEY result is an opaque byte sequence
            // terminated by one NUL byte; cchDest and the return are byte counts.
            key.push(0);
            key
        } else {
            map_utf16_case_units(
                mapping_source,
                case_flags,
                flags & LCMAP_LINGUISTIC_CASING != 0,
                locale,
            )
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
        };
        let required = if is_sort_key {
            output.len()
        } else {
            output.len() / 2
        };
        let required = u32::try_from(required)
            .map_err(|_| "LCMapStringW output length exceeds INT".to_string())?;
        if destination_length == 0 {
            return Ok((u64::from(required), None));
        }
        if (destination_length as u32) < required {
            return Ok((0, Some(ERROR_INSUFFICIENT_BUFFER)));
        }

        let source_bytes = source_units
            .len()
            .checked_mul(2)
            .ok_or_else(|| "LCMapStringW source range overflows".to_string())?;
        if !output.is_empty() {
            let source_end = source
                .checked_add(source_bytes as u64 - 1)
                .ok_or_else(|| "LCMapStringW source range overflows".to_string())?;
            let output_end = destination
                .checked_add(output.len() as u64 - 1)
                .ok_or_else(|| "LCMapStringW output range overflows".to_string())?;
            let overlaps = source <= output_end && destination <= source_end;
            let exact_in_place_case_map = !is_sort_key && source == destination;
            if overlaps && !exact_in_place_case_map {
                return Ok((0, Some(ERROR_INVALID_FLAGS)));
            }
            let regions = unicorn
                .mem_regions()
                .map_err(|error| format!("LCMapStringW memory-map query failed: {error}"))?;
            let mut cursor = destination;
            while cursor <= output_end {
                let region = regions
                    .iter()
                    .find(|region| {
                        region.begin <= cursor
                            && cursor <= region.end
                            && region.perms & Prot::WRITE.0 as u32 != 0
                    })
                    .ok_or_else(|| {
                        format!(
                            "LCMapStringW output {destination:#x}..={output_end:#x} is not fully writable"
                        )
                    })?;
                if region.end >= output_end {
                    break;
                }
                cursor = region
                    .end
                    .checked_add(1)
                    .ok_or_else(|| "LCMapStringW writable region overflows".to_string())?;
            }
        }
        unicorn.mem_write(destination, &output).map_err(|error| {
            format!("LCMapStringW output {destination:#x} is not writable: {error}")
        })?;
        Ok((u64::from(required), None))
    })();

    match result {
        Ok((returned, error)) => {
            if let Some(error) = error {
                unicorn.get_data_mut().windows_last_error = error;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn make_lcmap_sort_key(locale: u32, flags: u32, units: &[u16]) -> Result<Vec<u8>, String> {
    use icu_collator::{
        Collator,
        options::{CollatorOptions, Strength},
    };

    const NORM_IGNORECASE: u32 = 0x0000_0001;
    const NORM_IGNOREKANATYPE: u32 = 0x0001_0000;
    let preferences = match locale {
        0x0409 => icu_locale_core::locale!("en-US").into(),
        0x0411 => icu_locale_core::locale!("ja-JP").into(),
        _ => icu_locale_core::locale!("und").into(),
    };
    let mut options = CollatorOptions::default();
    // Identical strength preserves case, kana type, and width by default.
    // Ignore flags are implemented as isolated input transformations below so
    // asking to ignore one distinction cannot erase the others.
    options.strength = Some(Strength::Identical);
    let collator = Collator::try_new(preferences, options)
        .map_err(|error| format!("LCMapStringW could not load pinned collation data: {error}"))?;
    let mut key = Vec::new();
    let mut key_units = if flags & NORM_IGNORECASE != 0 {
        fold_utf16_case_preserving_surrogates(units)
    } else {
        units.to_vec()
    };
    if flags & NORM_IGNOREKANATYPE != 0 {
        key_units = ignore_fullwidth_katakana_type(&key_units);
    }
    collator
        .write_sort_key_utf16_to(&key_units, &mut key)
        .map_err(|error| format!("LCMapStringW could not construct sort key: {error:?}"))?;
    // ICU compares every ill-formed UTF-16 unit as U+FFFD. At Identical
    // strength WinNLS still needs distinct malformed code-unit sequences, so
    // retain a deterministic tie-break suffix only when such units occur.
    let has_malformed = char::decode_utf16(key_units.iter().copied()).any(|unit| unit.is_err());
    if has_malformed {
        key.push(0xff);
        // Encode the full transformed unit sequence, not merely the malformed
        // values. This retains their positions relative to valid U+FFFD units:
        // [D800, FFFD] and [FFFD, D800] must not collapse to one key.
        for unit in key_units {
            key.extend([
                ((unit >> 12) as u8) + 2,
                (((unit >> 8) & 0xf) as u8) + 2,
                (((unit >> 4) & 0xf) as u8) + 2,
                ((unit & 0xf) as u8) + 2,
            ]);
        }
    }
    Ok(key)
}

fn fold_utf16_case_preserving_surrogates(units: &[u16]) -> Vec<u16> {
    let mapper = icu_casemap::CaseMapper::new();
    let mut output = Vec::with_capacity(units.len());
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) => {
                let scalar = character.to_string();
                output.extend(mapper.fold_string(&scalar).encode_utf16());
            }
            Err(error) => output.push(error.unpaired_surrogate()),
        }
    }
    output
}

fn ignore_fullwidth_katakana_type(units: &[u16]) -> Vec<u16> {
    let mut output = Vec::with_capacity(units.len());
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) => match u32::from(character) {
                code @ 0x30a1..=0x30f6 => output.push((code - 0x60) as u16),
                0x30f7..=0x30fa => {
                    // Voiced WA/WI/WE/WO have no precomposed hiragana forms.
                    output.push((0x308f + (u32::from(character) - 0x30f7)) as u16);
                    output.push(0x3099);
                }
                0x30fd => output.push(0x309d),
                0x30fe => output.push(0x309e),
                _ => {
                    let mut encoded = [0; 2];
                    output.extend_from_slice(character.encode_utf16(&mut encoded));
                }
            },
            Err(error) => output.push(error.unpaired_surrogate()),
        }
    }
    output
}

fn map_utf16_case_units(
    units: &[u16],
    case_flag: u32,
    linguistic_casing: bool,
    locale: u32,
) -> Vec<u16> {
    use icu_casemap::CaseMapper;

    const LCMAP_LOWERCASE: u32 = 0x0000_0100;
    let mut output = Vec::with_capacity(units.len());
    let mapper = CaseMapper::new();
    let language = match locale {
        0x0409 => icu_locale_core::langid!("en"),
        0x0411 => icu_locale_core::langid!("ja"),
        _ => icu_locale_core::langid!("und"),
    };
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) if linguistic_casing => {
                // WinNLS casing remains context-insensitive even when the
                // linguistic table is selected. Map one scalar at a time so a
                // Greek sigma never observes its neighbors, while retaining
                // full per-scalar expansions such as sharp-s -> "SS".
                let scalar = character.to_string();
                let mapped = if case_flag == LCMAP_LOWERCASE {
                    mapper.lowercase_to_string(&scalar, &language)
                } else {
                    mapper.uppercase_to_string(&scalar, &language)
                };
                output.extend(mapped.encode_utf16());
            }
            Ok(character) => {
                let mapped = if case_flag == LCMAP_LOWERCASE {
                    mapper.simple_lowercase(character)
                } else {
                    mapper.simple_uppercase(character)
                };
                let mut encoded = [0; 2];
                output.extend_from_slice(mapped.encode_utf16(&mut encoded));
            }
            Err(error) => output.push(error.unpaired_surrogate()),
        }
    }
    output
}

// These predicates intentionally use versioned Unicode data crates rather than
// the host locale. Windows exposes classifications per UTF-16 code unit, so
// surrogate halves remain visible and receive only their documented C3 flags.
fn classify_utf16_unit(info_type: u32, unit: u16) -> u16 {
    use unicode_bidi::BidiClass;
    use unicode_general_category::{GeneralCategory, get_general_category};
    use unicode_script::{Script, UnicodeScript};
    use unicode_width::UnicodeWidthChar;

    const C1_UPPER: u16 = 0x0001;
    const C1_LOWER: u16 = 0x0002;
    const C1_DIGIT: u16 = 0x0004;
    const C1_SPACE: u16 = 0x0008;
    const C1_PUNCT: u16 = 0x0010;
    const C1_CNTRL: u16 = 0x0020;
    const C1_BLANK: u16 = 0x0040;
    const C1_XDIGIT: u16 = 0x0080;
    const C1_ALPHA: u16 = 0x0100;
    const C1_DEFINED: u16 = 0x0200;
    const C2_LEFTTORIGHT: u16 = 0x0001;
    const C2_RIGHTTOLEFT: u16 = 0x0002;
    const C2_EUROPENUMBER: u16 = 0x0003;
    const C2_EUROPESEPARATOR: u16 = 0x0004;
    const C2_EUROPETERMINATOR: u16 = 0x0005;
    const C2_ARABICNUMBER: u16 = 0x0006;
    const C2_COMMONSEPARATOR: u16 = 0x0007;
    const C2_BLOCKSEPARATOR: u16 = 0x0008;
    const C2_SEGMENTSEPARATOR: u16 = 0x0009;
    const C2_WHITESPACE: u16 = 0x000a;
    const C2_OTHERNEUTRAL: u16 = 0x000b;
    const C3_NONSPACING: u16 = 0x0001;
    const C3_DIACRITIC: u16 = 0x0002;
    const C3_VOWELMARK: u16 = 0x0004;
    const C3_SYMBOL: u16 = 0x0008;
    const C3_KATAKANA: u16 = 0x0010;
    const C3_HIRAGANA: u16 = 0x0020;
    const C3_HALFWIDTH: u16 = 0x0040;
    const C3_FULLWIDTH: u16 = 0x0080;
    const C3_IDEOGRAPH: u16 = 0x0100;
    const C3_KASHIDA: u16 = 0x0200;
    const C3_LEXICAL: u16 = 0x0400;
    const C3_HIGHSURROGATE: u16 = 0x0800;
    const C3_LOWSURROGATE: u16 = 0x1000;
    const C3_ALPHA: u16 = 0x8000;

    if info_type == 3 {
        if (0xd800..=0xdbff).contains(&unit) {
            return C3_HIGHSURROGATE;
        }
        if (0xdc00..=0xdfff).contains(&unit) {
            return C3_LOWSURROGATE;
        }
    }
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return 0;
    };
    let code = u32::from(character);
    let category = get_general_category(character);
    let is_punctuation = matches!(
        category,
        GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
    );
    let is_symbol = matches!(
        category,
        GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    );
    match info_type {
        1 => {
            let mut flags = 0;
            if character.is_uppercase() {
                flags |= C1_UPPER;
            }
            if character.is_lowercase() {
                flags |= C1_LOWER;
            }
            if category == GeneralCategory::DecimalNumber {
                flags |= C1_DIGIT;
            }
            if character.is_whitespace() {
                flags |= C1_SPACE;
            }
            if category == GeneralCategory::Control {
                flags |= C1_CNTRL;
            }
            if character == '\t' || category == GeneralCategory::SpaceSeparator {
                flags |= C1_BLANK;
            }
            if character.is_ascii_hexdigit() {
                flags |= C1_XDIGIT;
            }
            if character.is_alphabetic() {
                flags |= C1_ALPHA;
            }
            if is_punctuation || is_symbol {
                flags |= C1_PUNCT;
            }
            // C1_DEFINED is the fallback for an assigned character that has no
            // more specific CTYPE1 attribute; it is not an all-assigned bit.
            if flags == 0 && category != GeneralCategory::Unassigned {
                flags = C1_DEFINED;
            }
            flags
        }
        2 => match unicode_bidi::bidi_class(character) {
            BidiClass::L => C2_LEFTTORIGHT,
            BidiClass::R | BidiClass::AL => C2_RIGHTTOLEFT,
            BidiClass::EN => C2_EUROPENUMBER,
            BidiClass::ES => C2_EUROPESEPARATOR,
            BidiClass::ET => C2_EUROPETERMINATOR,
            BidiClass::AN => C2_ARABICNUMBER,
            BidiClass::CS => C2_COMMONSEPARATOR,
            BidiClass::B => C2_BLOCKSEPARATOR,
            BidiClass::S => C2_SEGMENTSEPARATOR,
            BidiClass::WS => C2_WHITESPACE,
            BidiClass::BN
            | BidiClass::LRE
            | BidiClass::LRO
            | BidiClass::RLE
            | BidiClass::RLO
            | BidiClass::PDF
            | BidiClass::LRI
            | BidiClass::RLI
            | BidiClass::FSI
            | BidiClass::PDI => 0,
            _ => C2_OTHERNEUTRAL,
        },
        3 => {
            let mut flags = 0;
            let combining = unicode_normalization::char::canonical_combining_class(character);
            if category == GeneralCategory::NonspacingMark {
                flags |= C3_NONSPACING;
            }
            if combining != 0 {
                flags |= C3_DIACRITIC;
            }
            if is_dependent_vowel_mark(character) {
                flags |= C3_VOWELMARK;
            }
            if character.is_alphabetic() {
                flags |= C3_ALPHA;
            }
            match character.script() {
                Script::Hiragana => flags |= C3_HIRAGANA,
                Script::Katakana => flags |= C3_KATAKANA,
                Script::Han => flags |= C3_IDEOGRAPH,
                _ => {}
            }
            // Unicode width data covers East Asian W/F characters. The explicit
            // Halfwidth Forms interval preserves Windows' C3_HALFWIDTH signal.
            if is_assigned_halfwidth_form(code, category) {
                flags |= C3_HALFWIDTH;
            }
            if (0xff66..=0xff9f).contains(&code) {
                flags |= C3_KATAKANA;
            }
            if UnicodeWidthChar::width(character) == Some(2) {
                flags |= C3_FULLWIDTH;
            }
            if code == 0x0640 {
                flags |= C3_KASHIDA;
            }
            if is_windows_lexical_character(code, category) {
                flags |= C3_LEXICAL;
            }
            if is_symbol {
                flags |= C3_SYMBOL;
            }
            flags
        }
        _ => 0,
    }
}

fn is_assigned_halfwidth_form(
    code: u32,
    category: unicode_general_category::GeneralCategory,
) -> bool {
    use unicode_general_category::GeneralCategory;

    category != GeneralCategory::Unassigned
        && matches!(
            code,
            0xff61..=0xffbe
                | 0xffc2..=0xffc7
                | 0xffca..=0xffcf
                | 0xffd2..=0xffd7
                | 0xffda..=0xffdc
        )
}

fn is_windows_lexical_character(
    code: u32,
    category: unicode_general_category::GeneralCategory,
) -> bool {
    use unicode_general_category::GeneralCategory;

    // Windows' C3_LEXICAL is narrower than Unicode punctuation: it marks
    // word-forming/joining punctuation. Unicode dash punctuation supplies the
    // versioned dash set; WinNLS additionally treats '=', the feminine and
    // masculine ordinal indicators, and Arabic kashida as lexical. General
    // punctuation such as '!' deliberately remains unmarked.
    category == GeneralCategory::DashPunctuation
        || matches!(code, 0x003d | 0x00aa | 0x00ba | 0x0640)
}

fn is_dependent_vowel_mark(character: char) -> bool {
    use icu_properties::{CodePointMapData, props::IndicSyllabicCategory};

    CodePointMapData::<IndicSyllabicCategory>::new().get(character)
        == IndicSyllabicCategory::VowelDependent
}

fn encode_shift_jis_with_default(input: &str, default: u8) -> (Vec<u8>, bool) {
    let mut output = Vec::with_capacity(input.len());
    let mut used_default = false;
    for character in input.chars() {
        let mut utf8 = [0; 4];
        let source = character.encode_utf8(&mut utf8);
        let mut encoded = [0; 8];
        let (result, read, written) = encoding_rs::SHIFT_JIS
            .new_encoder()
            .encode_from_utf8_without_replacement(source, &mut encoded, true);
        if result == encoding_rs::EncoderResult::InputEmpty && read == source.len() {
            output.extend_from_slice(&encoded[..written]);
        } else {
            output.push(default);
            used_default = true;
        }
    }
    (output, used_default)
}

fn emulate_get_environment_variable_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name_pointer = read_win64_import_argument(unicorn, 0)?;
        let buffer = read_win64_import_argument(unicorn, 1)?;
        let size = read_win64_import_argument(unicorn, 2)? as u32;
        let name = read_windows_environment_name(unicorn, name_pointer, "GetEnvironmentVariableA")?;
        let Some(value) = deterministic_guest_environment_value(&name) else {
            unicorn.get_data_mut().windows_last_error = ERROR_ENVVAR_NOT_FOUND;
            return Ok(0);
        };
        let required = u32::try_from(value.len() + 1)
            .map_err(|_| "GetEnvironmentVariableA value length exceeds DWORD".to_string())?;
        if size < required {
            return Ok(u64::from(required));
        }
        if buffer == 0 {
            return Err("GetEnvironmentVariableA output pointer is null".into());
        }
        let mut terminated = Vec::with_capacity(value.len() + 1);
        terminated.extend_from_slice(value);
        terminated.push(0);
        unicorn.mem_write(buffer, &terminated).map_err(|error| {
            format!("GetEnvironmentVariableA output {buffer:#x} is not writable: {error}")
        })?;
        Ok(value.len() as u64)
    })();
    match result {
        Ok(returned) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_get_environment_variable_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let name_pointer = read_win64_import_argument(unicorn, 0)?;
        let buffer = read_win64_import_argument(unicorn, 1)?;
        let size = read_win64_import_argument(unicorn, 2)? as u32;
        if name_pointer == 0 {
            return Err("GetEnvironmentVariableW name pointer is null".into());
        }
        let mut units = Vec::new();
        for index in 0..=MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
            let address = name_pointer
                .checked_add((index as u64) * 2)
                .ok_or_else(|| "GetEnvironmentVariableW name address overflow".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(address, 2)
                .map_err(|error| format!("GetEnvironmentVariableW name read failed: {error}"))?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                break;
            }
            if index == MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
                return Err(format!(
                    "GetEnvironmentVariableW name exceeds {MAX_WINDOWS_ENVIRONMENT_NAME_BYTES} UTF-16 units"
                ));
            }
            units.push(unit);
        }
        let name = String::from_utf16(&units)
            .map_err(|_| "GetEnvironmentVariableW name is invalid UTF-16".to_string())?;
        let Some(value) = deterministic_guest_environment_value(name.as_bytes()) else {
            unicorn.get_data_mut().windows_last_error = ERROR_ENVVAR_NOT_FOUND;
            return Ok(0);
        };
        let value: Vec<u16> = value.iter().map(|byte| u16::from(*byte)).collect();
        let required = u32::try_from(value.len() + 1)
            .map_err(|_| "GetEnvironmentVariableW value length exceeds DWORD".to_string())?;
        if size < required {
            return Ok(u64::from(required));
        }
        if buffer == 0 {
            return Err("GetEnvironmentVariableW output pointer is null".into());
        }
        let mut terminated = Vec::with_capacity((value.len() + 1) * 2);
        for unit in value.iter().copied().chain(std::iter::once(0)) {
            terminated.extend_from_slice(&unit.to_le_bytes());
        }
        unicorn.mem_write(buffer, &terminated).map_err(|error| {
            format!("GetEnvironmentVariableW output {buffer:#x} is not writable: {error}")
        })?;
        Ok(value.len() as u64)
    })();
    match result {
        Ok(returned) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_windows_last_error(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        match operation {
            LegacyWin64Import::GetLastError => Ok(u64::from(unicorn.get_data().windows_last_error)),
            LegacyWin64Import::SetLastError => {
                unicorn.get_data_mut().windows_last_error =
                    read_win64_import_argument(unicorn, 0)? as u32;
                Ok(0)
            }
            _ => Err("invalid last-error operation".into()),
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

fn emulate_set_thread_error_mode(unicorn: &mut Unicorn<'_, GuestState>) {
    const VALID_ERROR_MODE_FLAGS: u32 = 0x0000_8003;
    let result = (|| -> Result<bool, String> {
        let new_mode = read_win64_import_argument(unicorn, 0)? as u32;
        let old_mode_output = read_win64_import_argument(unicorn, 1)?;
        if new_mode & !VALID_ERROR_MODE_FLAGS != 0 {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            return Ok(false);
        }
        let old_mode = unicorn.get_data().windows_thread_error_mode;
        if old_mode_output != 0 {
            unicorn
                .mem_write(old_mode_output, &old_mode.to_le_bytes())
                .map_err(|error| {
                    format!(
                        "SetThreadErrorMode old-mode output {old_mode_output:#x} is not writable: {error}"
                    )
                })?;
        }
        unicorn.get_data_mut().windows_thread_error_mode = new_mode;
        Ok(true)
    })();
    match result {
        Ok(success) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(success));
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_load_library_ex_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;
    const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x0000_0100;
    const LOAD_LIBRARY_SEARCH_FLAGS: u32 = 0x0000_1f00;
    const NON_EXECUTABLE_RESOURCE_FLAGS: u32 = 0x0000_0062;
    const VALID_FLAGS: u32 = 0x0000_3ffb;
    let result = (|| -> Result<Option<u64>, String> {
        let path_pointer = read_win64_import_argument(unicorn, 0)?;
        let file_handle = read_win64_import_argument(unicorn, 1)?;
        let flags = read_win64_import_argument(unicorn, 2)? as u32;
        if path_pointer == 0
            || file_handle != 0
            || flags & !VALID_FLAGS != 0
            || flags & NON_EXECUTABLE_RESOURCE_FLAGS != 0
            || flags & LOAD_WITH_ALTERED_SEARCH_PATH != 0 && flags & LOAD_LIBRARY_SEARCH_FLAGS != 0
        {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            return Ok(None);
        }
        let mut units = Vec::new();
        let mut terminated = false;
        for index in 0..260u64 {
            let address = path_pointer
                .checked_add(index * 2)
                .ok_or_else(|| "LoadLibraryExW path address overflow".to_string())?;
            let bytes = unicorn
                .mem_read_as_vec(address, 2)
                .map_err(|error| format!("LoadLibraryExW path read failed: {error}"))?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                terminated = true;
                break;
            }
            units.push(unit);
        }
        if !terminated {
            return Err("LoadLibraryExW path exceeds 259 UTF-16 code units".into());
        }
        let path = String::from_utf16(&units)
            .map_err(|_| "LoadLibraryExW path is not valid UTF-16".to_string())?;
        let path_bytes = path.as_bytes();
        let drive_absolute = path_bytes.len() >= 4
            && path_bytes[0].is_ascii_alphabetic()
            && path_bytes[1] == b':'
            && matches!(path_bytes[2], b'\\' | b'/');
        let unc_absolute = path_bytes.len() >= 2
            && matches!(path_bytes[0], b'\\' | b'/')
            && path_bytes[1] == path_bytes[0]
            && path[2..]
                .split(['\\', '/'])
                .take(3)
                .collect::<Vec<_>>()
                .as_slice()
                .iter()
                .all(|component| !component.is_empty())
            && path[2..].split(['\\', '/']).count() >= 3;
        let absolute = drive_absolute || unc_absolute;
        if flags & (LOAD_WITH_ALTERED_SEARCH_PATH | LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR) != 0
            && !absolute
        {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            return Ok(None);
        }
        let Some(module) = path
            .rsplit(['\\', '/'])
            .next()
            .filter(|name| !name.is_empty())
        else {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            return Ok(None);
        };
        if module.eq_ignore_ascii_case("kernel32.dll") {
            Ok(Some(WINDOWS_KERNEL32_MODULE_TOKEN))
        } else {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            Ok(None)
        }
    })();
    match result {
        Ok(module) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, module.unwrap_or_default());
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_load_library_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<Option<u64>, String> {
        let path_pointer = read_win64_import_argument(unicorn, 0)?;
        if path_pointer == 0 {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            return Ok(None);
        }
        let mut path = Vec::new();
        let mut terminated = false;
        for index in 0..260u64 {
            let address = path_pointer
                .checked_add(index)
                .ok_or_else(|| "LoadLibraryA path address overflow".to_string())?;
            let byte = unicorn
                .mem_read_as_vec(address, 1)
                .map_err(|error| format!("LoadLibraryA path read failed: {error}"))?[0];
            if byte == 0 {
                terminated = true;
                break;
            }
            path.push(byte);
        }
        if !terminated {
            return Err("LoadLibraryA path exceeds 259 bytes".into());
        }
        let Some(module) = path
            .rsplit(|byte| matches!(byte, b'\\' | b'/'))
            .next()
            .filter(|name| !name.is_empty())
        else {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            return Ok(None);
        };
        if module.eq_ignore_ascii_case(b"kernel32.dll") {
            Ok(Some(WINDOWS_KERNEL32_MODULE_TOKEN))
        } else {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            Ok(None)
        }
    })();
    match result {
        Ok(module) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, module.unwrap_or_default());
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            let _ = unicorn.emu_stop();
        }
    }
}

fn read_msvcp_exception_ptr(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    label: &str,
) -> Result<[u8; MSVCP_EXCEPTION_PTR_BYTES], String> {
    if address == 0 {
        return Err(format!("MSVC exception_ptr {label} pointer is null"));
    }
    let mut bytes = [0u8; MSVCP_EXCEPTION_PTR_BYTES];
    unicorn
        .mem_read(address, &mut bytes)
        .map_err(|error| format!("MSVC exception_ptr {label} read failed: {error}"))?;
    Ok(bytes)
}

fn require_null_msvcp_exception_ptr(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    label: &str,
) -> Result<(), String> {
    let bytes = read_msvcp_exception_ptr(unicorn, address, label)?;
    if bytes != [0; MSVCP_EXCEPTION_PTR_BYTES] {
        return Err(format!(
            "MSVC exception_ptr {label} contains an unmodeled non-null exception reference"
        ));
    }
    Ok(())
}

fn write_null_msvcp_exception_ptr(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    label: &str,
) -> Result<(), String> {
    if address == 0 {
        return Err(format!("MSVC exception_ptr {label} pointer is null"));
    }
    unicorn
        .mem_write(address, &[0; MSVCP_EXCEPTION_PTR_BYTES])
        .map_err(|error| format!("MSVC exception_ptr {label} write failed: {error}"))
}

fn emulate_msvcp_exception_ptr(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    let result = (|| -> Result<(), String> {
        let destination = read_win64_import_argument(unicorn, 0)?;
        match operation {
            LegacyWin64Import::MsvcpExceptionPtrCreate => {
                write_null_msvcp_exception_ptr(unicorn, destination, "create destination")
            }
            LegacyWin64Import::MsvcpExceptionPtrCopy => {
                let source = read_win64_import_argument(unicorn, 1)?;
                require_null_msvcp_exception_ptr(unicorn, source, "copy source")?;
                write_null_msvcp_exception_ptr(unicorn, destination, "copy destination")
            }
            LegacyWin64Import::MsvcpExceptionPtrAssign => {
                let source = read_win64_import_argument(unicorn, 1)?;
                require_null_msvcp_exception_ptr(unicorn, destination, "assign destination")?;
                require_null_msvcp_exception_ptr(unicorn, source, "assign source")?;
                write_null_msvcp_exception_ptr(unicorn, destination, "assign destination")
            }
            LegacyWin64Import::MsvcpExceptionPtrDestroy => {
                require_null_msvcp_exception_ptr(unicorn, destination, "destroy object")?;
                write_null_msvcp_exception_ptr(unicorn, destination, "destroy object")
            }
            LegacyWin64Import::MsvcpExceptionPtrCurrentException => {
                require_null_msvcp_exception_ptr(
                    unicorn,
                    destination,
                    "current-exception destination",
                )?;
                // The single-thread backend has no modeled in-flight SEH/C++
                // exception at this boundary, so the documented result is null.
                write_null_msvcp_exception_ptr(
                    unicorn,
                    destination,
                    "current-exception destination",
                )
            }
            LegacyWin64Import::MsvcpExceptionPtrRethrow => {
                require_null_msvcp_exception_ptr(unicorn, destination, "rethrow source")?;
                Err("MSVC exception_ptr rethrow of null would raise bad_exception; exception transport is not modeled".into())
            }
            _ => Err("invalid MSVC exception_ptr operation".into()),
        }
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_crt_set_terminate(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let handler = read_win64_import_argument(unicorn, 0)?;
        if handler != 0 && !image_executable_address(unicorn.get_data(), handler) {
            return Err(format!(
                "CRT terminate handler {handler:#x} is outside the executable image"
            ));
        }
        let previous = unicorn.get_data().crt_terminate_handler;
        unicorn.get_data_mut().crt_terminate_handler = handler;
        unicorn
            .reg_write(RegisterX86::RAX, previous)
            .map_err(|error| format!("CRT set_terminate result write failed: {error}"))
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
                        format!(
                            "Windows critical-section object {address:#x} is not writable: {error}"
                        )
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
                *lock_count = lock_count
                    .checked_add(1)
                    .ok_or_else(|| "Windows critical-section recursion overflow".to_string())?;
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

fn emulate_get_module_handle_ex_a(unicorn: &mut Unicorn<'_, GuestState>) {
    const PIN: u32 = 0x1;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    const FROM_ADDRESS: u32 = 0x4;
    const VALID_FLAGS: u32 = PIN | UNCHANGED_REFCOUNT | FROM_ADDRESS;

    let flags = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX) as u32;
    let name_or_address = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    };

    if flags & !VALID_FLAGS != 0
        || flags & PIN != 0 && flags & UNCHANGED_REFCOUNT != 0
        || output == 0
    {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }

    let module = if flags & FROM_ADDRESS != 0 {
        unicorn
            .get_data()
            .image_region
            .filter(|(start, end)| (*start..*end).contains(&name_or_address))
            .map(|(start, _)| start)
    } else if name_or_address == 0 {
        unicorn.get_data().image_region.map(|(start, _)| start)
    } else {
        let mut bytes = Vec::new();
        let mut terminated = false;
        for index in 0..128u64 {
            let Some(address) = name_or_address.checked_add(index) else {
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
        if terminated && bytes.eq_ignore_ascii_case(b"kernel32.dll") {
            Some(WINDOWS_KERNEL32_MODULE_TOKEN)
        } else {
            None
        }
    };

    let Some(module) = module else {
        fail(unicorn, ERROR_MOD_NOT_FOUND);
        return;
    };
    if unicorn.mem_write(output, &module.to_le_bytes()).is_err() {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    // Guest images and synthetic system modules live for the worker lifetime,
    // so default, PIN, and UNCHANGED_REFCOUNT all preserve the same stable
    // handle while retaining their documented lookup behavior.
    let _ = unicorn.reg_write(RegisterX86::RAX, 1);
}

fn emulate_get_module_file_name_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let module = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
    let output = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let capacity = unicorn.reg_read(RegisterX86::R8).unwrap_or_default() as u32;
    let image_base = unicorn.get_data().image_region.map(|region| region.0);
    let path = if (module == 0 && image_base.is_some()) || Some(module) == image_base {
        Some(r"C:\AEXCompat\guest-plugin.aex")
    } else if module == WINDOWS_KERNEL32_MODULE_TOKEN {
        Some(r"C:\Windows\System32\kernel32.dll")
    } else {
        None
    };
    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    };
    let Some(path) = path else {
        fail(unicorn, ERROR_MOD_NOT_FOUND);
        return;
    };
    if output == 0 {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    if capacity == 0 {
        fail(unicorn, ERROR_INSUFFICIENT_BUFFER);
        return;
    }

    let units = path.encode_utf16().collect::<Vec<_>>();
    let truncated = units.len() + 1 > capacity as usize;
    let copied = units.len().min(capacity.saturating_sub(1) as usize);
    let mut bytes = Vec::with_capacity((copied + 1) * 2);
    for unit in &units[..copied] {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    if unicorn.mem_write(output, &bytes).is_err() {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    if truncated {
        unicorn.get_data_mut().windows_last_error = ERROR_INSUFFICIENT_BUFFER;
        let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(capacity));
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, copied as u64);
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

fn emulate_windows_condition_variable(unicorn: &mut Unicorn<'_, GuestState>, operation: u8) {
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
                ensure_windows_condition_variable(unicorn, address)?;
                Err("blocking SleepConditionVariableCS is unsupported in the serial backend".into())
            }
            2 | 3 => {
                ensure_windows_condition_variable(unicorn, address)?;
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

fn ensure_windows_condition_variable(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
) -> Result<(), String> {
    if unicorn
        .get_data()
        .windows_condition_variables
        .contains(&address)
    {
        return Ok(());
    }
    if unicorn.get_data().windows_condition_variables.len() >= MAX_WINDOWS_CONDITION_VARIABLES {
        return Err(format!(
            "Windows condition-variable count exceeds {MAX_WINDOWS_CONDITION_VARIABLES}"
        ));
    }
    let bytes = unicorn.mem_read_as_vec(address, 8).map_err(|error| {
        format!("Windows condition variable {address:#x} is unreadable: {error}")
    })?;
    if bytes != [0; 8] {
        return Err(format!(
            "Windows condition variable {address:#x} is neither initialized nor zero-initialized"
        ));
    }
    // CONDITION_VARIABLE_INIT is all-zero and is a complete supported
    // initialization path on Windows.  Writing the same bytes validates that
    // the guest object is writable before retaining it as live state.
    unicorn.mem_write(address, &[0; 8]).map_err(|error| {
        format!("Windows condition variable {address:#x} is not writable: {error}")
    })?;
    unicorn
        .get_data_mut()
        .windows_condition_variables
        .insert(address);
    Ok(())
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

fn emulate_get_system_info(unicorn: &mut Unicorn<'_, GuestState>) {
    const PROCESSOR_ARCHITECTURE_AMD64: u16 = 9;
    const PROCESSOR_AMD_X8664: u32 = 8664;
    const WINDOWS_MAXIMUM_APPLICATION_ADDRESS: u64 = 0x0000_7fff_fffe_ffff;
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("GetSystemInfo output pointer is null".to_string());
        }
        let mut info = [0u8; 48];
        info[0..2].copy_from_slice(&PROCESSOR_ARCHITECTURE_AMD64.to_le_bytes());
        info[4..8].copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
        info[8..16].copy_from_slice(&0x1_0000u64.to_le_bytes());
        info[16..24].copy_from_slice(&WINDOWS_MAXIMUM_APPLICATION_ADDRESS.to_le_bytes());
        info[24..32].copy_from_slice(&1u64.to_le_bytes());
        info[32..36].copy_from_slice(&1u32.to_le_bytes());
        info[36..40].copy_from_slice(&PROCESSOR_AMD_X8664.to_le_bytes());
        info[40..44].copy_from_slice(&0x1_0000u32.to_le_bytes());
        info[44..46].copy_from_slice(&6u16.to_le_bytes());
        unicorn
            .mem_write(output, &info)
            .map_err(|error| format!("GetSystemInfo output {output:#x} is not writable: {error}"))
    });
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
    }
}

fn emulate_get_startup_info_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const STARTUP_INFO_W_SIZE: usize = 104;
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("GetStartupInfoW output pointer is null".into());
        }
        let output_end = output
            .checked_add(STARTUP_INFO_W_SIZE as u64 - 1)
            .ok_or_else(|| "GetStartupInfoW output range overflows".to_string())?;
        let regions = unicorn
            .mem_regions()
            .map_err(|error| format!("GetStartupInfoW memory-map query failed: {error}"))?;
        let mut cursor = output;
        while cursor <= output_end {
            let region = regions
                .iter()
                .find(|region| {
                    region.begin <= cursor
                        && cursor <= region.end
                        && region.perms & Prot::WRITE.0 as u32 != 0
                })
                .ok_or_else(|| {
                    format!(
                        "GetStartupInfoW output {output:#x}..={output_end:#x} is not fully writable"
                    )
                })?;
            if region.end >= output_end {
                break;
            }
            cursor = region
                .end
                .checked_add(1)
                .ok_or_else(|| "GetStartupInfoW writable region overflows".to_string())?;
        }
        let mut startup_info = [0u8; STARTUP_INFO_W_SIZE];
        startup_info[..4].copy_from_slice(&(STARTUP_INFO_W_SIZE as u32).to_le_bytes());
        unicorn
            .mem_write(output, &startup_info)
            .map_err(|error| format!("GetStartupInfoW output {output:#x} is not writable: {error}"))
    });
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_rtl_capture_context(unicorn: &mut Unicorn<'_, GuestState>) {
    const CONTEXT_SIZE: usize = 0x4d0;
    const CONTEXT_AMD64_FULL_WITH_SEGMENTS: u32 = 0x0010_000f;
    const INTEGER_REGISTERS: [(RegisterX86, usize); 16] = [
        (RegisterX86::RAX, 0x78),
        (RegisterX86::RCX, 0x80),
        (RegisterX86::RDX, 0x88),
        (RegisterX86::RBX, 0x90),
        (RegisterX86::RSP, 0x98),
        (RegisterX86::RBP, 0xa0),
        (RegisterX86::RSI, 0xa8),
        (RegisterX86::RDI, 0xb0),
        (RegisterX86::R8, 0xb8),
        (RegisterX86::R9, 0xc0),
        (RegisterX86::R10, 0xc8),
        (RegisterX86::R11, 0xd0),
        (RegisterX86::R12, 0xd8),
        (RegisterX86::R13, 0xe0),
        (RegisterX86::R14, 0xe8),
        (RegisterX86::R15, 0xf0),
    ];
    const SEGMENT_REGISTERS: [(RegisterX86, usize); 6] = [
        (RegisterX86::CS, 0x38),
        (RegisterX86::DS, 0x3a),
        (RegisterX86::ES, 0x3c),
        (RegisterX86::FS, 0x3e),
        (RegisterX86::GS, 0x40),
        (RegisterX86::SS, 0x42),
    ];
    const XMM_REGISTERS: [RegisterX86; 16] = [
        RegisterX86::XMM0,
        RegisterX86::XMM1,
        RegisterX86::XMM2,
        RegisterX86::XMM3,
        RegisterX86::XMM4,
        RegisterX86::XMM5,
        RegisterX86::XMM6,
        RegisterX86::XMM7,
        RegisterX86::XMM8,
        RegisterX86::XMM9,
        RegisterX86::XMM10,
        RegisterX86::XMM11,
        RegisterX86::XMM12,
        RegisterX86::XMM13,
        RegisterX86::XMM14,
        RegisterX86::XMM15,
    ];
    const X87_REGISTERS: [RegisterX86; 8] = [
        RegisterX86::ST0,
        RegisterX86::ST1,
        RegisterX86::ST2,
        RegisterX86::ST3,
        RegisterX86::ST4,
        RegisterX86::ST5,
        RegisterX86::ST6,
        RegisterX86::ST7,
    ];

    let result = (|| -> Result<(), String> {
        let output = read_win64_import_argument(unicorn, 0)?;
        if output == 0 {
            return Err("RtlCaptureContext output pointer is null".into());
        }
        let output_end = output
            .checked_add(CONTEXT_SIZE as u64 - 1)
            .ok_or_else(|| "RtlCaptureContext output range overflows".to_string())?;
        let regions = unicorn
            .mem_regions()
            .map_err(|error| format!("RtlCaptureContext memory-map query failed: {error}"))?;
        let mut cursor = output;
        while cursor <= output_end {
            let region = regions
                .iter()
                .find(|region| {
                    region.begin <= cursor
                        && cursor <= region.end
                        && region.perms & Prot::WRITE.0 as u32 != 0
                })
                .ok_or_else(|| {
                    format!(
                        "RtlCaptureContext output {output:#x}..={output_end:#x} is not fully writable"
                    )
                })?;
            if region.end >= output_end {
                break;
            }
            cursor = region
                .end
                .checked_add(1)
                .ok_or_else(|| "RtlCaptureContext writable region overflows".to_string())?;
        }

        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("RtlCaptureContext could not read RSP: {error}"))?;
        let caller_rsp = rsp
            .checked_add(8)
            .ok_or_else(|| "RtlCaptureContext caller RSP overflows".to_string())?;
        let mut return_address = [0u8; 8];
        unicorn
            .mem_read(rsp, &mut return_address)
            .map_err(|error| {
                format!("RtlCaptureContext return address is not readable: {error}")
            })?;

        let mut context = [0u8; CONTEXT_SIZE];
        context[0x30..0x34].copy_from_slice(&CONTEXT_AMD64_FULL_WITH_SEGMENTS.to_le_bytes());
        let mxcsr = unicorn
            .reg_read(RegisterX86::MXCSR)
            .map_err(|error| format!("RtlCaptureContext could not read MXCSR: {error}"))?
            as u32;
        context[0x34..0x38].copy_from_slice(&mxcsr.to_le_bytes());
        context[0x118..0x11c].copy_from_slice(&mxcsr.to_le_bytes());
        let fpcw = unicorn
            .reg_read(RegisterX86::FPCW)
            .map_err(|error| format!("RtlCaptureContext could not read FPCW: {error}"))?
            as u16;
        let fpsw = unicorn
            .reg_read(RegisterX86::FPSW)
            .map_err(|error| format!("RtlCaptureContext could not read FPSW: {error}"))?
            as u16;
        let full_fptag = unicorn
            .reg_read(RegisterX86::FPTAG)
            .map_err(|error| format!("RtlCaptureContext could not read FPTAG: {error}"))?
            as u16;
        let abridged_fptag = (0..8).fold(0u8, |tag, physical_index| {
            let full_tag = (full_fptag >> (physical_index * 2)) & 0x3;
            tag | u8::from(full_tag != 0x3) << physical_index
        });
        context[0x100..0x102].copy_from_slice(&fpcw.to_le_bytes());
        context[0x102..0x104].copy_from_slice(&fpsw.to_le_bytes());
        context[0x104] = abridged_fptag;
        for (register, offset) in [
            (RegisterX86::FOP, 0x106),
            (RegisterX86::FCS, 0x10c),
            (RegisterX86::FDS, 0x114),
        ] {
            let value = unicorn.reg_read(register).map_err(|error| {
                format!("RtlCaptureContext could not read {register:?}: {error}")
            })? as u16;
            context[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (register, offset) in [(RegisterX86::FIP, 0x108), (RegisterX86::FDP, 0x110)] {
            let value = unicorn.reg_read(register).map_err(|error| {
                format!("RtlCaptureContext could not read {register:?}: {error}")
            })? as u32;
            context[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        context[0x11c..0x120].copy_from_slice(&0x0000_ffffu32.to_le_bytes());
        for (register, offset) in SEGMENT_REGISTERS {
            let value = unicorn.reg_read(register).map_err(|error| {
                format!("RtlCaptureContext could not read {register:?}: {error}")
            })? as u16;
            context[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let eflags = unicorn
            .reg_read(RegisterX86::EFLAGS)
            .map_err(|error| format!("RtlCaptureContext could not read EFLAGS: {error}"))?
            as u32;
        context[0x44..0x48].copy_from_slice(&eflags.to_le_bytes());
        for (register, offset) in INTEGER_REGISTERS {
            let value = if register == RegisterX86::RSP {
                caller_rsp
            } else {
                unicorn.reg_read(register).map_err(|error| {
                    format!("RtlCaptureContext could not read {register:?}: {error}")
                })?
            };
            context[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        context[0xf8..0x100].copy_from_slice(&return_address);
        for (logical_index, register) in X87_REGISTERS.into_iter().enumerate() {
            let value = unicorn.reg_read_long(register).map_err(|error| {
                format!("RtlCaptureContext could not read {register:?}: {error}")
            })?;
            let offset = 0x120 + logical_index * 16;
            context[offset..offset + 10].copy_from_slice(&value);
        }
        for (index, register) in XMM_REGISTERS.into_iter().enumerate() {
            let value = unicorn.reg_read_long(register).map_err(|error| {
                format!("RtlCaptureContext could not read {register:?}: {error}")
            })?;
            let offset = 0x1a0 + index * 16;
            context[offset..offset + 16].copy_from_slice(&value);
        }
        unicorn.mem_write(output, &context).map_err(|error| {
            format!("RtlCaptureContext output {output:#x} is not writable: {error}")
        })
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_get_cp_info(unicorn: &mut Unicorn<'_, GuestState>) {
    const CP_ACP: u32 = 0;
    const CP_SHIFT_JIS: u32 = 932;
    const CP_INFO_SIZE: usize = 20;
    let result = (|| {
        let code_page = read_win64_import_argument(unicorn, 0)? as u32;
        let output = read_win64_import_argument(unicorn, 1)?;
        if code_page != CP_ACP && code_page != CP_SHIFT_JIS {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("GetCPInfo could not write failure result: {error}"))?;
            return Ok(());
        }
        if output == 0 {
            return Err("GetCPInfo output pointer is null".to_string());
        }
        let output_end = output
            .checked_add(CP_INFO_SIZE as u64 - 1)
            .ok_or_else(|| "GetCPInfo output range overflows".to_string())?;
        let regions = unicorn
            .mem_regions()
            .map_err(|error| format!("GetCPInfo memory-map query failed: {error}"))?;
        let mut cursor = output;
        while cursor <= output_end {
            let region = regions
                .iter()
                .find(|region| {
                    region.begin <= cursor
                        && cursor <= region.end
                        && region.perms & Prot::WRITE.0 as u32 != 0
                })
                .ok_or_else(|| {
                    format!("GetCPInfo output {output:#x}..={output_end:#x} is not fully writable")
                })?;
            if region.end >= output_end {
                break;
            }
            cursor = region
                .end
                .checked_add(1)
                .ok_or_else(|| "GetCPInfo writable region overflows".to_string())?;
        }
        let mut info = [0u8; CP_INFO_SIZE];
        info[0..4].copy_from_slice(&2u32.to_le_bytes());
        info[4] = b'?';
        info[6..10].copy_from_slice(&[0x81, 0x9f, 0xe0, 0xfc]);
        unicorn
            .mem_write(output, &info)
            .map_err(|error| format!("GetCPInfo output {output:#x} is not writable: {error}"))?;
        unicorn
            .reg_write(RegisterX86::RAX, 1)
            .map_err(|error| format!("GetCPInfo could not write success result: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        let _ = unicorn.emu_stop();
    }
}

fn emulate_get_std_handle(unicorn: &mut Unicorn<'_, GuestState>) {
    const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
    const INVALID_HANDLE_VALUE: u64 = u64::MAX;
    let selector = read_win64_import_argument(unicorn, 0).unwrap_or_default() as u32;
    let returned = match selector {
        STD_INPUT_HANDLE => WINDOWS_STANDARD_INPUT_TOKEN,
        STD_OUTPUT_HANDLE => WINDOWS_STANDARD_OUTPUT_TOKEN,
        STD_ERROR_HANDLE => WINDOWS_STANDARD_ERROR_TOKEN,
        _ => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
            INVALID_HANDLE_VALUE
        }
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned);
}

fn emulate_get_console_mode(unicorn: &mut Unicorn<'_, GuestState>) {
    let _handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
    let _mode_output = read_win64_import_argument(unicorn, 1).unwrap_or_default();
    // The synthetic standard streams intentionally model redirected endpoints,
    // not host console buffers. Windows reports ERROR_INVALID_HANDLE when
    // GetConsoleMode is used on a redirected pipe/file handle and leaves the
    // caller's mode storage untouched.
    unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_get_file_type(unicorn: &mut Unicorn<'_, GuestState>) {
    const FILE_TYPE_UNKNOWN: u64 = 0;
    const FILE_TYPE_PIPE: u64 = 3;
    let handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
    let returned = if matches!(
        handle,
        WINDOWS_STANDARD_INPUT_TOKEN | WINDOWS_STANDARD_OUTPUT_TOKEN | WINDOWS_STANDARD_ERROR_TOKEN
    ) {
        FILE_TYPE_PIPE
    } else {
        unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
        FILE_TYPE_UNKNOWN
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned);
}

fn emulate_get_command_line_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = unicorn.get_data().windows_command_line_a;
    if pointer == 0 {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error =
                Some("GetCommandLineA process string is not initialized".into());
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        let _ = unicorn.emu_stop();
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_get_command_line_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = unicorn.get_data().windows_command_line_w;
    if pointer == 0 {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error =
                Some("GetCommandLineW process string is not initialized".into());
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        let _ = unicorn.emu_stop();
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_process_prng(unicorn: &mut Unicorn<'_, GuestState>) {
    // ProcessPrng fills a guest buffer with process-scoped random bytes.
    // Use a deterministic process-local stream so calls receive distinct bytes
    // while render results remain reproducible across hosts.
    let result = (|| -> Result<(), String> {
        let buffer = read_win64_import_argument(unicorn, 0)?;
        let length = read_win64_import_argument(unicorn, 1)?;
        if length == 0 {
            return Ok(());
        }
        if buffer == 0 {
            return Err("ProcessPrng buffer pointer is null".to_string());
        }
        if length > MAX_PROCESS_PRNG_BYTES {
            return Err(format!(
                "ProcessPrng length {length} exceeds {MAX_PROCESS_PRNG_BYTES}"
            ));
        }
        let mut bytes = vec![0u8; length as usize];
        let state = &mut unicorn.get_data_mut().process_prng_state;
        for chunk in bytes.chunks_mut(8) {
            // SplitMix64 is compact, deterministic, and adequate for emulating
            // the independent seeds expected by CRT and container internals.
            *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut value = *state;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^= value >> 31;
            chunk.copy_from_slice(&value.to_le_bytes()[..chunk.len()]);
        }
        unicorn
            .mem_write(buffer, &bytes)
            .map_err(|error| format!("ProcessPrng buffer {buffer:#x} is not writable: {error}"))
    })();
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

fn fail_process_heap(unicorn: &mut Unicorn<'_, GuestState>, error: String) {
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(error);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    let _ = unicorn.emu_stop();
}

fn require_process_heap_handle(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: &str,
) -> Result<(), String> {
    let handle = read_win64_import_argument(unicorn, 0)?;
    if handle != PROCESS_HEAP_HANDLE {
        return Err(format!(
            "{operation} rejected unknown heap handle {handle:#x}"
        ));
    }
    Ok(())
}

fn read_process_heap_flags(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: &str,
    allowed: u32,
) -> Result<u32, String> {
    let flags = u32::try_from(read_win64_import_argument(unicorn, 1)?)
        .map_err(|_| format!("{operation} flags exceed DWORD"))?;
    let unsupported = flags & !allowed;
    if unsupported != 0 {
        let generate = flags & HEAP_GENERATE_EXCEPTIONS != 0;
        return Err(format!(
            "{operation} flags {flags:#x} include unsupported bits {unsupported:#x}{}",
            if generate {
                " (HEAP_GENERATE_EXCEPTIONS is not modeled)"
            } else {
                ""
            }
        ));
    }
    Ok(flags)
}

fn emulate_process_heap(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    match operation {
        LegacyWin64Import::HeapAlloc => emulate_heap_alloc(unicorn),
        LegacyWin64Import::HeapFree => emulate_heap_free(unicorn),
        LegacyWin64Import::HeapReAlloc => emulate_heap_realloc(unicorn),
        _ => fail_process_heap(unicorn, "invalid process heap operation".into()),
    }
}

fn allocate_process_heap_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    size: u64,
) -> Result<u64, CrtHeapError> {
    let allocation = unicorn
        .get_data()
        .crt_heap
        .prepare_process_heap_allocation(size)?;
    let pointer = unicorn
        .get_data()
        .crt_heap
        .first_fit(CRT_HEAP_BASE, CRT_HEAP_END, allocation)?;
    unicorn
        .mem_map(pointer, allocation.backing_size, Prot::READ | Prot::WRITE)
        .map_err(|_| CrtHeapError::AddressSpaceExhausted)?;
    if let Err(error) = unicorn.get_data_mut().crt_heap.insert(pointer, allocation) {
        let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
        return Err(error);
    }
    Ok(pointer)
}

fn free_process_heap_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    pointer: u64,
) -> Result<(), String> {
    let allocation = unicorn
        .get_data_mut()
        .crt_heap
        .remove_process_heap(pointer)
        .map_err(|error| error.to_string())?;
    unicorn
        .mem_unmap(pointer, allocation.backing_size)
        .map_err(|error| format!("unmap process heap allocation {pointer:#x}: {error}"))
}

fn emulate_heap_alloc(unicorn: &mut Unicorn<'_, GuestState>) {
    let arguments = (|| -> Result<(u32, u64), String> {
        require_process_heap_handle(unicorn, "HeapAlloc")?;
        let flags = read_process_heap_flags(unicorn, "HeapAlloc", HEAP_ALLOC_ALLOWED_FLAGS)?;
        let size = read_win64_import_argument(unicorn, 2)?;
        Ok((flags, size))
    })();
    let (flags, size) = match arguments {
        Ok(arguments) => arguments,
        Err(error) => return fail_process_heap(unicorn, error),
    };

    let pointer = match allocate_process_heap_region(unicorn, size) {
        Ok(pointer) => pointer,
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    };
    if flags & HEAP_ZERO_MEMORY != 0 {
        let length = size.max(1) as usize;
        if let Err(error) = unicorn.mem_write(pointer, &vec![0; length]) {
            let _ = free_process_heap_region(unicorn, pointer);
            return fail_process_heap(unicorn, format!("HeapAlloc zero-fill failed: {error}"));
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_heap_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = match (|| -> Result<u64, String> {
        require_process_heap_handle(unicorn, "HeapFree")?;
        let _ = read_process_heap_flags(unicorn, "HeapFree", HEAP_NO_SERIALIZE)?;
        read_win64_import_argument(unicorn, 2)
    })() {
        Ok(pointer) => pointer,
        Err(error) => return fail_process_heap(unicorn, error),
    };
    if pointer == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        return;
    }
    match free_process_heap_region(unicorn, pointer) {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        Err(error) => fail_process_heap(unicorn, format!("HeapFree failed: {error}")),
    }
}

fn emulate_heap_realloc(unicorn: &mut Unicorn<'_, GuestState>) {
    let arguments = (|| -> Result<(u32, u64, u64), String> {
        require_process_heap_handle(unicorn, "HeapReAlloc")?;
        let flags = read_process_heap_flags(unicorn, "HeapReAlloc", HEAP_REALLOC_ALLOWED_FLAGS)?;
        let pointer = read_win64_import_argument(unicorn, 2)?;
        let size = read_win64_import_argument(unicorn, 3)?;
        if pointer == 0 {
            return Err("HeapReAlloc pointer is null".into());
        }
        Ok((flags, pointer, size))
    })();
    let (flags, pointer, size) = match arguments {
        Ok(arguments) => arguments,
        Err(error) => return fail_process_heap(unicorn, error),
    };

    let old = match unicorn.get_data().crt_heap.process_heap_allocation(pointer) {
        Ok(allocation) => allocation,
        Err(error) => return fail_process_heap(unicorn, format!("HeapReAlloc failed: {error}")),
    };
    let in_place = match unicorn
        .get_data()
        .crt_heap
        .prepare_process_heap_in_place_reallocation(pointer, size)
    {
        Ok(replacement) => replacement,
        Err(CrtHeapError::ForeignOrFreedPointer | CrtHeapError::AllocatorMismatch) => {
            return fail_process_heap(unicorn, "HeapReAlloc rejected foreign allocation".into());
        }
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    };
    if let Some(replacement) = in_place {
        if flags & HEAP_ZERO_MEMORY != 0 && replacement.requested_size > old.requested_size {
            let extra = (replacement.requested_size - old.requested_size) as usize;
            if let Err(error) = unicorn.mem_write(pointer + old.requested_size, &vec![0; extra]) {
                return fail_process_heap(
                    unicorn,
                    format!("HeapReAlloc zero-extend failed: {error}"),
                );
            }
        }
        if let Err(error) = unicorn
            .get_data_mut()
            .crt_heap
            .commit_process_heap_reallocation(pointer, pointer, replacement)
        {
            return fail_process_heap(unicorn, format!("HeapReAlloc commit failed: {error}"));
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
        return;
    }
    if flags & HEAP_REALLOC_IN_PLACE_ONLY != 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }

    let replacement = match unicorn
        .get_data()
        .crt_heap
        .prepare_process_heap_reallocation(pointer, size)
    {
        Ok(allocation) => allocation,
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    };
    let new_pointer =
        match unicorn
            .get_data()
            .crt_heap
            .first_fit(CRT_HEAP_BASE, CRT_HEAP_END, replacement)
        {
            Ok(pointer) => pointer,
            Err(_) => {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                return;
            }
        };
    if unicorn
        .mem_map(
            new_pointer,
            replacement.backing_size,
            Prot::READ | Prot::WRITE,
        )
        .is_err()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    let copy_length = old.requested_size.min(replacement.requested_size) as usize;
    let move_result = (|| -> Result<(), String> {
        if copy_length > 0 {
            let bytes = unicorn
                .mem_read_as_vec(pointer, copy_length)
                .map_err(|error| format!("HeapReAlloc read old block failed: {error}"))?;
            unicorn
                .mem_write(new_pointer, &bytes)
                .map_err(|error| format!("HeapReAlloc write new block failed: {error}"))?;
        }
        if flags & HEAP_ZERO_MEMORY != 0 && replacement.requested_size > old.requested_size {
            let extra = (replacement.requested_size - old.requested_size) as usize;
            unicorn
                .mem_write(new_pointer + old.requested_size, &vec![0; extra])
                .map_err(|error| format!("HeapReAlloc zero-extend failed: {error}"))?;
        }
        Ok(())
    })();
    if let Err(error) = move_result {
        let _ = unicorn.mem_unmap(new_pointer, replacement.backing_size);
        return fail_process_heap(unicorn, error);
    }
    let removed = match unicorn
        .get_data_mut()
        .crt_heap
        .commit_process_heap_reallocation(pointer, new_pointer, replacement)
    {
        Ok(allocation) => allocation,
        Err(error) => {
            let _ = unicorn.mem_unmap(new_pointer, replacement.backing_size);
            return fail_process_heap(unicorn, format!("HeapReAlloc commit failed: {error}"));
        }
    };
    if let Err(error) = unicorn.mem_unmap(pointer, removed.backing_size) {
        return fail_process_heap(
            unicorn,
            format!("HeapReAlloc unmap old block failed: {error}"),
        );
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, new_pointer);
}

fn emulate_query_performance_counter(unicorn: &mut Unicorn<'_, GuestState>) {
    emulate_query_performance_value(unicorn, "QueryPerformanceCounter", 1);
}

fn emulate_query_performance_frequency(unicorn: &mut Unicorn<'_, GuestState>) {
    // This fixed 10 MHz clock domain matches the 100 ns unit used by Windows
    // FILETIME while remaining independent of host time and hardware timers.
    emulate_query_performance_value(unicorn, "QueryPerformanceFrequency", 10_000_000);
}

fn emulate_query_performance_value(
    unicorn: &mut Unicorn<'_, GuestState>,
    function: &str,
    value: u64,
) {
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err(format!("{function} output pointer is null"));
        }
        unicorn
            .mem_write(output, &value.to_le_bytes())
            .map_err(|error| format!("{function} output {output:#x} is not writable: {error}"))
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

fn set_fls_error(unicorn: &mut Unicorn<'_, GuestState>, error: String, returned: u64) {
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(error);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, returned);
}

fn emulate_tls(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    const TLS_OUT_OF_INDEXES: u64 = u32::MAX as u64;
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    let returned = match operation {
        LegacyWin64Import::TlsAlloc => {
            if let Some(index) = (0..MAX_WINDOWS_TLS_SLOTS)
                .find(|index| !unicorn.get_data().windows_tls_slots.contains_key(index))
            {
                unicorn.get_data_mut().windows_tls_slots.insert(index, 0);
                u64::from(index)
            } else {
                unicorn.get_data_mut().windows_last_error = ERROR_NOT_ENOUGH_MEMORY;
                TLS_OUT_OF_INDEXES
            }
        }
        LegacyWin64Import::TlsGetValue => {
            let index = read_win64_import_argument(unicorn, 0).unwrap_or(u64::MAX) as u32;
            if let Some(value) = unicorn.get_data().windows_tls_slots.get(&index).copied() {
                unicorn.get_data_mut().windows_last_error = ERROR_SUCCESS;
                value
            } else {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                0
            }
        }
        LegacyWin64Import::TlsSetValue => {
            let index = read_win64_import_argument(unicorn, 0).unwrap_or(u64::MAX) as u32;
            let value = read_win64_import_argument(unicorn, 1).unwrap_or_default();
            if let Some(slot) = unicorn.get_data_mut().windows_tls_slots.get_mut(&index) {
                *slot = value;
                1
            } else {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                0
            }
        }
        LegacyWin64Import::TlsFree => {
            let index = read_win64_import_argument(unicorn, 0).unwrap_or(u64::MAX) as u32;
            if unicorn
                .get_data_mut()
                .windows_tls_slots
                .remove(&index)
                .is_some()
            {
                1
            } else {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                0
            }
        }
        _ => 0,
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned);
}

fn fls_index_argument(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u32, String> {
    Ok(read_win64_import_argument(unicorn, 0)? as u32)
}

fn emulate_fls(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        match operation {
            LegacyWin64Import::FlsAlloc => {
                let callback = read_win64_import_argument(unicorn, 0)?;
                if callback != 0 && !image_executable_address(unicorn.get_data(), callback) {
                    return Err(format!(
                        "FlsAlloc callback {callback:#x} is outside the executable image"
                    ));
                }
                let index = (0..MAX_WINDOWS_FLS_SLOTS)
                    .find(|index| !unicorn.get_data().windows_fls_slots.contains_key(index))
                    .ok_or_else(|| format!("FLS slot count exceeds {MAX_WINDOWS_FLS_SLOTS}"))?;
                unicorn
                    .get_data_mut()
                    .windows_fls_slots
                    .insert(index, WindowsFlsSlot { callback, value: 0 });
                Ok(u64::from(index))
            }
            LegacyWin64Import::FlsGetValue => {
                let index = fls_index_argument(unicorn)?;
                unicorn
                    .get_data()
                    .windows_fls_slots
                    .get(&index)
                    .map(|slot| slot.value)
                    .ok_or_else(|| format!("FlsGetValue index {index} is not allocated"))
            }
            LegacyWin64Import::FlsSetValue => {
                let index = fls_index_argument(unicorn)?;
                let value = read_win64_import_argument(unicorn, 1)?;
                let slot = unicorn
                    .get_data_mut()
                    .windows_fls_slots
                    .get_mut(&index)
                    .ok_or_else(|| format!("FlsSetValue index {index} is not allocated"))?;
                slot.value = value;
                Ok(1)
            }
            _ => Err("invalid FLS operation".into()),
        }
    })();
    match result {
        Ok(returned) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, returned);
        }
        Err(error) => {
            let returned = if operation == LegacyWin64Import::FlsAlloc {
                u64::from(u32::MAX)
            } else {
                0
            };
            set_fls_error(unicorn, error, returned);
        }
    }
}

fn finish_fls_free(
    unicorn: &mut Unicorn<'_, GuestState>,
    pending: PendingFlsFree,
) -> Result<(), String> {
    if unicorn
        .get_data_mut()
        .windows_fls_slots
        .remove(&pending.index)
        .is_none()
    {
        return Err(format!(
            "FlsFree index {} disappeared during callback",
            pending.index
        ));
    }
    unicorn
        .reg_write(RegisterX86::RSP, pending.continuation_rsp)
        .map_err(|error| format!("FlsFree final stack write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::R11, pending.return_address)
        .map_err(|error| format!("FlsFree return target write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RAX, 1)
        .map_err(|error| format!("FlsFree return value write failed: {error}"))
}

fn fail_fls_free(unicorn: &mut Unicorn<'_, GuestState>, error: String) {
    unicorn.get_data_mut().pending_fls_free = None;
    set_fls_error(unicorn, error, 0);
    let _ = unicorn.emu_stop();
}

fn emulate_fls_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        if unicorn.get_data().pending_fls_free.is_some() {
            return Err("nested FlsFree callback is unsupported".into());
        }
        let index = fls_index_argument(unicorn)?;
        let slot = unicorn
            .get_data()
            .windows_fls_slots
            .get(&index)
            .copied()
            .ok_or_else(|| format!("FlsFree index {index} is not allocated"))?;
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("FlsFree stack read failed: {error}"))?;
        let return_address = read_vcomp_u64(unicorn, rsp)
            .map_err(|error| format!("FlsFree return address read failed: {error}"))?;
        if return_address != RETURN_ADDRESS
            && !image_executable_address(unicorn.get_data(), return_address)
        {
            return Err(format!(
                "FlsFree caller return {return_address:#x} is outside the executable image"
            ));
        }
        let continuation_rsp = rsp
            .checked_add(8)
            .ok_or_else(|| "FlsFree continuation stack overflow".to_string())?;
        let pending = PendingFlsFree {
            index,
            return_address,
            continuation_rsp,
        };
        if slot.callback == 0 || slot.value == 0 {
            return finish_fls_free(unicorn, pending);
        }
        unicorn
            .mem_write(rsp, &HOST_FLS_FREE_CONTINUE.to_le_bytes())
            .map_err(|error| format!("FlsFree callback continuation write failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::RCX, slot.value)
            .map_err(|error| format!("FlsFree callback argument write failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::R11, slot.callback)
            .map_err(|error| format!("FlsFree callback target write failed: {error}"))?;
        unicorn.get_data_mut().pending_fls_free = Some(pending);
        Ok(())
    })();
    if let Err(error) = result {
        fail_fls_free(unicorn, error);
    }
}

fn continue_fls_free(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| -> Result<(), String> {
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("FlsFree callback stack read failed: {error}"))?;
        let pending = unicorn
            .get_data_mut()
            .pending_fls_free
            .take()
            .ok_or_else(|| "FlsFree continuation has no pending callback".to_string())?;
        if rsp != pending.continuation_rsp {
            return Err(format!(
                "FlsFree callback stack {rsp:#x} does not match {:#x}",
                pending.continuation_rsp
            ));
        }
        finish_fls_free(unicorn, pending)
    })();
    if let Err(error) = result {
        fail_fls_free(unicorn, error);
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
