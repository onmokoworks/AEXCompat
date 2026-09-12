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

// The guest CRT remains in its initial C locale; locale mutation is unsupported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CrtAsciiClass {
    Alpha,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Upper,
    Xdigit,
}

impl CrtAsciiClass {
    fn contains(self, byte: u8) -> bool {
        match self {
            Self::Alpha => byte.is_ascii_alphabetic(),
            Self::Digit => byte.is_ascii_digit(),
            Self::Graph => byte.is_ascii_graphic(),
            Self::Lower => byte.is_ascii_lowercase(),
            Self::Print => byte.is_ascii_graphic() || byte == b' ',
            Self::Punct => byte.is_ascii_punctuation(),
            Self::Upper => byte.is_ascii_uppercase(),
            Self::Xdigit => byte.is_ascii_hexdigit(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyWin64Import {
    Malloc,
    Calloc,
    Free,
    CrtStrdup,
    CrtStricmp,
    CrtIsSpace,
    CrtIsAlnum,
    CrtAsciiClass(CrtAsciiClass),
    CrtToLower,
    CrtToUpper,
    AlignedMalloc,
    AlignedFree,
    CallNewHandler,
    Strncpy,
    Memset,
    MemoryCopy,
    MemChr,
    StrStr,
    CrtTime64,
    StrCmp,
    StrNCmp,
    StrLen,
    StrCpy,
    SetNamedSecurityInfoA,
    SetEntriesInAclA,
    LocalFree,
    AllocateAndInitializeSid,
    FreeSid,
    RegOpenKeyExA,
    RegCloseKey,
    MemCmp,
    StdioVsnprintfS,
    StdioVsscanf,
    StdioVsprintf,
    AcRtIobFunc,
    Fgetc,
    Fread,
    Fclose,
    Fopen,
    FopenS,
    StrncpyS,
    MsvcpLockitCtor,
    MsvcpLockitDtor,
    MsvcpMutexInit,
    MsvcpMutexLock,
    MsvcpMutexUnlock,
    MsvcpMutexDestroy,
    MsvcpHardwareConcurrency,
    ShGetFolderPathA,
    MsvcpExceptionPtrCreate,
    MsvcpExceptionPtrCopy,
    MsvcpExceptionPtrAssign,
    MsvcpExceptionPtrDestroy,
    MsvcpExceptionPtrCurrentException,
    MsvcpExceptionPtrRethrow,
    VcruntimeExceptionCopy,
    VcruntimeExceptionDestroy,
    CxxThrowException,
    CopySign,
    Cos,
    CosF,
    Ceil,
    CeilF,
    ExpF,
    Floor,
    FloorF,
    FmodF,
    LRound,
    LRoundF,
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
    CreateFileW,
    FindFirstFileA,
    FindNextFileA,
    FindClose,
    FindFirstFileExW,
    CreateThread,
    NtWriteFile,
    WakeByAddressAll,
    WakeByAddressSingle,
    WaitOnAddress,
    CreateSemaphoreA,
    ReleaseSemaphore,
    CreateMutexA,
    ReleaseMutex,
    WaitForSingleObject,
    WaitForSingleObjectEx,
    CloseHandle,
    GetCurrentThread,
    SetThreadStackGuarantee,
    SwitchToThread,
    ResumeThread,
    GetCommandLineA,
    GetCommandLineW,
    GetACP,
    GetCPInfo,
    IsDebuggerPresent,
    OutputDebugStringA,
    GetCurrentThreadId,
    GetCurrentProcessId,
    QueryPerformanceCounter,
    QueryPerformanceFrequency,
    GetEnvironmentVariableA,
    GetEnvironmentVariableW,
    GetEnvironmentStringsW,
    FreeEnvironmentStringsW,
    WideCharToMultiByte,
    MultiByteToWideChar,
    GetStringTypeW,
    LCMapStringW,
    GetLastError,
    SetLastError,
    SetThreadErrorMode,
    LoadLibraryExA,
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
    CrtPutenv,
    CrtGetenv,
    InitializeCriticalSection,
    InitializeCriticalSectionAndSpinCount,
    InitializeCriticalSectionEx,
    EnterCriticalSection,
    LeaveCriticalSection,
    DeleteCriticalSection,
    SetSecurityDescriptorDacl,
    InitializeSecurityDescriptor,
    InitializeSrwLock,
    AcquireSrwLockExclusive,
    TryAcquireSrwLockExclusive,
    ReleaseSrwLockExclusive,
    GetModuleHandleW,
    GetModuleHandleExA,
    GetModuleHandleExW,
    GetModuleFileNameW,
    LoadLibraryW,
    GetProcAddress,
    InitializeSListHead,
    DisableThreadLibraryCalls,
    ProcessPrng,
    GetProcessHeap,
    HeapAlloc,
    HeapFree,
    HeapReAlloc,
    HeapCreate,
    HeapDestroy,
    WsaStartup,
    WsaCleanup,
    RtlPcToFileHeader,
    RaiseException,
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
        ("msvcp140.dll", "_Thrd_id") => LegacyWin64Import::GetCurrentThreadId,
        (_, "_Thrd_id") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("msvcp140.dll", "??0_Lockit@std@@QEAA@H@Z") => LegacyWin64Import::MsvcpLockitCtor,
        ("msvcp140.dll", "??1_Lockit@std@@QEAA@XZ") => LegacyWin64Import::MsvcpLockitDtor,
        ("advapi32.dll", "SetSecurityDescriptorDacl") => {
            LegacyWin64Import::SetSecurityDescriptorDacl
        }
        (_, "SetSecurityDescriptorDacl") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("advapi32.dll", "InitializeSecurityDescriptor") => {
            LegacyWin64Import::InitializeSecurityDescriptor
        }
        (_, "InitializeSecurityDescriptor") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-sysinfo-l1-1-0.dll", "GetSystemTimeAsFileTime") => {
            LegacyWin64Import::GetSystemTimeAsFileTime
        }
        ("kernel32.dll", "GetSystemInfo") => LegacyWin64Import::GetSystemInfo,
        (_, "GetSystemInfo") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "GetStartupInfoW") => {
            LegacyWin64Import::GetStartupInfoW
        }
        (_, "GetStartupInfoW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-rtlsupport-l1-1-0.dll", "RtlCaptureContext") => {
            LegacyWin64Import::RtlCaptureContext
        }
        (_, "RtlCaptureContext") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll", "GetStdHandle") => {
            LegacyWin64Import::GetStdHandle
        }
        (_, "GetStdHandle") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-console-l1-1-0.dll", "GetConsoleMode") => {
            LegacyWin64Import::GetConsoleMode
        }
        (_, "GetConsoleMode") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-file-l1-1-0.dll", "GetFileType") => {
            LegacyWin64Import::GetFileType
        }
        (_, "GetFileType") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-file-l1-1-0.dll", "CreateFileW") => {
            LegacyWin64Import::CreateFileW
        }
        (_, "CreateFileW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "FindFirstFileExW") => LegacyWin64Import::FindFirstFileExW,
        ("kernel32.dll", "FindFirstFileA") => LegacyWin64Import::FindFirstFileA,
        ("kernel32.dll", "FindNextFileA") => LegacyWin64Import::FindNextFileA,
        ("kernel32.dll", "FindClose") => LegacyWin64Import::FindClose,
        (_, "FindNextFileA" | "FindClose") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (_, "FindFirstFileA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (_, "FindFirstFileExW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "CreateThread") => LegacyWin64Import::CreateThread,
        (_, "CreateThread") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("ntdll.dll", "NtWriteFile") => LegacyWin64Import::NtWriteFile,
        (_, "NtWriteFile") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-synch-l1-2-0.dll", "WakeByAddressAll") => {
            LegacyWin64Import::WakeByAddressAll
        }
        (_, "WakeByAddressAll") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-synch-l1-2-0.dll", "WakeByAddressSingle") => {
            LegacyWin64Import::WakeByAddressSingle
        }
        (_, "WakeByAddressSingle") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-synch-l1-2-0.dll", "WaitOnAddress") => {
            LegacyWin64Import::WaitOnAddress
        }
        (_, "WaitOnAddress") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "CreateSemaphoreA") => LegacyWin64Import::CreateSemaphoreA,
        ("kernel32.dll", "ReleaseSemaphore") => LegacyWin64Import::ReleaseSemaphore,
        (_, "CreateSemaphoreA" | "ReleaseSemaphore") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("kernel32.dll", "CreateMutexA") => LegacyWin64Import::CreateMutexA,
        ("kernel32.dll", "ReleaseMutex") => LegacyWin64Import::ReleaseMutex,
        (_, "CreateMutexA" | "ReleaseMutex") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("kernel32.dll", "WaitForSingleObject") => LegacyWin64Import::WaitForSingleObject,
        ("kernel32.dll", "WaitForSingleObjectEx") => LegacyWin64Import::WaitForSingleObjectEx,
        ("kernel32.dll" | "api-ms-win-core-handle-l1-1-0.dll", "CloseHandle") => {
            LegacyWin64Import::CloseHandle
        }
        ("kernel32.dll", "GetCurrentThread") => LegacyWin64Import::GetCurrentThread,
        ("kernel32.dll", "SetThreadStackGuarantee") => LegacyWin64Import::SetThreadStackGuarantee,
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "SwitchToThread") => {
            LegacyWin64Import::SwitchToThread
        }
        ("kernel32.dll", "ResumeThread") => LegacyWin64Import::ResumeThread,
        (
            _,
            "WaitForSingleObject"
            | "WaitForSingleObjectEx"
            | "CloseHandle"
            | "GetCurrentThread"
            | "SetThreadStackGuarantee"
            | "SwitchToThread"
            | "ResumeThread",
        ) => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll", "GetCommandLineA") => {
            LegacyWin64Import::GetCommandLineA
        }
        (_, "GetCommandLineA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll", "GetCommandLineW") => {
            LegacyWin64Import::GetCommandLineW
        }
        (_, "GetCommandLineW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-localization-l1-2-0.dll", "GetACP") => {
            LegacyWin64Import::GetACP
        }
        (_, "GetACP") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-localization-l1-2-0.dll", "GetCPInfo") => {
            LegacyWin64Import::GetCPInfo
        }
        (_, "GetCPInfo") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-debug-l1-1-0.dll", "IsDebuggerPresent") => {
            LegacyWin64Import::IsDebuggerPresent
        }
        (_, "IsDebuggerPresent") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "OutputDebugStringA") => LegacyWin64Import::OutputDebugStringA,
        (_, "OutputDebugStringA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "GetCurrentThreadId") => {
            LegacyWin64Import::GetCurrentThreadId
        }
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "GetCurrentProcessId") => {
            LegacyWin64Import::GetCurrentProcessId
        }
        ("kernel32.dll" | "api-ms-win-core-profile-l1-1-0.dll", "QueryPerformanceCounter") => {
            LegacyWin64Import::QueryPerformanceCounter
        }
        ("kernel32.dll", "QueryPerformanceFrequency") => {
            LegacyWin64Import::QueryPerformanceFrequency
        }
        (
            "kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll",
            "GetEnvironmentVariableA",
        ) => LegacyWin64Import::GetEnvironmentVariableA,
        ("kernel32.dll", "GetEnvironmentVariableW") => LegacyWin64Import::GetEnvironmentVariableW,
        (_, "GetEnvironmentVariableW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (
            "kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll",
            "GetEnvironmentStringsW",
        ) => LegacyWin64Import::GetEnvironmentStringsW,
        (_, "GetEnvironmentStringsW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (
            "kernel32.dll" | "api-ms-win-core-processenvironment-l1-1-0.dll",
            "FreeEnvironmentStringsW",
        ) => LegacyWin64Import::FreeEnvironmentStringsW,
        (_, "FreeEnvironmentStringsW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-string-l1-1-0.dll", "WideCharToMultiByte") => {
            LegacyWin64Import::WideCharToMultiByte
        }
        (_, "WideCharToMultiByte") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-string-l1-1-0.dll", "MultiByteToWideChar") => {
            LegacyWin64Import::MultiByteToWideChar
        }
        (_, "MultiByteToWideChar") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-string-l1-1-0.dll", "GetStringTypeW") => {
            LegacyWin64Import::GetStringTypeW
        }
        (_, "GetStringTypeW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-localization-l1-2-0.dll", "LCMapStringW") => {
            LegacyWin64Import::LCMapStringW
        }
        (_, "LCMapStringW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-errorhandling-l1-1-0.dll", "GetLastError") => {
            LegacyWin64Import::GetLastError
        }
        ("kernel32.dll" | "api-ms-win-core-errorhandling-l1-1-0.dll", "SetLastError") => {
            LegacyWin64Import::SetLastError
        }
        ("kernel32.dll", "SetThreadErrorMode") => LegacyWin64Import::SetThreadErrorMode,
        (_, "SetThreadErrorMode") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "LoadLibraryA") => LegacyWin64Import::LoadLibraryA,
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "LoadLibraryExA") => {
            LegacyWin64Import::LoadLibraryExA
        }
        (_, "LoadLibraryExA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (_, "LoadLibraryA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "LoadLibraryExW") => {
            LegacyWin64Import::LoadLibraryExW
        }
        (_, "LoadLibraryExW") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll", "FlsAlloc") => LegacyWin64Import::FlsAlloc,
        ("kernel32.dll", "FlsGetValue") => LegacyWin64Import::FlsGetValue,
        ("kernel32.dll", "FlsSetValue") => LegacyWin64Import::FlsSetValue,
        ("kernel32.dll", "FlsFree") => LegacyWin64Import::FlsFree,
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "TlsAlloc") => {
            LegacyWin64Import::TlsAlloc
        }
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "TlsGetValue") => {
            LegacyWin64Import::TlsGetValue
        }
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "TlsSetValue") => {
            LegacyWin64Import::TlsSetValue
        }
        ("kernel32.dll" | "api-ms-win-core-processthreads-l1-1-0.dll", "TlsFree") => {
            LegacyWin64Import::TlsFree
        }
        (_, "TlsAlloc" | "TlsGetValue" | "TlsSetValue" | "TlsFree") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("kernel32.dll" | "api-ms-win-core-interlocked-l1-1-0.dll", "InitializeSListHead") => {
            LegacyWin64Import::InitializeSListHead
        }
        ("kernel32.dll", "DisableThreadLibraryCalls") => {
            LegacyWin64Import::DisableThreadLibraryCalls
        }
        ("bcryptprimitives.dll", "ProcessPrng") => LegacyWin64Import::ProcessPrng,
        (_, "ProcessPrng") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "GetProcessHeap") => {
            LegacyWin64Import::GetProcessHeap
        }
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "HeapAlloc") => {
            LegacyWin64Import::HeapAlloc
        }
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "HeapFree") => {
            LegacyWin64Import::HeapFree
        }
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "HeapReAlloc") => {
            LegacyWin64Import::HeapReAlloc
        }
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "HeapCreate") => {
            LegacyWin64Import::HeapCreate
        }
        ("kernel32.dll" | "api-ms-win-core-heap-l1-1-0.dll", "HeapDestroy") => {
            LegacyWin64Import::HeapDestroy
        }
        (
            _,
            "GetProcessHeap" | "HeapAlloc" | "HeapFree" | "HeapReAlloc" | "HeapCreate"
            | "HeapDestroy",
        ) => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("ws2_32.dll" | "wsock32.dll", "WSAStartup" | "ORDINAL 115") => {
            LegacyWin64Import::WsaStartup
        }
        ("ws2_32.dll" | "wsock32.dll", "WSACleanup" | "ORDINAL 116") => {
            LegacyWin64Import::WsaCleanup
        }
        (_, "WSAStartup" | "ORDINAL 115" | "WSACleanup" | "ORDINAL 116") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("kernel32.dll", "RtlPcToFileHeader") => LegacyWin64Import::RtlPcToFileHeader,
        (_, "RtlPcToFileHeader") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-errorhandling-l1-1-0.dll", "RaiseException") => {
            LegacyWin64Import::RaiseException
        }
        (_, "RaiseException") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "InitializeCriticalSection") => {
            LegacyWin64Import::InitializeCriticalSection
        }
        (
            "kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll",
            "InitializeCriticalSectionAndSpinCount",
        ) => LegacyWin64Import::InitializeCriticalSectionAndSpinCount,
        ("kernel32.dll", "InitializeCriticalSectionEx") => {
            LegacyWin64Import::InitializeCriticalSectionEx
        }
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "EnterCriticalSection") => {
            LegacyWin64Import::EnterCriticalSection
        }
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "LeaveCriticalSection") => {
            LegacyWin64Import::LeaveCriticalSection
        }
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "DeleteCriticalSection") => {
            LegacyWin64Import::DeleteCriticalSection
        }
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "InitializeSRWLock") => {
            LegacyWin64Import::InitializeSrwLock
        }
        (_, "InitializeSRWLock") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "AcquireSRWLockExclusive") => {
            LegacyWin64Import::AcquireSrwLockExclusive
        }
        ("kernel32.dll", "TryAcquireSRWLockExclusive") => {
            LegacyWin64Import::TryAcquireSrwLockExclusive
        }
        ("kernel32.dll" | "api-ms-win-core-synch-l1-1-0.dll", "ReleaseSRWLockExclusive") => {
            LegacyWin64Import::ReleaseSrwLockExclusive
        }
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "GetModuleHandleW") => {
            LegacyWin64Import::GetModuleHandleW
        }
        ("kernel32.dll", "GetModuleHandleExA") => LegacyWin64Import::GetModuleHandleExA,
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "GetModuleHandleExW") => {
            LegacyWin64Import::GetModuleHandleExW
        }
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "GetModuleFileNameW") => {
            LegacyWin64Import::GetModuleFileNameW
        }
        ("kernel32.dll", "LoadLibraryW") => LegacyWin64Import::LoadLibraryW,
        ("kernel32.dll" | "api-ms-win-core-libraryloader-l1-2-0.dll", "GetProcAddress") => {
            LegacyWin64Import::GetProcAddress
        }
        (
            _,
            "InitializeCriticalSection"
            | "InitializeCriticalSectionAndSpinCount"
            | "InitializeCriticalSectionEx"
            | "EnterCriticalSection"
            | "AcquireSRWLockExclusive"
            | "TryAcquireSRWLockExclusive"
            | "ReleaseSRWLockExclusive"
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
        ("api-ms-win-crt-environment-l1-1-0.dll", "_putenv") => LegacyWin64Import::CrtPutenv,
        ("api-ms-win-crt-environment-l1-1-0.dll", "getenv") => LegacyWin64Import::CrtGetenv,
        (_, "getenv") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "_copysign" | "copysign") => {
            LegacyWin64Import::CopySign
        }
        (_, "_copysign" | "copysign") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll", "cos") => LegacyWin64Import::Cos,
        (_, "cos") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "ceil") => LegacyWin64Import::Ceil,
        (_, "ceil") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "ceilf") => LegacyWin64Import::CeilF,
        (_, "ceilf") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "floor") => LegacyWin64Import::Floor,
        (_, "floor") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll", "fmodf") => LegacyWin64Import::FmodF,
        (_, "fmodf") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "lround") => LegacyWin64Import::LRound,
        (_, "lround") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-math-l1-1-0.dll" | "ucrtbase.dll", "lroundf") => {
            LegacyWin64Import::LRoundF
        }
        (_, "lroundf") => return Win64ImportDispatch::UnsupportedLegacyImport,
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
        ("api-ms-win-crt-time-l1-1-0.dll" | "ucrtbase.dll", "_time64") => {
            LegacyWin64Import::CrtTime64
        }
        (_, "_time64") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "strcmp") => {
            LegacyWin64Import::StrCmp
        }
        (_, "strcmp") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "strncmp") => {
            LegacyWin64Import::StrNCmp
        }
        (_, "strncmp") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "strlen") => {
            LegacyWin64Import::StrLen
        }
        (_, "strlen") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "strcpy") => {
            LegacyWin64Import::StrCpy
        }
        (_, "strcpy") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("advapi32.dll", "SetNamedSecurityInfoA") => LegacyWin64Import::SetNamedSecurityInfoA,
        (_, "SetNamedSecurityInfoA") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("advapi32.dll", "SetEntriesInAclA") => LegacyWin64Import::SetEntriesInAclA,
        ("kernel32.dll", "LocalFree") => LegacyWin64Import::LocalFree,
        (_, "SetEntriesInAclA" | "LocalFree") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("advapi32.dll", "AllocateAndInitializeSid") => LegacyWin64Import::AllocateAndInitializeSid,
        ("advapi32.dll", "FreeSid") => LegacyWin64Import::FreeSid,
        (_, "AllocateAndInitializeSid" | "FreeSid") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("advapi32.dll", "RegOpenKeyExA") => LegacyWin64Import::RegOpenKeyExA,
        ("advapi32.dll", "RegCloseKey") => LegacyWin64Import::RegCloseKey,
        (_, "RegOpenKeyExA" | "RegCloseKey") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcruntime140.dll", "strstr") => LegacyWin64Import::StrStr,
        (_, "strstr") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("vcruntime140.dll", "memchr") => LegacyWin64Import::MemChr,
        (_, "memchr") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("vcruntime140.dll", "memcmp") => LegacyWin64Import::MemCmp,
        (_, "memcmp") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "__stdio_common_vsnprintf_s") => {
            LegacyWin64Import::StdioVsnprintfS
        }
        (_, "__stdio_common_vsnprintf_s") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "__stdio_common_vsprintf") => {
            LegacyWin64Import::StdioVsprintf
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "__stdio_common_vsscanf") => {
            LegacyWin64Import::StdioVsscanf
        }
        (_, "__stdio_common_vsscanf") => return Win64ImportDispatch::UnsupportedLegacyImport,
        (_, "__stdio_common_vsprintf") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "__acrt_iob_func") => {
            LegacyWin64Import::AcRtIobFunc
        }
        (_, "__acrt_iob_func") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "fread") => LegacyWin64Import::Fread,
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "fclose") => LegacyWin64Import::Fclose,
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "getc" | "fgetc") => {
            LegacyWin64Import::Fgetc
        }
        (_, "getc" | "fgetc" | "fread" | "fclose") => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "fopen") => LegacyWin64Import::Fopen,
        (_, "fopen") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-stdio-l1-1-0.dll" | "ucrtbase.dll", "fopen_s") => {
            LegacyWin64Import::FopenS
        }
        (_, "fopen_s") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "strncpy_s") => {
            LegacyWin64Import::StrncpyS
        }
        (_, "strncpy_s") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "_strdup") => {
            LegacyWin64Import::CrtStrdup
        }
        (_, "_strdup") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "_stricmp") => {
            LegacyWin64Import::CrtStricmp
        }
        (_, "_stricmp") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "tolower") => {
            LegacyWin64Import::CrtToLower
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isspace") => {
            LegacyWin64Import::CrtIsSpace
        }
        (_, "isspace") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isalnum") => {
            LegacyWin64Import::CrtIsAlnum
        }
        (_, "isalnum") => return Win64ImportDispatch::UnsupportedLegacyImport,
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isalpha") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Alpha)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isdigit") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Digit)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isgraph") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Graph)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "islower") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Lower)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isprint") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Print)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "ispunct") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Punct)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isupper") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Upper)
        }
        ("api-ms-win-crt-string-l1-1-0.dll" | "ucrtbase.dll", "isxdigit") => {
            LegacyWin64Import::CrtAsciiClass(CrtAsciiClass::Xdigit)
        }
        (
            _,
            "isalpha" | "isdigit" | "isgraph" | "islower" | "isprint" | "ispunct" | "isupper"
            | "isxdigit",
        ) => return Win64ImportDispatch::UnsupportedLegacyImport,

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
        ("shell32.dll", "SHGetFolderPathA") => LegacyWin64Import::ShGetFolderPathA,
        (_, "SHGetFolderPathA") => return Win64ImportDispatch::UnsupportedLegacyImport,
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
        Win64ImportDispatch::LegacyImplemented(implementation) => {
            match implementation {
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
                LegacyWin64Import::CrtStricmp => {
                    uc("write _stricmp return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install _stricmp import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_stricmp(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::CrtIsSpace => {
                    uc("write isspace return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install isspace import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            let result = (|| -> Result<u64, String> {
                                let input = read_win64_import_argument(unicorn, 0)? as u32 as i32;
                                if !(-1..=255).contains(&input) {
                                    return Err(
                                        "isspace input is neither unsigned char nor EOF".into()
                                    );
                                }
                                // CRT starts in the C locale; locale mutation is unsupported.
                                Ok(if matches!(input, 9..=13 | 32) { 8 } else { 0 })
                            })();
                            finish_guest_stdio(unicorn, result);
                        }),
                    )?;
                }
                LegacyWin64Import::CrtIsAlnum => {
                    uc("write isalnum return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install isalnum import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            let result = (|| -> Result<u64, String> {
                                let input = read_win64_import_argument(unicorn, 0)? as u32 as i32;
                                if !(-1..=255).contains(&input) {
                                    return Err(
                                        "isalnum input is neither unsigned char nor EOF".into()
                                    );
                                }
                                // CRT starts in the C locale; locale mutation is unsupported.
                                Ok(u64::from(
                                    input >= 0 && (input as u8).is_ascii_alphanumeric(),
                                ))
                            })();
                            finish_guest_stdio(unicorn, result);
                        }),
                    )?;
                }
                LegacyWin64Import::CrtAsciiClass(class) => {
                    uc(
                        "write CRT character classification return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install CRT character classification import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            let result = (|| -> Result<u64, String> {
                                let input = read_win64_import_argument(unicorn, 0)? as u32 as i32;
                                if !(-1..=255).contains(&input) {
                                    return Err(format!(
                                        "CRT {class:?} classification input is neither unsigned char nor EOF"
                                    ));
                                }
                                Ok(u64::from(input >= 0 && class.contains(input as u8)))
                            })();
                            finish_guest_stdio(unicorn, result);
                        }),
                    )?;
                }
                LegacyWin64Import::CrtToLower => {
                    uc("write tolower return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install tolower import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            let input =
                                unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX) as u32;
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
                            let input =
                                unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX) as u32;
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
                LegacyWin64Import::SetNamedSecurityInfoA
                | LegacyWin64Import::SetEntriesInAclA
                | LegacyWin64Import::LocalFree => {
                    uc("write ACL return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install ACL import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_windows_acl(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::AllocateAndInitializeSid | LegacyWin64Import::FreeSid => {
                    uc("write SID return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install SID import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_windows_sid(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::RegOpenKeyExA => {
                    uc(
                        "write RegOpenKeyExA return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install RegOpenKeyExA",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_reg_open_key_ex_a(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::RegCloseKey => {
                    uc("write RegCloseKey return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install RegCloseKey",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_reg_close_key(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrCpy => {
                    uc("write strcpy return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strcpy import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_strcpy(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::CrtTime64 => {
                    uc("write _time64 return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install _time64",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_time64(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrCmp => {
                    uc("write strcmp return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strcmp",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_strcmp(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrNCmp => {
                    uc("write strncmp return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strncmp",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_strncmp(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrLen => {
                    uc("write strlen return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strlen import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_strlen(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrStr => {
                    uc("write strstr return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strstr import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_strstr(unicorn);
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
                LegacyWin64Import::StdioVsscanf => {
                    uc("write vsscanf return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install vsscanf",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_stdio_common_vsscanf(unicorn);
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
                operation @ (LegacyWin64Import::MsvcpLockitCtor
                | LegacyWin64Import::MsvcpLockitDtor) => {
                    let destroy = operation == LegacyWin64Import::MsvcpLockitDtor;
                    uc("write Lockit return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install Lockit",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_msvcp_lockit(unicorn, destroy);
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
                LegacyWin64Import::ShGetFolderPathA => {
                    uc(
                        "write SHGetFolderPathA return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install bounded SHGetFolderPathA import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_sh_get_folder_path_a(unicorn);
                        }),
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
                LegacyWin64Import::CopySign => {
                    uc("write copysign return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install copysign",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            let result = (|| -> Result<(), String> {
                                let mut magnitude = unicorn
                                    .reg_read_long(RegisterX86::XMM0)
                                    .map_err(|e| e.to_string())?;
                                let sign = unicorn
                                    .reg_read_long(RegisterX86::XMM1)
                                    .map_err(|e| e.to_string())?;
                                // Copy only the IEEE binary64 sign bit, including NaNs,
                                // signed zeros and subnormals, without FP arithmetic.
                                magnitude[7] = (magnitude[7] & 0x7f) | (sign[7] & 0x80);
                                unicorn
                                    .reg_write_long(RegisterX86::XMM0, &magnitude)
                                    .map_err(|e| e.to_string())
                            })();
                            if let Err(error) = result {
                                if unicorn.get_data().callback_error.is_none() {
                                    unicorn.get_data_mut().callback_error =
                                        Some(format!("copysign registers: {error}"));
                                }
                                let _ = unicorn.emu_stop();
                            }
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
                LegacyWin64Import::FmodF => {
                    install_fmodf_import(unicorn, stub)?;
                }
                LegacyWin64Import::LRound => {
                    install_lround_import(unicorn, stub)?;
                }
                LegacyWin64Import::LRoundF => {
                    install_lroundf_import(unicorn, stub)?;
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
                LegacyWin64Import::CreateFileW => {
                    uc("write CreateFileW return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install bounded CreateFileW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_create_file_w(unicorn);
                        }),
                    )?;
                }
                operation @ (LegacyWin64Import::FindFirstFileA
                | LegacyWin64Import::FindNextFileA
                | LegacyWin64Import::FindClose) => {
                    uc("write file search return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install file search",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_guest_file_search(unicorn, operation);
                        }),
                    )?;
                }
                LegacyWin64Import::FindFirstFileExW => {
                    uc(
                        "write FindFirstFileExW return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install bounded FindFirstFileExW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_find_first_file_ex_w(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::CreateThread => {
                    uc(
                        "write CreateThread callback tail jump",
                        unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                    )?;
                    uc(
                        "install bounded CreateThread import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_create_thread(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::NtWriteFile => {
                    uc("write NtWriteFile return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install bounded NtWriteFile import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_nt_write_file(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::WakeByAddressAll => {
                    // WakeByAddressAll is a VOID function.  A plain RET preserves
                    // RAX while the hook updates only guest-owned waiter state.
                    uc(
                        "write WakeByAddressAll return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install bounded WakeByAddressAll import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_wake_by_address_all(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::WakeByAddressSingle => {
                    uc(
                        "write WakeByAddressSingle return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install bounded WakeByAddressSingle import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_wake_by_address(unicorn, false);
                        }),
                    )?;
                }
                LegacyWin64Import::WaitOnAddress => {
                    uc(
                        "write WaitOnAddress return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install cooperative WaitOnAddress import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_wait_on_address(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::CreateMutexA
                | LegacyWin64Import::ReleaseMutex
                | LegacyWin64Import::CreateSemaphoreA
                | LegacyWin64Import::ReleaseSemaphore => {
                    uc("write mutex return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install mutex",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_windows_kernel_object(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::WaitForSingleObject
                | LegacyWin64Import::WaitForSingleObjectEx
                | LegacyWin64Import::CloseHandle
                | LegacyWin64Import::SetThreadStackGuarantee => {
                    uc(
                        "write Windows thread lifecycle return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install Windows thread lifecycle import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_windows_thread_lifecycle(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::GetCurrentThread => {
                    uc(
                        "install current-thread pseudo handle",
                        unicorn.mem_write(stub, &deterministic_u64_stub(u64::MAX - 1)),
                    )?;
                }
                LegacyWin64Import::SwitchToThread => {
                    uc(
                        "write cooperative SwitchToThread return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install cooperative SwitchToThread",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_switch_to_thread(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::ResumeThread => {
                    uc(
                        "write ResumeThread callback tail jump",
                        unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                    )?;
                    uc(
                        "install bounded ResumeThread import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_resume_thread(unicorn);
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
                LegacyWin64Import::GetCurrentThreadId => {
                    uc(
                        "write GetCurrentThreadId return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install deterministic guest thread identity",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            let id = unicorn.get_data().current_windows_thread_id;
                            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(id));
                        }),
                    )?;
                }
                LegacyWin64Import::GetCurrentProcessId => {
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
                LegacyWin64Import::GetEnvironmentStringsW => {
                    uc(
                        "write GetEnvironmentStringsW return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install GetEnvironmentStringsW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_get_environment_strings_w(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::FreeEnvironmentStringsW => {
                    uc(
                        "write FreeEnvironmentStringsW return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install FreeEnvironmentStringsW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_free_environment_strings_w(unicorn);
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
                LegacyWin64Import::LoadLibraryExA => {
                    uc(
                        "write LoadLibraryExA return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install LoadLibraryExA",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_load_library_ex_a(unicorn);
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
                LegacyWin64Import::CrtPutenv => {
                    uc("write putenv return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install putenv",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_crt_putenv(unicorn);
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
                | LegacyWin64Import::InitializeCriticalSectionEx
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
                LegacyWin64Import::SetSecurityDescriptorDacl => {
                    uc(
                        "write descriptor DACL setter return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install descriptor DACL setter",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_set_security_descriptor_dacl(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::InitializeSecurityDescriptor => {
                    uc(
                        "write security descriptor initializer return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install security descriptor initializer",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_initialize_security_descriptor(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::InitializeSrwLock
                | LegacyWin64Import::AcquireSrwLockExclusive
                | LegacyWin64Import::TryAcquireSrwLockExclusive
                | LegacyWin64Import::ReleaseSrwLockExclusive => {
                    uc("write SRW return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install SRW import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_windows_srw_lock(unicorn, implementation);
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
                LegacyWin64Import::GetModuleHandleExW => {
                    uc(
                        "write GetModuleHandleExW return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install GetModuleHandleExW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_get_module_handle_ex_w(unicorn);
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
                LegacyWin64Import::LoadLibraryW => {
                    uc(
                        "write LoadLibraryW return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install LoadLibraryW import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_load_library_w(unicorn);
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
                            emulate_get_proc_address(unicorn);
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
                LegacyWin64Import::OutputDebugStringA => {
                    uc(
                        "write OutputDebugStringA return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install OutputDebugStringA import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_output_debug_string_a(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::AcRtIobFunc => {
                    uc(
                        "write standard FILE return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install standard FILE import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_acrt_iob_func(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::Fgetc | LegacyWin64Import::Fread | LegacyWin64Import::Fclose => {
                    uc("write stdio return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install stdio",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_guest_stdio(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::Fopen => {
                    uc("write fopen return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install fopen",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_fopen(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::FopenS => {
                    uc("write fopen_s return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install fopen_s import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_fopen_s(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::StrncpyS => {
                    uc("write strncpy_s return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install strncpy_s import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_strncpy_s(unicorn);
                        }),
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
                LegacyWin64Import::HeapCreate | LegacyWin64Import::HeapDestroy => {
                    uc(
                        "write private heap return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install private heap import",
                        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                            emulate_private_heap_lifecycle(unicorn, implementation);
                        }),
                    )?;
                }
                LegacyWin64Import::WsaStartup => {
                    uc("write WSAStartup return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install WSAStartup import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_wsa_startup(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::WsaCleanup => {
                    uc("write WSACleanup return", unicorn.mem_write(stub, &[0xc3]))?;
                    uc(
                        "install WSACleanup import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_wsa_cleanup(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::RtlPcToFileHeader => {
                    uc(
                        "write RtlPcToFileHeader return",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install RtlPcToFileHeader import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_rtl_pc_to_file_header(unicorn);
                        }),
                    )?;
                }
                LegacyWin64Import::RaiseException => {
                    uc(
                        "write RaiseException trap",
                        unicorn.mem_write(stub, &[0xc3]),
                    )?;
                    uc(
                        "install RaiseException import",
                        unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                            emulate_raise_exception(unicorn);
                        }),
                    )?;
                }
            }
        }
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
        let returned = if let Some(value) = guest_environment_value(unicorn.get_data(), &name) {
            let mut terminated = Vec::with_capacity(value.len() + 1);
            terminated.extend_from_slice(&value);
            terminated.push(0);
            let buffer = guest_getenv_buffer(unicorn)?;
            unicorn
                .mem_write(buffer, &terminated)
                .map_err(|error| format!("CRT getenv value write failed: {error}"))?;
            buffer
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
    deterministic_guest_environment_entries()
        .iter()
        .find_map(|(candidate, value)| name.eq_ignore_ascii_case(candidate).then_some(*value))
}

fn deterministic_guest_environment_entries() -> &'static [(&'static [u8], &'static [u8])] {
    // Keep this sorted case-insensitively, matching the ordering of a Windows
    // environment block and the allowlist used by GetEnvironmentVariableA/W.
    &[(b"OPENCV_FOR_THREADS_NUM", b"1")]
}

fn environment_strings_range(state: &mut GuestState) -> Result<(u64, u64), String> {
    if state.environment_strings_base == 0 {
        let namespace = NEXT_ENVIRONMENT_STRINGS_NAMESPACE
            .fetch_update(AtomicOrdering::Relaxed, AtomicOrdering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "GetEnvironmentStringsW namespace exhausted".to_string())?;
        let offset = namespace
            .checked_mul(ENVIRONMENT_STRINGS_NAMESPACE_SIZE)
            .ok_or_else(|| "GetEnvironmentStringsW namespace exhausted".to_string())?;
        state.environment_strings_base = ENVIRONMENT_STRINGS_BASE
            .checked_add(offset)
            .ok_or_else(|| "GetEnvironmentStringsW namespace exhausted".to_string())?;
    }
    let end = state
        .environment_strings_base
        .checked_add(ENVIRONMENT_STRINGS_NAMESPACE_SIZE)
        .filter(|end| *end <= ENVIRONMENT_STRINGS_END)
        .ok_or_else(|| "GetEnvironmentStringsW namespace exhausted".to_string())?;
    Ok((state.environment_strings_base, end - PAGE_SIZE))
}

fn emulate_get_environment_strings_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    let result = (|| -> Result<u64, String> {
        let block = guest_environment_block_w(unicorn.get_data());
        let allocation = unicorn
            .get_data()
            .crt_heap
            .prepare_environment_strings_allocation(block.len() as u64)
            .map_err(|error| error.to_string())?;
        let (range_start, range_end) = environment_strings_range(unicorn.get_data_mut())?;
        let pointer = unicorn
            .get_data()
            .crt_heap
            .first_fit(range_start, range_end, allocation)
            .map_err(|error| error.to_string())?;
        unicorn
            .mem_map(pointer, allocation.backing_size, Prot::READ | Prot::WRITE)
            .map_err(|error| format!("GetEnvironmentStringsW map failed: {error}"))?;
        if let Err(error) = unicorn.mem_write(pointer, &block) {
            let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
            return Err(format!(
                "GetEnvironmentStringsW block write failed: {error}"
            ));
        }
        if let Err(error) = unicorn.get_data_mut().crt_heap.insert(pointer, allocation) {
            let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
            return Err(error.to_string());
        }
        Ok(pointer)
    })();
    match result {
        Ok(pointer) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
        }
        Err(_) => {
            unicorn.get_data_mut().windows_last_error = ERROR_NOT_ENOUGH_MEMORY;
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    }
}

fn emulate_free_environment_strings_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        let allocation = unicorn
            .get_data_mut()
            .crt_heap
            .remove_environment_strings(pointer)
            .map_err(|error| error.to_string())?;
        if let Err(error) = unicorn.mem_unmap(pointer, allocation.backing_size) {
            // Restore ownership if unmapping unexpectedly fails, so callers can
            // retry and the allocation is still cleaned up with the session.
            let _ = unicorn.get_data_mut().crt_heap.insert(pointer, allocation);
            return Err(format!("FreeEnvironmentStringsW unmap failed: {error}"));
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        Err(_) => {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
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
        let Some(value) = guest_environment_value(unicorn.get_data(), &name) else {
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
        terminated.extend_from_slice(&value);
        terminated.push(0);
        if !guest_range_has_permission(unicorn, buffer, terminated.len() as u64, Prot::WRITE)? {
            return Err(format!(
                "environment variable output {buffer:#x} is not writable"
            ));
        }
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
        let Some(value) = guest_environment_value(unicorn.get_data(), name.as_bytes()) else {
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
        if !guest_range_has_permission(unicorn, buffer, terminated.len() as u64, Prot::WRITE)? {
            return Err(format!(
                "environment variable output {buffer:#x} is not writable"
            ));
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
        if let Ok(requested) = std::str::from_utf8(&path) {
            if let Some(library) = guest_library_by_name(unicorn.get_data(), requested) {
                if !library.initialized {
                    return Err(
                        "LoadLibraryA requested a DLL while its initialization is incomplete"
                            .into(),
                    );
                }
                return Ok(Some(library.base));
            }
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
        } else if module.eq_ignore_ascii_case(b"rgbranding.dll") {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            Err(
                "external guest dependency unavailable: RGBranding.dll (LoadLibraryA does not host-load DLLs)"
                    .into(),
            )
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
            if matches!(operation, LegacyWin64Import::InitializeCriticalSectionEx) {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                return Ok(0);
            }
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
            LegacyWin64Import::InitializeCriticalSectionEx => {
                const CRITICAL_SECTION_NO_DEBUG_INFO: u32 = 0x0100_0000;
                const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
                let _spin_count = read_win64_import_argument(unicorn, 1)? as u32;
                let flags = read_win64_import_argument(unicorn, 2)? as u32;
                let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
                    unicorn.get_data_mut().windows_last_error = error;
                    Ok(0)
                };
                if flags & !CRITICAL_SECTION_NO_DEBUG_INFO != 0 {
                    return fail(unicorn, ERROR_INVALID_PARAMETER);
                }
                if unicorn
                    .get_data()
                    .windows_critical_sections
                    .contains_key(&address)
                {
                    return fail(unicorn, ERROR_INVALID_PARAMETER);
                }
                if unicorn.get_data().windows_critical_sections.len()
                    >= MAX_WINDOWS_CRITICAL_SECTIONS
                {
                    return fail(unicorn, ERROR_NOT_ENOUGH_MEMORY);
                }
                if !guest_range_has_permission(
                    unicorn,
                    address,
                    WINDOWS_CRITICAL_SECTION_BYTES as u64,
                    Prot::WRITE,
                )
                .unwrap_or(false)
                {
                    return fail(unicorn, ERROR_INVALID_PARAMETER);
                }
                if unicorn
                    .mem_write(address, &[0; WINDOWS_CRITICAL_SECTION_BYTES])
                    .is_err()
                {
                    return fail(unicorn, ERROR_INVALID_PARAMETER);
                }
                unicorn
                    .get_data_mut()
                    .windows_critical_sections
                    .insert(address, 0);
                Ok(1)
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

fn emulate_windows_srw_lock(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<(), String> {
        let address = read_win64_import_argument(unicorn, 0)?;
        if address == 0
            || !guest_range_has_permission(unicorn, address, 8, Prot::READ | Prot::WRITE)
                .unwrap_or(false)
        {
            return Err(format!(
                "SRW lock storage {address:#x} is not readable and writable"
            ));
        }
        if operation == LegacyWin64Import::InitializeSrwLock {
            if unicorn
                .get_data()
                .windows_srw_locks
                .get(&address)
                .is_some_and(|lock| lock.owner.is_some() || !lock.waiters.is_empty())
            {
                return Err(format!("cannot initialize active SRW lock {address:#x}"));
            }
            unicorn
                .mem_write(address, &[0; 8])
                .map_err(|error| format!("initialize SRW storage failed: {error}"))?;
            // A zero-initialized lock is registered lazily on first acquire,
            // just like SRWLOCK_INIT; initialization needs no host allocation.
            unicorn.get_data_mut().windows_srw_locks.remove(&address);
            return Ok(());
        }
        if !unicorn.get_data().windows_srw_locks.contains_key(&address) {
            if unicorn.get_data().windows_srw_locks.len() >= MAX_WINDOWS_SRW_LOCKS {
                return Err(format!("SRW lock count exceeds {MAX_WINDOWS_SRW_LOCKS}"));
            }
            let bytes = unicorn
                .mem_read_as_vec(address, 8)
                .map_err(|error| format!("read SRW lock storage failed: {error}"))?;
            if bytes != [0; 8] {
                return Err(format!("SRW lock {address:#x} is not initialized by zero"));
            }
            unicorn
                .get_data_mut()
                .windows_srw_locks
                .insert(address, WindowsSrwLock::default());
        }
        let thread_id = unicorn.get_data().current_windows_thread_id;
        match operation {
            LegacyWin64Import::AcquireSrwLockExclusive => {
                let lock = unicorn
                    .get_data_mut()
                    .windows_srw_locks
                    .get_mut(&address)
                    .expect("SRW lock was inserted");
                if lock.owner.is_none() {
                    lock.owner = Some(thread_id);
                    unicorn
                        .mem_write(address, &1u64.to_le_bytes())
                        .map_err(|error| format!("write acquired SRW state failed: {error}"))?;
                    return Ok(());
                }
                if lock.owner == Some(thread_id) {
                    return Err(format!("SRW lock {address:#x} recursive exclusive acquire"));
                }
                if lock.waiters.len() >= MAX_WINDOWS_SRW_WAITERS {
                    return Err(format!(
                        "SRW lock waiter count exceeds {MAX_WINDOWS_SRW_WAITERS}"
                    ));
                }
                if lock.waiters.contains(&thread_id) {
                    return Err(format!(
                        "thread {thread_id} is already waiting on SRW lock {address:#x}"
                    ));
                }
                lock.waiters.push_back(thread_id);
                let rsp = unicorn
                    .reg_read(RegisterX86::RSP)
                    .map_err(|error| format!("read SRW acquire stack failed: {error}"))?;
                let return_address = read_vcomp_u64(unicorn, rsp)?;
                unicorn
                    .reg_write(RegisterX86::RSP, rsp + 8)
                    .map_err(|error| format!("advance SRW acquire stack failed: {error}"))?;
                unicorn
                    .reg_write(RegisterX86::RIP, return_address)
                    .map_err(|error| format!("advance SRW acquire return failed: {error}"))?;
                unicorn.get_data_mut().scheduler_yield_reason = Some(SchedulerYieldReason::SrwLock);
                unicorn.get_data_mut().scheduler_resume_rip = return_address;
                unicorn
                    .emu_stop()
                    .map_err(|error| format!("SRW acquire scheduler stop failed: {error}"))
            }
            LegacyWin64Import::TryAcquireSrwLockExclusive => {
                let lock = unicorn
                    .get_data_mut()
                    .windows_srw_locks
                    .get_mut(&address)
                    .expect("SRW lock was inserted");
                let acquired = lock.owner.is_none();
                if acquired {
                    lock.owner = Some(thread_id);
                    unicorn
                        .mem_write(address, &1u64.to_le_bytes())
                        .map_err(|error| format!("write acquired SRW state failed: {error}"))?;
                }
                unicorn
                    .reg_write(RegisterX86::RAX, u64::from(acquired))
                    .map_err(|error| format!("write SRW try-acquire result failed: {error}"))
            }
            LegacyWin64Import::ReleaseSrwLockExclusive => {
                let lock = unicorn
                    .get_data_mut()
                    .windows_srw_locks
                    .get_mut(&address)
                    .expect("SRW lock was inserted");
                if lock.owner != Some(thread_id) {
                    return Err(format!(
                        "thread {thread_id} does not own SRW lock {address:#x}"
                    ));
                }
                let next = lock.waiters.pop_front();
                lock.owner = next;
                unicorn
                    .mem_write(address, &(u64::from(next.is_some())).to_le_bytes())
                    .map_err(|error| format!("write released SRW state failed: {error}"))?;
                if let Some(waiter) = next.filter(|waiter| *waiter != 1) {
                    unicorn
                        .get_data_mut()
                        .scheduler_woken_threads
                        .push_back(waiter);
                    unicorn.get_data_mut().scheduler_ready_hint = true;
                }
                Ok(())
            }
            _ => Err("invalid SRW lock operation".into()),
        }
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_get_module_handle_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let fail = |unicorn: &mut Unicorn<'_, GuestState>| {
        unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    };
    let returned = if pointer == 0 {
        unicorn.get_data().image_region.map(|(start, _)| start)
    } else {
        let mut units = Vec::new();
        let mut terminated = false;
        for index in 0..128u64 {
            let Some(address) = pointer.checked_add(index.saturating_mul(2)) else {
                break;
            };
            if !guest_range_has_permission(unicorn, address, 2, Prot::READ).unwrap_or(false) {
                break;
            }
            let Ok(bytes) = unicorn.mem_read_as_vec(address, 2) else {
                break;
            };
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                terminated = true;
                break;
            }
            units.push(unit);
        }
        if !terminated {
            None
        } else if let Ok(name) = String::from_utf16(&units) {
            let basename = name
                .rsplit(['\\', '/'])
                .next()
                .filter(|basename| !basename.is_empty());
            match basename {
                Some(name) if name.eq_ignore_ascii_case("kernel32.dll") => {
                    Some(WINDOWS_KERNEL32_MODULE_TOKEN)
                }
                Some(name) if name.eq_ignore_ascii_case("ntdll.dll") => {
                    Some(WINDOWS_NTDLL_MODULE_TOKEN)
                }
                // This API-set lookup is an optional capability probe. Keep
                // it unavailable so the guest takes its modeled fallback.
                _ => guest_library_by_name(unicorn.get_data(), &name).map(|library| library.base),
            }
        } else {
            None
        }
    };
    if let Some(returned) = returned {
        let _ = unicorn.reg_write(RegisterX86::RAX, returned);
    } else {
        fail(unicorn);
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
        guest_module_from_address(unicorn.get_data(), name_or_address)
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
        } else if terminated {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|name| guest_library_by_name(unicorn.get_data(), name))
                .map(|library| library.base)
        } else {
            None
        }
    };

    let Some(module) = module else {
        fail(unicorn, ERROR_MOD_NOT_FOUND);
        return;
    };
    if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE).unwrap_or(false)
        || unicorn.mem_write(output, &module.to_le_bytes()).is_err()
    {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    // Guest images and synthetic system modules live for the worker lifetime,
    // so default, PIN, and UNCHANGED_REFCOUNT all preserve the same stable
    // handle while retaining their documented lookup behavior.
    let _ = unicorn.reg_write(RegisterX86::RAX, 1);
}

fn emulate_get_module_handle_ex_w(unicorn: &mut Unicorn<'_, GuestState>) {
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
    if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE).unwrap_or(false) {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }

    let module = if flags & FROM_ADDRESS != 0 {
        guest_module_from_address(unicorn.get_data(), name_or_address)
    } else if name_or_address == 0 {
        unicorn.get_data().image_region.map(|(start, _)| start)
    } else {
        let mut units = Vec::new();
        let mut terminated = false;
        for index in 0..128u64 {
            let Some(address) = name_or_address.checked_add(index.saturating_mul(2)) else {
                break;
            };
            if !guest_range_has_permission(unicorn, address, 2, Prot::READ).unwrap_or(false) {
                break;
            }
            let Ok(bytes) = unicorn.mem_read_as_vec(address, 2) else {
                break;
            };
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                terminated = true;
                break;
            }
            units.push(unit);
        }
        if !terminated {
            None
        } else if String::from_utf16(&units)
            .is_ok_and(|name| name.eq_ignore_ascii_case("kernel32.dll"))
        {
            Some(WINDOWS_KERNEL32_MODULE_TOKEN)
        } else {
            String::from_utf16(&units)
                .ok()
                .and_then(|name| guest_library_by_name(unicorn.get_data(), &name))
                .map(|library| library.base)
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
    // Both modeled modules have worker-lifetime storage. The default lookup's
    // increment, PIN, and UNCHANGED_REFCOUNT therefore differ only in lifetime
    // policy, not in the stable handle observable by this guest.
    let _ = unicorn.reg_write(RegisterX86::RAX, 1);
}

fn emulate_get_module_file_name_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let module = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
    let output = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let capacity = unicorn.reg_read(RegisterX86::R8).unwrap_or_default() as u32;
    let image_base = unicorn.get_data().image_region.map(|region| region.0);
    let real_path = unicorn
        .get_data()
        .loaded_libraries
        .iter()
        .find(|(_, library)| library.base == module)
        .map(|(name, _)| name.replace('/', "\\"));
    let path = if (module == 0 && image_base.is_some()) || Some(module) == image_base {
        Some(r"C:\AEXCompat\guest-plugin.aex")
    } else if module == WINDOWS_KERNEL32_MODULE_TOKEN {
        Some(r"C:\Windows\System32\kernel32.dll")
    } else if module == WINDOWS_NTDLL_MODULE_TOKEN {
        Some(r"C:\Windows\System32\ntdll.dll")
    } else {
        real_path.as_deref()
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
    if !guest_range_has_permission(unicorn, output, bytes.len() as u64, Prot::WRITE)
        .unwrap_or(false)
        || unicorn.mem_write(output, &bytes).is_err()
    {
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

fn emulate_load_library_w(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    };
    if pointer == 0 {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    let mut units = Vec::new();
    let mut terminated = false;
    for index in 0..512u64 {
        let Some(address) = pointer.checked_add(index * 2) else {
            break;
        };
        let Ok(bytes) = unicorn.mem_read_as_vec(address, 2) else {
            break;
        };
        let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        if unit == 0 {
            terminated = true;
            break;
        }
        units.push(unit);
    }
    if !terminated {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    }
    let Ok(name) = String::from_utf16(&units) else {
        fail(unicorn, ERROR_INVALID_PARAMETER);
        return;
    };
    let normalized = name.replace('/', "\\");
    let path_qualified =
        normalized.contains('\\') || normalized.as_bytes().get(1).copied() == Some(b':');
    let module = if path_qualified {
        if normalized.eq_ignore_ascii_case(r"C:\Windows\System32\kernel32.dll") {
            Some(WINDOWS_KERNEL32_MODULE_TOKEN)
        } else if normalized.eq_ignore_ascii_case(r"C:\AEXCompat\guest-plugin.aex") {
            unicorn.get_data().image_region.map(|region| region.0)
        } else {
            None
        }
    } else {
        let completed = if normalized.contains('.') {
            normalized
        } else {
            format!("{normalized}.dll")
        };
        if completed.eq_ignore_ascii_case("kernel32.dll") {
            Some(WINDOWS_KERNEL32_MODULE_TOKEN)
        } else if completed.eq_ignore_ascii_case("guest-plugin.aex") {
            unicorn.get_data().image_region.map(|region| region.0)
        } else {
            None
        }
    };
    let Some(module) = module else {
        fail(unicorn, ERROR_MOD_NOT_FOUND);
        return;
    };
    let count = unicorn
        .get_data()
        .windows_module_refcounts
        .get(&module)
        .copied()
        .unwrap_or_default();
    if count >= MAX_WINDOWS_MODULE_REFERENCES {
        fail(unicorn, ERROR_NOT_ENOUGH_MEMORY);
        return;
    }
    unicorn
        .get_data_mut()
        .windows_module_refcounts
        .insert(module, count + 1);
    let _ = unicorn.reg_write(RegisterX86::RAX, module);
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

fn install_dynamic_windows_import_callbacks(
    unicorn: &mut Unicorn<'static, GuestState>,
) -> Result<(), GuestError> {
    uc(
        "write dynamic FlsAlloc callback return",
        unicorn.mem_write(HOST_DYNAMIC_FLS_ALLOC, &[0xc3]),
    )?;
    uc(
        "install dynamic FlsAlloc callback",
        unicorn.add_code_hook(
            HOST_DYNAMIC_FLS_ALLOC,
            HOST_DYNAMIC_FLS_ALLOC,
            |unicorn, _, _| {
                emulate_fls(unicorn, LegacyWin64Import::FlsAlloc);
            },
        ),
    )?;
    Ok(())
}

fn emulate_get_proc_address(unicorn: &mut Unicorn<'_, GuestState>) {
    let module = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let pointer = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    };

    let real_module = unicorn
        .get_data()
        .loaded_libraries
        .values()
        .find(|library| library.base == module);
    if real_module.is_none()
        && module != WINDOWS_KERNEL32_MODULE_TOKEN
        && module != WINDOWS_NTDLL_MODULE_TOKEN
    {
        fail(unicorn, ERROR_MOD_NOT_FOUND);
        return;
    }
    // Win32 encodes an ordinal in the low 16 bits of the name pointer. This
    // synthetic module exposes names only, so ordinals fail without probing
    // guest memory at small integer addresses.
    if pointer <= u64::from(u16::MAX) {
        fail(unicorn, ERROR_PROC_NOT_FOUND);
        return;
    }

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
    if let Some(library) = unicorn
        .get_data()
        .loaded_libraries
        .values()
        .find(|library| library.base == module)
    {
        let address = if terminated {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|name| library.exports.get(name))
                .filter(|address| (library.base..library.end).contains(address))
                .copied()
        } else {
            None
        };
        if let Some(address) = address {
            let _ = unicorn.reg_write(RegisterX86::RAX, address);
        } else {
            fail(unicorn, ERROR_PROC_NOT_FOUND);
        }
        return;
    }
    let dynamic = if module == WINDOWS_KERNEL32_MODULE_TOKEN && terminated {
        match bytes.as_slice() {
            b"InitializeConditionVariable" => Some(HOST_INITIALIZE_CONDITION_VARIABLE),
            b"SleepConditionVariableCS" => Some(HOST_SLEEP_CONDITION_VARIABLE_CS),
            b"WakeConditionVariable" => Some(HOST_WAKE_CONDITION_VARIABLE),
            b"WakeAllConditionVariable" => Some(HOST_WAKE_ALL_CONDITION_VARIABLE),
            b"FlsAlloc" => Some(HOST_DYNAMIC_FLS_ALLOC),
            _ => None,
        }
    } else {
        None
    };
    if let Some(dynamic) = dynamic {
        // GetProcAddress leaves last error unspecified on success. Preserve the
        // guest's value so capability probes cannot erase an earlier error.
        let _ = unicorn.reg_write(RegisterX86::RAX, dynamic);
    } else {
        fail(unicorn, ERROR_PROC_NOT_FOUND);
    }
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

fn record_windows_address_waiter(
    state: &mut GuestState,
    address: u64,
    thread_id: u32,
) -> Result<(), String> {
    if address == 0 {
        return Err("WaitOnAddress address is null".into());
    }
    if !state.windows_address_waiters.contains_key(&address)
        && state.windows_address_waiters.len() >= MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS
    {
        return Err(format!(
            "Windows address-wait location count exceeds {MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS}"
        ));
    }
    let waiters = state.windows_address_waiters.entry(address).or_default();
    if !waiters.contains(&thread_id) && waiters.len() >= MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION {
        return Err(format!(
            "Windows address waiter count at {address:#x} exceeds \
             {MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION}"
        ));
    }
    waiters.insert(thread_id);
    Ok(())
}

fn emulate_wake_by_address_all(unicorn: &mut Unicorn<'_, GuestState>) {
    // Windows treats the argument as an address identity for waking purposes;
    // WakeByAddressAll neither returns a status nor needs to read the pointed-to
    // bytes.  This makes NULL, stale, and unmapped no-waiter addresses safe
    // no-ops and avoids leaking host synchronization or memory behavior.
    emulate_wake_by_address(unicorn, true);
}

fn emulate_wake_by_address(unicorn: &mut Unicorn<'_, GuestState>, wake_all: bool) {
    let address = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let state = unicorn.get_data_mut();
    let valid = state.windows_address_waiters.len() <= MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS
        && state
            .windows_address_waiters
            .values()
            .all(|waiters| waiters.len() <= MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION);
    if !valid {
        if state.callback_error.is_none() {
            state.callback_error = Some("Windows address-wait state exceeded its bounds".into());
        }
        let _ = unicorn.emu_stop();
        return;
    }
    let mut awakened = Vec::new();
    if wake_all {
        if let Some(waiters) = state.windows_address_waiters.remove(&address) {
            awakened.extend(waiters);
        }
    } else if let Some(waiters) = state.windows_address_waiters.get_mut(&address) {
        if let Some(thread_id) = waiters.first().copied() {
            waiters.remove(&thread_id);
            awakened.push(thread_id);
        }
        if waiters.is_empty() {
            state.windows_address_waiters.remove(&address);
        }
    }
    for thread_id in awakened {
        if thread_id == 1 {
            if let Some(main_wait) = state.scheduler_main_wait.as_mut() {
                if main_wait.address == address {
                    main_wait.woken = true;
                }
            }
            continue;
        }
        if !state
            .windows_threads
            .values()
            .any(|thread| thread.id == thread_id && !thread.completed)
        {
            continue;
        }
        if state.scheduler_woken_threads.len() >= MAX_WINDOWS_THREADS {
            if state.callback_error.is_none() {
                state.callback_error = Some(format!(
                    "Windows address wake queue exceeds {MAX_WINDOWS_THREADS} threads"
                ));
            }
            let _ = unicorn.emu_stop();
            return;
        }
        state.scheduler_woken_threads.push_back(thread_id);
        state.scheduler_ready_hint = true;
    }
}

fn emulate_wait_on_address(unicorn: &mut Unicorn<'_, GuestState>) {
    const ERROR_TIMEOUT: u32 = 1460;
    let result = (|| -> Result<(), String> {
        let address = read_win64_import_argument(unicorn, 0)?;
        let compare_address = read_win64_import_argument(unicorn, 1)?;
        let address_size = read_win64_import_argument(unicorn, 2)?;
        let milliseconds = read_win64_import_argument(unicorn, 3)? as u32;
        let size = match address_size {
            1 | 2 | 4 | 8 => address_size as usize,
            _ => {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                unicorn.reg_write(RegisterX86::RAX, 0).map_err(|error| {
                    format!("WaitOnAddress invalid-size return failed: {error}")
                })?;
                return Ok(());
            }
        };
        if address == 0 || compare_address == 0 {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("WaitOnAddress null return failed: {error}"))?;
            return Ok(());
        }
        let mut current = [0_u8; 8];
        let mut compare = [0_u8; 8];
        if unicorn.mem_read(address, &mut current[..size]).is_err()
            || unicorn
                .mem_read(compare_address, &mut compare[..size])
                .is_err()
        {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("WaitOnAddress pointer return failed: {error}"))?;
            return Ok(());
        }
        if current[..size] != compare[..size] {
            unicorn
                .reg_write(RegisterX86::RAX, 1)
                .map_err(|error| format!("WaitOnAddress changed return failed: {error}"))?;
            return Ok(());
        }
        if milliseconds == 0 {
            unicorn.get_data_mut().windows_last_error = ERROR_TIMEOUT;
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("WaitOnAddress timeout return failed: {error}"))?;
            return Ok(());
        }
        if milliseconds != u32::MAX && unicorn.get_data().scheduler_switches_remaining <= 1 {
            unicorn.get_data_mut().windows_last_error = ERROR_TIMEOUT;
            unicorn.reg_write(RegisterX86::RAX, 0).map_err(|error| {
                format!("WaitOnAddress exhausted-scheduler timeout return failed: {error}")
            })?;
            return Ok(());
        }
        let thread_id = unicorn.get_data().current_windows_thread_id;
        record_windows_address_waiter(unicorn.get_data_mut(), address, thread_id)?;
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("WaitOnAddress stack read failed: {error}"))?;
        let return_address = read_vcomp_u64(unicorn, rsp)?;
        unicorn
            .reg_write(RegisterX86::RSP, rsp + 8)
            .map_err(|error| format!("WaitOnAddress stack advance failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::RIP, return_address)
            .map_err(|error| format!("WaitOnAddress return target failed: {error}"))?;
        // A resumed wait may be spurious; Win32 callers must re-check their
        // predicate.  TRUE means only that this bounded wait attempt returned.
        unicorn
            .reg_write(RegisterX86::RAX, 1)
            .map_err(|error| format!("WaitOnAddress wake return failed: {error}"))?;
        unicorn.get_data_mut().scheduler_yield_reason = Some(SchedulerYieldReason::AddressWait);
        unicorn.get_data_mut().scheduler_resume_rip = return_address;
        let deadline = (milliseconds != u32::MAX).then(|| {
            let bounded_ticks = u64::from(milliseconds).min(
                unicorn
                    .get_data()
                    .scheduler_switches_remaining
                    .saturating_sub(1),
            );
            unicorn
                .get_data()
                .scheduler_virtual_tick
                .saturating_add(bounded_ticks)
        });
        if unicorn.get_data().pending_windows_thread.is_some() {
            unicorn.get_data_mut().scheduler_wait_deadline = deadline;
        } else {
            unicorn.get_data_mut().scheduler_main_wait = Some(SchedulerMainWait {
                address,
                deadline,
                woken: false,
            });
        }
        unicorn
            .emu_stop()
            .map_err(|error| format!("WaitOnAddress scheduler stop failed: {error}"))
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
    // Use the current wall clock. A fixed historical date would invalidate
    // observations of expiration, licensing, and other time-dependent logic.
    let result = read_win64_import_argument(unicorn, 0).and_then(|output| {
        if output == 0 {
            return Err("GetSystemTimeAsFileTime output pointer is null".to_string());
        }
        if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err("GetSystemTimeAsFileTime output is not writable".into());
        }
        let filetime = windows_filetime(std::time::SystemTime::now())?;
        unicorn
            .mem_write(output, &filetime.to_le_bytes())
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

fn emulate_output_debug_string_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        // With no debugger attached, Windows exposes no observable output to
        // the process.  Keep NULL and the empty string as harmless no-ops, and
        // validate every non-empty guest string without forwarding its bytes
        // to a host debugger, console, log, or file.
        if pointer == 0 {
            return Ok(());
        }

        let regions = unicorn
            .mem_regions()
            .map_err(|error| format!("OutputDebugStringA memory-map query failed: {error}"))?;
        let mut cursor = pointer;
        let mut remaining = MAX_CRT_STRING_BYTES
            .checked_add(1)
            .expect("debug string scan bound fits u64");
        while remaining != 0 {
            let region = regions
                .iter()
                .find(|region| {
                    region.begin <= cursor
                        && cursor <= region.end
                        && region.perms & Prot::READ.0 as u32 == Prot::READ.0 as u32
                })
                .ok_or_else(|| format!("OutputDebugStringA string at {cursor:#x} is unreadable"))?;
            let available = region
                .end
                .checked_sub(cursor)
                .and_then(|length| length.checked_add(1))
                .ok_or_else(|| "OutputDebugStringA readable range overflow".to_string())?;
            let chunk_length = available.min(remaining);
            let chunk_length_usize = usize::try_from(chunk_length)
                .map_err(|_| "OutputDebugStringA chunk length does not fit usize".to_string())?;
            let chunk = unicorn
                .mem_read_as_vec(cursor, chunk_length_usize)
                .map_err(|error| {
                    format!("OutputDebugStringA string at {cursor:#x} is unreadable: {error}")
                })?;
            if chunk.contains(&0) {
                return Ok(());
            }
            remaining -= chunk_length;
            cursor = cursor
                .checked_add(chunk_length)
                .ok_or_else(|| "OutputDebugStringA string address overflow".to_string())?;
        }
        Err(format!(
            "OutputDebugStringA string exceeds {MAX_CRT_STRING_BYTES} bytes without a terminator"
        ))
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

fn emulate_fopen_s(unicorn: &mut Unicorn<'_, GuestState>) {
    const EINVAL: u32 = 22;

    let result = (|| -> Result<u32, String> {
        let result_pointer = read_win64_import_argument(unicorn, 0)?;
        let filename_pointer = read_win64_import_argument(unicorn, 1)?;
        let mode_pointer = read_win64_import_argument(unicorn, 2)?;

        if result_pointer == 0 {
            return Ok(EINVAL);
        }
        if !guest_range_has_permission(unicorn, result_pointer, 8, Prot::WRITE)? {
            return Err(format!(
                "fopen_s result pointer {result_pointer:#x} is not writable"
            ));
        }

        // UCRT's invalid-parameter path preserves a non-null result slot when
        // either string pointer is NULL.  There is no guest-visible `_errno`
        // import in this backend, so the ABI-observable contract here is the
        // returned `errno_t`; the internal errno remains thread-logical and
        // must not be exposed as process-global guest storage.
        if filename_pointer == 0 || mode_pointer == 0 {
            return Ok(EINVAL);
        }
        let filename = read_crt_stdio_c_string(
            unicorn,
            filename_pointer,
            MAX_CRT_STRING_BYTES,
            "fopen_s filename",
        )?;
        const MAX_FOPEN_MODE_BYTES: u64 = 64;
        let mode =
            read_crt_stdio_c_string(unicorn, mode_pointer, MAX_FOPEN_MODE_BYTES, "fopen_s mode")?;
        let (stream, errno) = open_guest_stream(unicorn, &filename, &mode)?;
        unicorn
            .mem_write(result_pointer, &stream.to_le_bytes())
            .map_err(|error| format!("fopen_s result write failed: {error}"))?;
        Ok(errno)
    })();

    match result {
        Ok(errno) => {
            unicorn.get_data_mut().crt_errno = errno;
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(errno));
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strncpy_s(unicorn: &mut Unicorn<'_, GuestState>) {
    const EINVAL: u32 = 22;
    const ERANGE: u32 = 34;
    const STRUNCATE: u32 = 80;
    const TRUNCATE: u64 = u64::MAX;
    const RSIZE_MAX: u64 = u64::MAX >> 1;

    let result = (|| -> Result<(u32, bool), String> {
        let destination = read_win64_import_argument(unicorn, 0)?;
        let destination_size = read_win64_import_argument(unicorn, 1)?;
        let source = read_win64_import_argument(unicorn, 2)?;
        let count = read_win64_import_argument(unicorn, 3)?;

        if destination == 0 || destination_size == 0 {
            return Ok((EINVAL, true));
        }
        if destination_size > RSIZE_MAX {
            return Ok((EINVAL, true));
        }
        if destination_size > MAX_CRT_STRING_BYTES {
            return Err(format!(
                "strncpy_s destination size {destination_size} exceeds bounded {MAX_CRT_STRING_BYTES} bytes"
            ));
        }
        if !guest_range_has_permission(unicorn, destination, destination_size, Prot::WRITE)? {
            return Err(format!(
                "strncpy_s destination range {destination:#x}+{destination_size} is not fully writable"
            ));
        }
        if count != TRUNCATE && count > RSIZE_MAX {
            unicorn
                .mem_write(destination, &[0])
                .map_err(|error| format!("strncpy_s destination reset failed: {error}"))?;
            return Ok((EINVAL, true));
        }
        if source == 0 {
            unicorn
                .mem_write(destination, &[0])
                .map_err(|error| format!("strncpy_s destination reset failed: {error}"))?;
            return Ok((EINVAL, true));
        }

        let read_limit = if count == TRUNCATE {
            destination_size
        } else {
            count.min(destination_size)
        };
        let source_bytes = read_strncpy_s_source(unicorn, source, read_limit)?;
        let terminator = source_bytes.iter().position(|byte| *byte == 0);
        let source_span = terminator.map_or(read_limit, |index| index as u64 + 1);
        if source_span != 0 {
            let destination_end = destination
                .checked_add(destination_size)
                .ok_or_else(|| "strncpy_s destination range overflow".to_string())?;
            let source_end = source
                .checked_add(source_span)
                .ok_or_else(|| "strncpy_s source range overflow".to_string())?;
            if destination < source_end && source < destination_end {
                unicorn
                    .mem_write(destination, &[0])
                    .map_err(|error| format!("strncpy_s destination reset failed: {error}"))?;
                return Ok((EINVAL, true));
            }
        }

        let (mut output, errno, set_errno) = if let Some(terminator) = terminator {
            (source_bytes[..terminator].to_vec(), 0, false)
        } else if count == TRUNCATE {
            let copied = destination_size.saturating_sub(1) as usize;
            (source_bytes[..copied].to_vec(), STRUNCATE, false)
        } else if count < destination_size {
            (source_bytes, 0, false)
        } else {
            unicorn
                .mem_write(destination, &[0])
                .map_err(|error| format!("strncpy_s destination reset failed: {error}"))?;
            return Ok((ERANGE, true));
        };
        output.push(0);
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("strncpy_s destination write failed: {error}"))?;
        Ok((errno, set_errno))
    })();

    match result {
        Ok((errno, set_errno)) => {
            if set_errno {
                unicorn.get_data_mut().crt_errno = errno;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(errno));
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn read_strncpy_s_source(
    unicorn: &Unicorn<'_, GuestState>,
    source: u64,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let regions = unicorn
        .mem_regions()
        .map_err(|error| format!("strncpy_s memory-map query failed: {error}"))?;
    let mut bytes = Vec::new();
    let mut cursor = source;
    let mut remaining = limit;
    while remaining != 0 {
        let region = regions
            .iter()
            .find(|region| {
                region.begin <= cursor
                    && cursor <= region.end
                    && region.perms & Prot::READ.0 as u32 == Prot::READ.0 as u32
            })
            .ok_or_else(|| format!("strncpy_s source at {cursor:#x} is not readable"))?;
        let available = region
            .end
            .checked_sub(cursor)
            .and_then(|length| length.checked_add(1))
            .ok_or_else(|| "strncpy_s source range overflow".to_string())?;
        let chunk_length = available.min(remaining);
        let chunk = unicorn
            .mem_read_as_vec(
                cursor,
                usize::try_from(chunk_length)
                    .map_err(|_| "strncpy_s source length does not fit usize".to_string())?,
            )
            .map_err(|error| format!("strncpy_s source read failed: {error}"))?;
        if let Some(terminator) = chunk.iter().position(|byte| *byte == 0) {
            bytes.extend_from_slice(&chunk[..=terminator]);
            break;
        }
        bytes.extend_from_slice(&chunk);
        remaining -= chunk_length;
        cursor = cursor
            .checked_add(chunk_length)
            .ok_or_else(|| "strncpy_s source range overflow".to_string())?;
    }
    Ok(bytes)
}

fn trim_leading_crt_mode_spaces(mut value: &[u8]) -> &[u8] {
    while let Some(rest) = value.strip_prefix(b" ") {
        value = rest;
    }
    value
}

fn valid_fopen_mode(mode: &[u8]) -> bool {
    let mode = trim_leading_crt_mode_spaces(mode);
    let Some((&first, suffix)) = mode.split_first() else {
        return false;
    };
    if !matches!(first, b'r' | b'w' | b'a') {
        return false;
    }

    let mut seen_plus = false;
    let mut seen_text_mode = false;
    let mut seen_commit_mode = false;
    let mut seen_scan_mode = false;
    let mut seen_temporary = false;
    let mut seen_delete = false;
    let mut parse_flag = |byte: u8| -> bool {
        match byte {
            b' ' => true,
            b'+' if !seen_plus => {
                seen_plus = true;
                true
            }
            b'b' | b't' if !seen_text_mode => {
                seen_text_mode = true;
                true
            }
            b'c' | b'n' if !seen_commit_mode => {
                seen_commit_mode = true;
                true
            }
            b'S' | b'R' if !seen_scan_mode => {
                seen_scan_mode = true;
                true
            }
            b'T' if !seen_temporary => {
                seen_temporary = true;
                true
            }
            b'D' if !seen_delete => {
                seen_delete = true;
                true
            }
            b'N' => true,
            b'x' if first == b'w' => true,
            _ => false,
        }
    };

    let (core_flags, comma_options) = match suffix.iter().position(|byte| *byte == b',') {
        Some(comma) => (&suffix[..comma], Some(&suffix[comma + 1..])),
        None => (suffix, None),
    };
    if !core_flags.iter().copied().all(&mut parse_flag) {
        return false;
    }

    let Some(mut options) = comma_options else {
        return true;
    };
    options = trim_leading_crt_mode_spaces(options);
    if options.is_empty() {
        return false;
    }

    let Some(mut encoding) = options.strip_prefix(b"ccs") else {
        return false;
    };
    encoding = trim_leading_crt_mode_spaces(encoding);
    let Some(rest) = encoding.strip_prefix(b"=") else {
        return false;
    };
    encoding = trim_leading_crt_mode_spaces(rest);
    let encoding_length = [b"UTF-16LE".as_slice(), b"UNICODE", b"UTF-8"]
        .into_iter()
        .find(|candidate| {
            encoding.len() >= candidate.len()
                && encoding[..candidate.len()].eq_ignore_ascii_case(candidate)
        })
        .map(<[u8]>::len);
    let Some(encoding_length) = encoding_length else {
        return false;
    };
    encoding[encoding_length..].iter().all(|byte| *byte == b' ')
}

fn guest_range_has_permission(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    length: u64,
    permission: Prot,
) -> Result<bool, String> {
    if length == 0 {
        return Ok(true);
    }
    let Some(end) = address.checked_add(length - 1) else {
        return Ok(false);
    };
    let regions = unicorn
        .mem_regions()
        .map_err(|error| format!("guest memory-map query failed: {error}"))?;
    let permission = permission.0 as u32;
    let mut cursor = address;
    while cursor <= end {
        let Some(region) = regions.iter().find(|region| {
            region.begin <= cursor
                && cursor <= region.end
                && region.perms & permission == permission
        }) else {
            return Ok(false);
        };
        if region.end >= end {
            return Ok(true);
        }
        let Some(next) = region.end.checked_add(1) else {
            return Ok(false);
        };
        cursor = next;
    }
    Ok(true)
}

fn emulate_sh_get_folder_path_a(unicorn: &mut Unicorn<'_, GuestState>) {
    const CSIDL_COMMON_APPDATA: u32 = 0x23;
    const CSIDL_PROGRAM_FILES: u32 = 0x26;
    const SHGFP_TYPE_CURRENT: u32 = 0;
    const COMMON_APPDATA_PATH: &[u8] = b"C:\\ProgramData\0";
    const PROGRAM_FILES_PATH: &[u8] = b"C:\\Program Files\0";

    let result = (|| {
        let hwnd = read_win64_import_argument(unicorn, 0).map_err(|_| HRESULT_E_INVALIDARG)?;
        let csidl =
            read_win64_import_argument(unicorn, 1).map_err(|_| HRESULT_E_INVALIDARG)? as u32;
        let token = read_win64_import_argument(unicorn, 2).map_err(|_| HRESULT_E_INVALIDARG)?;
        let flags =
            read_win64_import_argument(unicorn, 3).map_err(|_| HRESULT_E_INVALIDARG)? as u32;
        let output = read_win64_import_argument(unicorn, 4).map_err(|_| HRESULT_E_INVALIDARG)?;
        if hwnd != 0 || token != 0 || flags != SHGFP_TYPE_CURRENT || output == 0 {
            return Err(HRESULT_E_INVALIDARG);
        }
        let path = match csidl {
            CSIDL_COMMON_APPDATA => COMMON_APPDATA_PATH,
            CSIDL_PROGRAM_FILES => PROGRAM_FILES_PATH,
            _ => return Err(HRESULT_E_INVALIDARG),
        };
        if !guest_range_has_permission(unicorn, output, WINDOWS_MAX_PATH_BYTES, Prot::WRITE)
            .unwrap_or(false)
        {
            return Err(HRESULT_E_INVALIDARG);
        }
        unicorn
            .mem_write(output, path)
            .map_err(|_| HRESULT_E_INVALIDARG)?;
        Ok(0)
    })();
    let returned = result.unwrap_or_else(|error| error);
    let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(returned));
}

fn emulate_nt_write_file(unicorn: &mut Unicorn<'_, GuestState>) {
    const STATUS_SUCCESS: u32 = 0x0000_0000;
    const STATUS_ACCESS_VIOLATION: u32 = 0xc000_0005;
    const STATUS_INVALID_HANDLE: u32 = 0xc000_0008;
    const STATUS_INVALID_PARAMETER: u32 = 0xc000_000d;
    const IO_STATUS_BLOCK_SIZE: u64 = 16;
    const MAX_DIAGNOSTIC_WRITE_BYTES: u32 = 1024 * 1024;

    let result = (|| -> Result<u32, String> {
        let file_handle = read_win64_import_argument(unicorn, 0)?;
        let event = read_win64_import_argument(unicorn, 1)?;
        let apc_routine = read_win64_import_argument(unicorn, 2)?;
        let apc_context = read_win64_import_argument(unicorn, 3)?;
        let io_status_block = read_win64_import_argument(unicorn, 4)?;
        let buffer = read_win64_import_argument(unicorn, 5)?;
        // Length is an ULONG.  The upper half of its Win64 argument register or
        // stack slot is unspecified and is non-zero in the observed Rust AEXes.
        let length = read_win64_import_argument(unicorn, 6)? as u32;
        let byte_offset = read_win64_import_argument(unicorn, 7)?;
        let key = read_win64_import_argument(unicorn, 8)?;

        if !matches!(
            file_handle,
            WINDOWS_STANDARD_OUTPUT_TOKEN | WINDOWS_STANDARD_ERROR_TOKEN
        ) {
            return Ok(STATUS_INVALID_HANDLE);
        }
        // The synthetic streams are synchronous diagnostic sinks.  They have
        // no host object capable of signaling events or dispatching APCs, and
        // do not model seekable offsets or keyed file operations.
        if event != 0
            || apc_routine != 0
            || apc_context != 0
            || byte_offset != 0
            || key != 0
            || length > MAX_DIAGNOSTIC_WRITE_BYTES
        {
            return Ok(STATUS_INVALID_PARAMETER);
        }
        if io_status_block == 0
            || !guest_range_has_permission(
                unicorn,
                io_status_block,
                IO_STATUS_BLOCK_SIZE,
                Prot::WRITE,
            )?
        {
            return Ok(STATUS_ACCESS_VIOLATION);
        }
        if length != 0
            && (buffer == 0
                || !guest_range_has_permission(unicorn, buffer, u64::from(length), Prot::READ)?)
        {
            return Ok(STATUS_ACCESS_VIOLATION);
        }

        // Read only after every pointer and policy check has passed.  This is
        // bounded above and intentionally discarded: guest diagnostics must
        // never reach host descriptors, terminals, or files.
        if length != 0 {
            unicorn
                .mem_read_as_vec(buffer, length as usize)
                .map_err(|error| format!("NtWriteFile payload read failed: {error}"))?;
        }
        let mut io_status = [0u8; IO_STATUS_BLOCK_SIZE as usize];
        io_status[..4].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
        io_status[8..].copy_from_slice(&u64::from(length).to_le_bytes());
        unicorn
            .mem_write(io_status_block, &io_status)
            .map_err(|error| format!("NtWriteFile IO_STATUS_BLOCK write failed: {error}"))?;
        Ok(STATUS_SUCCESS)
    })();

    let status = match result {
        Ok(status) => status,
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            STATUS_ACCESS_VIOLATION
        }
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(status));
}

fn emulate_create_file_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const INVALID_HANDLE_VALUE: u64 = u64::MAX;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const GENERIC_ALL: u32 = 0x1000_0000;
    const DELETE: u32 = 0x0001_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const WRITE_OWNER: u32 = 0x0008_0000;
    const FILE_WRITE_DATA: u32 = 0x0000_0002;
    const FILE_APPEND_DATA: u32 = 0x0000_0004;
    const FILE_WRITE_EA: u32 = 0x0000_0010;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0000_0100;
    const OPEN_EXISTING: u32 = 3;
    const MAX_PATH_UNITS: usize = 32_767;

    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, INVALID_HANDLE_VALUE);
    };
    let result = (|| {
        let path_pointer = read_win64_import_argument(unicorn, 0)?;
        if path_pointer == 0 {
            // A missing filename is an ordinary API failure, not corruption
            // of the guest execution environment. ERROR_PATH_NOT_FOUND keeps
            // it in the same path-resolution family as an empty filename and
            // avoids turning plugin input into a worker-level abort.
            fail(unicorn, ERROR_PATH_NOT_FOUND);
            return Ok(());
        }
        let mut units = Vec::new();
        for index in 0..=MAX_PATH_UNITS {
            let address = path_pointer
                .checked_add((index as u64) * 2)
                .ok_or_else(|| "CreateFileW path range overflows".to_string())?;
            let bytes = unicorn.mem_read_as_vec(address, 2).map_err(|error| {
                format!("CreateFileW path {path_pointer:#x} is not fully readable: {error}")
            })?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                break;
            }
            if index == MAX_PATH_UNITS {
                // The mapped string is readable and terminated beyond the
                // supported Win32 path bound. This is plugin-controlled API
                // input, so report the ordinary filename-range error rather
                // than escalating it to a worker execution failure.
                fail(unicorn, ERROR_FILENAME_EXCED_RANGE);
                return Ok(());
            }
            units.push(unit);
        }
        if char::decode_utf16(units.iter().copied()).any(|character| character.is_err()) {
            fail(unicorn, ERROR_INVALID_PARAMETER);
            return Ok(());
        }
        if units.is_empty() {
            fail(unicorn, ERROR_PATH_NOT_FOUND);
            return Ok(());
        }
        let desired_access = read_win64_import_argument(unicorn, 1)? as u32;
        let share_mode = read_win64_import_argument(unicorn, 2)? as u32;
        let security_attributes = read_win64_import_argument(unicorn, 3)?;
        let creation_disposition = read_win64_import_argument(unicorn, 4)? as u32;
        let _flags_and_attributes = read_win64_import_argument(unicorn, 5)? as u32;
        let template_file = read_win64_import_argument(unicorn, 6)?;
        if share_mode & !0x7 != 0 {
            fail(unicorn, ERROR_INVALID_PARAMETER);
            return Ok(());
        }

        let path = String::from_utf16(&units)
            .map_err(|_| "CreateFileW path contains malformed UTF-16".to_string())?;
        let normalized = path.replace('/', "\\");
        let components = normalized.split('\\').collect::<Vec<_>>();
        let is_host_escape = normalized.starts_with("\\\\")
            || normalized.starts_with("\\?\\")
            || normalized.starts_with("\\.\\")
            || components.iter().any(|component| *component == "..");
        let write_access = desired_access
            & (GENERIC_WRITE
                | GENERIC_ALL
                | DELETE
                | WRITE_DAC
                | WRITE_OWNER
                | FILE_WRITE_DATA
                | FILE_APPEND_DATA
                | FILE_WRITE_EA
                | FILE_WRITE_ATTRIBUTES)
            != 0;
        if is_host_escape || write_access || creation_disposition != OPEN_EXISTING {
            fail(unicorn, ERROR_ACCESS_DENIED);
            return Ok(());
        }

        // The corpus bundle contains only the AEX image and no explicitly
        // mounted plugin assets. A valid read-only open therefore observes the
        // same missing-file result as a sandbox with an empty asset namespace.
        // Crucially, the guest path is never translated to or opened on the host.
        // lpSecurityAttributes is only relevant if a new object is created,
        // and hTemplateFile is not consumed for OPEN_EXISTING. Since no object
        // exists in this namespace, neither legal optional argument is
        // dereferenced or treated as an invalid pointer/handle.
        let _ = (security_attributes, template_file);
        fail(unicorn, ERROR_FILE_NOT_FOUND);
        Ok(())
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, INVALID_HANDLE_VALUE);
        let _ = unicorn.emu_stop();
    }
}

fn emulate_find_first_file_ex_w(unicorn: &mut Unicorn<'_, GuestState>) {
    const INVALID_HANDLE_VALUE: u64 = u64::MAX;
    const MAX_PATH_UNITS: usize = 32_767;
    const WIN32_FIND_DATA_W_SIZE: u64 = 592;
    const FIND_EX_INFO_BASIC: u32 = 1;
    const FIND_EX_SEARCH_LIMIT_TO_DIRECTORIES: u32 = 1;
    const FIND_EX_SEARCH_LIMIT_TO_DEVICES: u32 = 2;
    const SUPPORTED_ADDITIONAL_FLAGS: u32 = 0x1 | 0x2 | 0x4;

    let fail = |unicorn: &mut Unicorn<'_, GuestState>, error: u32| {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, INVALID_HANDLE_VALUE);
    };
    let result = (|| {
        let path_pointer = read_win64_import_argument(unicorn, 0)?;
        if path_pointer == 0 {
            fail(unicorn, ERROR_PATH_NOT_FOUND);
            return Ok(());
        }
        let mut units = Vec::new();
        for index in 0..=MAX_PATH_UNITS {
            let address = path_pointer
                .checked_add((index as u64) * 2)
                .ok_or_else(|| "FindFirstFileExW path range overflows".to_string())?;
            let bytes = unicorn.mem_read_as_vec(address, 2).map_err(|error| {
                format!("FindFirstFileExW path {path_pointer:#x} is not fully readable: {error}")
            })?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                break;
            }
            if index == MAX_PATH_UNITS {
                fail(unicorn, ERROR_FILENAME_EXCED_RANGE);
                return Ok(());
            }
            units.push(unit);
        }
        if char::decode_utf16(units.iter().copied()).any(|character| character.is_err()) {
            fail(unicorn, ERROR_INVALID_PARAMETER);
            return Ok(());
        }
        if units.is_empty() {
            fail(unicorn, ERROR_PATH_NOT_FOUND);
            return Ok(());
        }

        let info_level = read_win64_import_argument(unicorn, 1)? as u32;
        let output = read_win64_import_argument(unicorn, 2)?;
        let search_op = read_win64_import_argument(unicorn, 3)? as u32;
        let search_filter = read_win64_import_argument(unicorn, 4)?;
        let additional_flags = read_win64_import_argument(unicorn, 5)? as u32;
        if info_level > FIND_EX_INFO_BASIC
            || search_op > FIND_EX_SEARCH_LIMIT_TO_DEVICES
            || search_filter != 0
            || additional_flags & !SUPPORTED_ADDITIONAL_FLAGS != 0
            || output == 0
            || !guest_range_has_permission(unicorn, output, WIN32_FIND_DATA_W_SIZE, Prot::WRITE)?
        {
            fail(unicorn, ERROR_INVALID_PARAMETER);
            return Ok(());
        }
        if search_op > FIND_EX_SEARCH_LIMIT_TO_DIRECTORIES {
            fail(unicorn, ERROR_NOT_SUPPORTED);
            return Ok(());
        }

        let path = String::from_utf16(&units)
            .map_err(|_| "FindFirstFileExW path contains malformed UTF-16".to_string())?;
        let normalized = path.replace('/', "\\");
        let components = normalized.split('\\').collect::<Vec<_>>();
        if normalized.starts_with("\\\\")
            || normalized.starts_with("\\?\\")
            || normalized.starts_with("\\.\\")
            || components.iter().any(|component| *component == "..")
        {
            fail(unicorn, ERROR_ACCESS_DENIED);
            return Ok(());
        }

        // The sealed guest namespace contains no mounted directory entries.
        // Do not translate the guest path or consult the host filesystem, and
        // leave WIN32_FIND_DATAW untouched when no match exists.
        fail(unicorn, ERROR_FILE_NOT_FOUND);
        Ok(())
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.reg_write(RegisterX86::RAX, INVALID_HANDLE_VALUE);
        let _ = unicorn.emu_stop();
    }
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
) -> Result<u64, String> {
    let handle = read_win64_import_argument(unicorn, 0)?;
    if handle != PROCESS_HEAP_HANDLE
        && !unicorn
            .get_data()
            .windows_private_heaps
            .contains_key(&handle)
    {
        return Err(format!(
            "{operation} rejected unknown heap handle {handle:#x}"
        ));
    }
    Ok(handle)
}

fn require_heap_allocation_owner(
    unicorn: &Unicorn<'_, GuestState>,
    operation: &str,
    handle: u64,
    pointer: u64,
) -> Result<(), String> {
    if handle == PROCESS_HEAP_HANDLE {
        if unicorn
            .get_data()
            .windows_private_heaps
            .values()
            .any(|allocations| allocations.contains(&pointer))
        {
            return Err(format!(
                "{operation} rejected private allocation {pointer:#x} on the process heap"
            ));
        }
    } else if !unicorn
        .get_data()
        .windows_private_heaps
        .get(&handle)
        .is_some_and(|allocations| allocations.contains(&pointer))
    {
        return Err(format!(
            "{operation} rejected allocation {pointer:#x} not owned by heap {handle:#x}"
        ));
    }
    Ok(())
}

fn emulate_private_heap_lifecycle(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    match operation {
        LegacyWin64Import::HeapCreate => {
            let flags = read_win64_import_argument(unicorn, 0).unwrap_or(u64::MAX);
            let initial_size = read_win64_import_argument(unicorn, 1).unwrap_or(u64::MAX);
            let maximum_size = read_win64_import_argument(unicorn, 2).unwrap_or(u64::MAX);
            if flags & !u64::from(HEAP_NO_SERIALIZE) != 0
                || initial_size != 0
                || maximum_size != 0
                || unicorn.get_data().windows_private_heaps.len() >= 64
            {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                return;
            }
            let generation = NEXT_PRIVATE_HEAP_TOKEN.fetch_add(1, AtomicOrdering::Relaxed);
            let Some(handle) = PRIVATE_HEAP_TOKEN_BASE.checked_add(generation) else {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                return;
            };
            unicorn
                .get_data_mut()
                .windows_private_heaps
                .insert(handle, BTreeSet::new());
            let _ = unicorn.reg_write(RegisterX86::RAX, handle);
        }
        LegacyWin64Import::HeapDestroy => {
            let handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
            if handle == PROCESS_HEAP_HANDLE {
                return fail_process_heap(unicorn, "HeapDestroy rejected the process heap".into());
            }
            let Some(allocations) = unicorn.get_data_mut().windows_private_heaps.remove(&handle)
            else {
                return fail_process_heap(
                    unicorn,
                    format!("HeapDestroy rejected unknown heap handle {handle:#x}"),
                );
            };
            for pointer in allocations {
                let allocation = match unicorn.get_data_mut().crt_heap.remove_process_heap(pointer)
                {
                    Ok(allocation) => allocation,
                    Err(error) => {
                        return fail_process_heap(
                            unicorn,
                            format!("HeapDestroy allocation {pointer:#x} removal failed: {error}"),
                        );
                    }
                };
                if let Err(error) = unicorn.mem_unmap(pointer, allocation.backing_size) {
                    return fail_process_heap(
                        unicorn,
                        format!("HeapDestroy allocation {pointer:#x} unmap failed: {error}"),
                    );
                }
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        _ => fail_process_heap(unicorn, "invalid private heap lifecycle operation".into()),
    }
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
    handle: u64,
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
    if handle != PROCESS_HEAP_HANDLE {
        let Some(allocations) = unicorn
            .get_data_mut()
            .windows_private_heaps
            .get_mut(&handle)
        else {
            let _ = unicorn.get_data_mut().crt_heap.remove_process_heap(pointer);
            let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
            return Err(CrtHeapError::AllocatorMismatch);
        };
        allocations.insert(pointer);
    }
    Ok(pointer)
}

fn free_process_heap_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    handle: u64,
    pointer: u64,
) -> Result<(), String> {
    let allocation = unicorn
        .get_data_mut()
        .crt_heap
        .remove_process_heap(pointer)
        .map_err(|error| error.to_string())?;
    unicorn
        .mem_unmap(pointer, allocation.backing_size)
        .map_err(|error| format!("unmap process heap allocation {pointer:#x}: {error}"))?;
    if handle != PROCESS_HEAP_HANDLE {
        if let Some(allocations) = unicorn
            .get_data_mut()
            .windows_private_heaps
            .get_mut(&handle)
        {
            allocations.remove(&pointer);
        }
    }
    Ok(())
}

fn emulate_heap_alloc(unicorn: &mut Unicorn<'_, GuestState>) {
    let arguments = (|| -> Result<(u64, u32, u64), String> {
        let handle = require_process_heap_handle(unicorn, "HeapAlloc")?;
        let flags = read_process_heap_flags(unicorn, "HeapAlloc", HEAP_ALLOC_ALLOWED_FLAGS)?;
        let size = read_win64_import_argument(unicorn, 2)?;
        Ok((handle, flags, size))
    })();
    let (handle, flags, size) = match arguments {
        Ok(arguments) => arguments,
        Err(error) => return fail_process_heap(unicorn, error),
    };

    let pointer = match allocate_process_heap_region(unicorn, handle, size) {
        Ok(pointer) => pointer,
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    };
    if flags & HEAP_ZERO_MEMORY != 0 {
        let length = size.max(1) as usize;
        if let Err(error) = unicorn.mem_write(pointer, &vec![0; length]) {
            let _ = free_process_heap_region(unicorn, handle, pointer);
            return fail_process_heap(unicorn, format!("HeapAlloc zero-fill failed: {error}"));
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_heap_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let (handle, pointer) = match (|| -> Result<(u64, u64), String> {
        let handle = require_process_heap_handle(unicorn, "HeapFree")?;
        let _ = read_process_heap_flags(unicorn, "HeapFree", HEAP_NO_SERIALIZE)?;
        let pointer = read_win64_import_argument(unicorn, 2)?;
        if pointer != 0 {
            require_heap_allocation_owner(unicorn, "HeapFree", handle, pointer)?;
        }
        Ok((handle, pointer))
    })() {
        Ok(pointer) => pointer,
        Err(error) => return fail_process_heap(unicorn, error),
    };
    if pointer == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        return;
    }
    match free_process_heap_region(unicorn, handle, pointer) {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        Err(error) => fail_process_heap(unicorn, format!("HeapFree failed: {error}")),
    }
}

fn emulate_heap_realloc(unicorn: &mut Unicorn<'_, GuestState>) {
    let arguments = (|| -> Result<(u64, u32, u64, u64), String> {
        let handle = require_process_heap_handle(unicorn, "HeapReAlloc")?;
        let flags = read_process_heap_flags(unicorn, "HeapReAlloc", HEAP_REALLOC_ALLOWED_FLAGS)?;
        let pointer = read_win64_import_argument(unicorn, 2)?;
        let size = read_win64_import_argument(unicorn, 3)?;
        if pointer == 0 {
            return Err("HeapReAlloc pointer is null".into());
        }
        require_heap_allocation_owner(unicorn, "HeapReAlloc", handle, pointer)?;
        Ok((handle, flags, pointer, size))
    })();
    let (handle, flags, pointer, size) = match arguments {
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
    if handle != PROCESS_HEAP_HANDLE {
        let Some(allocations) = unicorn
            .get_data_mut()
            .windows_private_heaps
            .get_mut(&handle)
        else {
            return fail_process_heap(unicorn, "HeapReAlloc lost private heap ownership".into());
        };
        allocations.remove(&pointer);
        allocations.insert(new_pointer);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, new_pointer);
}

fn fail_windows_thread_callback(unicorn: &mut Unicorn<'_, GuestState>, error: String) {
    unicorn.get_data_mut().scheduler_parent_context = None;
    if let Some(pending) = unicorn.get_data_mut().pending_windows_thread.take() {
        let stack = unicorn
            .get_data()
            .windows_threads
            .get(&pending.handle)
            .filter(|thread| thread.stack_mapped)
            .map(|thread| (thread.stack_base, thread.stack_size));
        if let Some((stack_base, stack_size)) = stack {
            let _ = unicorn.mem_unmap(stack_base, stack_size);
            if let Some(thread) = unicorn
                .get_data_mut()
                .windows_threads
                .get_mut(&pending.handle)
            {
                thread.stack_mapped = false;
            }
        }
        restore_windows_thread_context(unicorn.get_data_mut(), &pending);
        let _ = unicorn.mem_write(0x08, &pending.caller_teb_stack);
    }
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(error);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    let _ = unicorn.emu_stop();
}

fn restore_windows_thread_context(state: &mut GuestState, pending: &PendingWindowsThread) {
    for (index, value) in &pending.caller_tls_values {
        if let Some(slot) = state.windows_tls_slots.get_mut(index) {
            *slot = *value;
        }
    }
    for (index, value) in &mut state.windows_tls_slots {
        if !pending.caller_tls_values.contains_key(index) {
            *value = 0;
        }
    }
    for (index, slot) in &mut state.windows_fls_slots {
        if let Some(value) = pending.caller_fls_values.get(index) {
            slot.value = *value;
        } else {
            slot.value = 0;
        }
    }
    state.windows_last_error = pending.caller_last_error;
    state.crt_errno = pending.caller_crt_errno;
    state.windows_thread_error_mode = pending.caller_thread_error_mode;
    state.current_windows_thread_id = pending.caller_thread_id;
}

fn continue_windows_thread(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| -> Result<(), String> {
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("CreateThread callback stack read failed: {error}"))?;
        let mut pending = unicorn
            .get_data_mut()
            .pending_windows_thread
            .take()
            .ok_or_else(|| "CreateThread continuation has no pending callback".to_string())?;
        unicorn.get_data_mut().pending_windows_thread = Some(pending.clone());
        if rsp != pending.callback_return_rsp {
            return Err(format!(
                "CreateThread callback stack {rsp:#x} does not match {:#x}",
                pending.callback_return_rsp
            ));
        }
        if pending.exit_code.is_none() {
            pending.exit_code =
                Some(unicorn.reg_read(RegisterX86::RAX).map_err(|error| {
                    format!("CreateThread callback exit code read failed: {error}")
                })? as u32);
        }
        const MAX_FLS_DESTRUCTOR_PASSES: u32 = 4;
        loop {
            let candidate = unicorn
                .get_data()
                .windows_fls_slots
                .iter()
                .find(|(index, slot)| {
                    slot.callback != 0 && slot.value != 0 && !pending.fls_processed.contains(index)
                })
                .map(|(index, slot)| (*index, slot.callback, slot.value));
            if let Some((index, callback, value)) = candidate {
                unicorn
                    .get_data_mut()
                    .windows_fls_slots
                    .get_mut(&index)
                    .expect("observed FLS slot")
                    .value = 0;
                pending.fls_processed.insert(index);
                let callback_rsp = pending.callback_return_rsp - 8;
                unicorn
                    .mem_write(callback_rsp, &HOST_CREATE_THREAD_CONTINUE.to_le_bytes())
                    .map_err(|error| {
                        format!("FLS destructor continuation write failed: {error}")
                    })?;
                unicorn
                    .reg_write(RegisterX86::RSP, callback_rsp)
                    .map_err(|error| format!("FLS destructor stack write failed: {error}"))?;
                unicorn
                    .reg_write(RegisterX86::RCX, value)
                    .map_err(|error| format!("FLS destructor value write failed: {error}"))?;
                unicorn
                    .reg_write(RegisterX86::R11, callback)
                    .map_err(|error| format!("FLS destructor target write failed: {error}"))?;
                unicorn.get_data_mut().pending_windows_thread = Some(pending);
                return Ok(());
            }
            let needs_another_pass = unicorn
                .get_data()
                .windows_fls_slots
                .values()
                .any(|slot| slot.callback != 0 && slot.value != 0);
            if !needs_another_pass {
                break;
            }
            if pending.fls_pass >= MAX_FLS_DESTRUCTOR_PASSES {
                return Err(format!(
                    "guest thread FLS destructors exceeded {MAX_FLS_DESTRUCTOR_PASSES} passes"
                ));
            }
            pending.fls_pass += 1;
            pending.fls_processed.clear();
        }
        let exit_code = pending.exit_code.expect("thread entry completed");
        let exiting_id = unicorn.get_data().current_windows_thread_id;
        unicorn.get_data_mut().windows_objects.abandon(exiting_id);
        let (stack_base, stack_size) = {
            let thread = unicorn
                .get_data_mut()
                .windows_threads
                .get_mut(&pending.handle)
                .ok_or_else(|| format!("CreateThread handle {:#x} disappeared", pending.handle))?;
            thread.completed = true;
            thread.suspended = false;
            thread.exit_code = exit_code;
            thread.stack_mapped = false;
            (thread.stack_base, thread.stack_size)
        };
        unicorn
            .mem_unmap(stack_base, stack_size)
            .map_err(|error| format!("CreateThread stack unmap failed: {error}"))?;
        unicorn
            .mem_write(0x08, &pending.caller_teb_stack)
            .map_err(|error| format!("CreateThread TEB stack restore failed: {error}"))?;
        let thread = unicorn
            .get_data()
            .windows_threads
            .get(&pending.handle)
            .expect("thread remains recorded");
        let remove_closed = !thread.handle_open;
        restore_windows_thread_context(unicorn.get_data_mut(), &pending);
        if remove_closed {
            unicorn
                .get_data_mut()
                .windows_threads
                .remove(&pending.handle);
        }
        unicorn.get_data_mut().pending_windows_thread = None;
        unicorn
            .reg_write(RegisterX86::RSP, pending.continuation_rsp)
            .map_err(|error| format!("CreateThread final stack write failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::R11, pending.return_address)
            .map_err(|error| format!("CreateThread return target write failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::RAX, pending.completion_return)
            .map_err(|error| format!("CreateThread return value write failed: {error}"))?;
        if unicorn.get_data().scheduler_resume_active {
            unicorn.get_data_mut().scheduler_resume_active = false;
            unicorn.get_data_mut().scheduler_child_completed = true;
            unicorn
                .reg_write(RegisterX86::RIP, pending.return_address)
                .map_err(|error| {
                    format!("CreateThread completed scheduler target failed: {error}")
                })?;
            unicorn
                .emu_stop()
                .map_err(|error| format!("CreateThread scheduler stop failed: {error}"))?;
        } else {
            unicorn.get_data_mut().scheduler_parent_context = None;
        }
        Ok(())
    })();
    if let Err(error) = result {
        fail_windows_thread_callback(unicorn, error);
    }
}

fn dispatch_windows_thread(
    unicorn: &mut Unicorn<'_, GuestState>,
    handle: u64,
    return_address: u64,
    continuation_rsp: u64,
    completion_return: u64,
) -> Result<(), String> {
    if unicorn.get_data().pending_windows_thread.is_some() {
        return Err("nested CreateThread execution is unsupported".into());
    }
    let mut caller_context = unicorn
        .context_init()
        .map_err(|error| format!("CreateThread caller context save failed: {error}"))?;
    caller_context
        .reg_write(RegisterX86::RIP, return_address)
        .map_err(|error| format!("CreateThread caller context target failed: {error}"))?;
    caller_context
        .reg_write(RegisterX86::RSP, continuation_rsp)
        .map_err(|error| format!("CreateThread caller context stack failed: {error}"))?;
    caller_context
        .reg_write(RegisterX86::RAX, completion_return)
        .map_err(|error| format!("CreateThread caller context result failed: {error}"))?;
    let (id, start, parameter, stack_base, stack_size) = unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .filter(|thread| thread.handle_open)
        .map(|thread| {
            (
                thread.id,
                thread.start,
                thread.parameter,
                thread.stack_base,
                thread.stack_size,
            )
        })
        .ok_or_else(|| format!("CreateThread handle {handle:#x} is stale"))?;
    let caller_tls_values = unicorn.get_data().windows_tls_slots.clone();
    let caller_fls_values = unicorn
        .get_data()
        .windows_fls_slots
        .iter()
        .map(|(index, slot)| (*index, slot.value))
        .collect();
    let mut caller_teb_stack = [0u8; 16];
    unicorn
        .mem_read(0x08, &mut caller_teb_stack)
        .map_err(|error| format!("CreateThread TEB stack read failed: {error}"))?;
    let callback_rsp = ((stack_base + stack_size) - 0x108) | 8;
    let pending = PendingWindowsThread {
        handle,
        return_address,
        continuation_rsp,
        callback_return_rsp: callback_rsp + 8,
        caller_tls_values,
        caller_fls_values,
        caller_last_error: unicorn.get_data().windows_last_error,
        caller_crt_errno: unicorn.get_data().crt_errno,
        caller_thread_error_mode: unicorn.get_data().windows_thread_error_mode,
        caller_thread_id: unicorn.get_data().current_windows_thread_id,
        completion_return,
        caller_teb_stack,
        exit_code: None,
        fls_pass: 1,
        fls_processed: BTreeSet::new(),
    };
    unicorn.get_data_mut().pending_windows_thread = Some(pending);
    unicorn.get_data_mut().scheduler_parent_context = Some(caller_context);
    for value in unicorn.get_data_mut().windows_tls_slots.values_mut() {
        *value = 0;
    }
    for slot in unicorn.get_data_mut().windows_fls_slots.values_mut() {
        slot.value = 0;
    }
    unicorn.get_data_mut().windows_last_error = 0;
    unicorn.get_data_mut().crt_errno = 0;
    unicorn.get_data_mut().windows_thread_error_mode = 0;
    unicorn.get_data_mut().current_windows_thread_id = id;
    unicorn
        .mem_write(callback_rsp, &HOST_CREATE_THREAD_CONTINUE.to_le_bytes())
        .map_err(|error| format!("CreateThread continuation write failed: {error}"))?;
    let mut teb_stack = [0u8; 16];
    teb_stack[0..8].copy_from_slice(&(stack_base + stack_size).to_le_bytes());
    teb_stack[8..16].copy_from_slice(&stack_base.to_le_bytes());
    unicorn
        .mem_write(0x08, &teb_stack)
        .map_err(|error| format!("CreateThread TEB stack write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RSP, callback_rsp)
        .map_err(|error| format!("CreateThread callback stack write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::RCX, parameter)
        .map_err(|error| format!("CreateThread callback parameter write failed: {error}"))?;
    unicorn
        .reg_write(RegisterX86::R11, start)
        .map_err(|error| format!("CreateThread callback target write failed: {error}"))?;
    Ok(())
}

fn emulate_switch_to_thread(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("SwitchToThread stack read failed: {error}"))?;
        let return_address = read_vcomp_u64(unicorn, rsp)?;
        unicorn
            .reg_write(RegisterX86::RSP, rsp + 8)
            .map_err(|error| format!("SwitchToThread stack advance failed: {error}"))?;
        unicorn
            .reg_write(RegisterX86::RIP, return_address)
            .map_err(|error| format!("SwitchToThread return target failed: {error}"))?;
        let yielded = unicorn.get_data().pending_windows_thread.is_some()
            || unicorn.get_data().scheduler_ready_hint;
        unicorn
            .reg_write(RegisterX86::RAX, u64::from(yielded))
            .map_err(|error| format!("SwitchToThread return value failed: {error}"))?;
        unicorn.get_data_mut().scheduler_yield_reason = Some(SchedulerYieldReason::Voluntary);
        unicorn.get_data_mut().scheduler_resume_rip = return_address;
        unicorn
            .emu_stop()
            .map_err(|error| format!("SwitchToThread scheduler stop failed: {error}"))
    })();
    if let Err(error) = result {
        fail_windows_thread_callback(unicorn, error);
    }
}

fn emulate_create_thread(unicorn: &mut Unicorn<'_, GuestState>) {
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const STACK_SIZE_PARAM_IS_A_RESERVATION: u32 = 0x0001_0000;
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    let result = (|| -> Result<(), String> {
        let security_attributes = read_win64_import_argument(unicorn, 0)?;
        let stack_size = read_win64_import_argument(unicorn, 1)?;
        let start = read_win64_import_argument(unicorn, 2)?;
        let parameter = read_win64_import_argument(unicorn, 3)?;
        let flags = read_win64_import_argument(unicorn, 4)? as u32;
        let thread_id_output = read_win64_import_argument(unicorn, 5)?;
        if security_attributes != 0 {
            return Err("CreateThread security attributes are unsupported".into());
        }
        if stack_size > STACK_SIZE {
            return Err(format!(
                "CreateThread stack reservation {stack_size:#x} exceeds bounded guest stack {STACK_SIZE:#x}"
            ));
        }
        if flags & !(CREATE_SUSPENDED | STACK_SIZE_PARAM_IS_A_RESERVATION) != 0 {
            return Err(format!("CreateThread flags {flags:#x} are unsupported"));
        }
        if !image_executable_address(unicorn.get_data(), start) {
            return Err(format!(
                "CreateThread start routine {start:#x} is outside the executable image"
            ));
        }
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("CreateThread stack read failed: {error}"))?;
        let return_address = read_vcomp_u64(unicorn, rsp)
            .map_err(|error| format!("CreateThread return address read failed: {error}"))?;
        if flags & CREATE_SUSPENDED == 0
            && return_address != RETURN_ADDRESS
            && !image_executable_address(unicorn.get_data(), return_address)
        {
            return Err(format!(
                "CreateThread caller return {return_address:#x} is outside the executable image"
            ));
        }
        if thread_id_output != 0 {
            let output_end = thread_id_output
                .checked_add(3)
                .ok_or_else(|| "CreateThread thread-id output range overflows".to_string())?;
            let regions = unicorn
                .mem_regions()
                .map_err(|error| format!("CreateThread memory-map query failed: {error}"))?;
            let mut cursor = thread_id_output;
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
                            "CreateThread thread-id output {thread_id_output:#x}..={output_end:#x} is not fully writable"
                        )
                    })?;
                if region.end >= output_end {
                    break;
                }
                cursor = region
                    .end
                    .checked_add(1)
                    .ok_or_else(|| "CreateThread writable region overflows".to_string())?;
            }
        }
        if unicorn.get_data().windows_threads.len() >= MAX_WINDOWS_THREADS {
            unicorn.get_data_mut().windows_last_error = ERROR_NOT_ENOUGH_MEMORY;
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("CreateThread limit return failed: {error}"))?;
            return Ok(());
        }
        let id = unicorn.get_data().next_windows_thread_id;
        let handle = WINDOWS_THREAD_HANDLE_BASE
            .checked_add(u64::from(id) * 0x10)
            .ok_or_else(|| "CreateThread handle overflow".to_string())?;
        let next_id = id
            .checked_add(1)
            .ok_or_else(|| "CreateThread id space exhausted".to_string())?;
        let suspended = flags & CREATE_SUSPENDED != 0;
        let requested_stack = if stack_size == 0 {
            STACK_SIZE
        } else if flags & STACK_SIZE_PARAM_IS_A_RESERVATION != 0 {
            stack_size.max(PAGE_SIZE)
        } else {
            STACK_SIZE
        };
        let mapped_stack_size = requested_stack
            .checked_add(PAGE_SIZE - 1)
            .map(|size| size & !(PAGE_SIZE - 1))
            .ok_or_else(|| "CreateThread stack size overflow".to_string())?;
        let stack_slot = (0..MAX_WINDOWS_THREADS)
            .find(|slot| {
                let base = WINDOWS_THREAD_STACK_BASE + (*slot as u64) * WINDOWS_THREAD_STACK_STRIDE;
                !unicorn
                    .get_data()
                    .windows_threads
                    .values()
                    .any(|thread| thread.stack_base == base)
            })
            .ok_or_else(|| "CreateThread stack slots exhausted".to_string())?;
        let thread_stack_base =
            WINDOWS_THREAD_STACK_BASE + (stack_slot as u64) * WINDOWS_THREAD_STACK_STRIDE;
        unicorn
            .mem_map(
                thread_stack_base,
                mapped_stack_size,
                Prot::READ | Prot::WRITE,
            )
            .map_err(|error| format!("CreateThread stack map failed: {error}"))?;
        if thread_id_output != 0 {
            if let Err(error) = unicorn.mem_write(thread_id_output, &id.to_le_bytes()) {
                let _ = unicorn.mem_unmap(thread_stack_base, mapped_stack_size);
                return Err(format!(
                    "CreateThread thread-id output {thread_id_output:#x} is not writable: {error}"
                ));
            }
        }
        unicorn.get_data_mut().windows_threads.insert(
            handle,
            WindowsThread {
                id,
                start,
                parameter,
                suspended,
                completed: false,
                exit_code: 0,
                handle_open: true,
                stack_base: thread_stack_base,
                stack_size: mapped_stack_size,
                stack_mapped: true,
            },
        );
        unicorn.get_data_mut().next_windows_thread_id = next_id;
        if suspended {
            unicorn
                .reg_write(RegisterX86::RAX, handle)
                .map_err(|error| format!("CreateThread suspended return failed: {error}"))?;
            unicorn
                .reg_write(RegisterX86::RSP, rsp + 8)
                .map_err(|error| format!("CreateThread suspended stack advance failed: {error}"))?;
            unicorn
                .reg_write(RegisterX86::R11, return_address)
                .map_err(|error| format!("CreateThread suspended return target failed: {error}"))?;
            return Ok(());
        }
        dispatch_windows_thread(unicorn, handle, return_address, rsp + 8, handle)
    })();
    if let Err(error) = result {
        unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
        fail_windows_thread_callback(unicorn, error);
    }
}

fn emulate_resume_thread(unicorn: &mut Unicorn<'_, GuestState>) {
    const THREAD_ERROR: u64 = u32::MAX as u64;
    let result = (|| -> Result<(), String> {
        let handle = read_win64_import_argument(unicorn, 0)?;
        match unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .filter(|thread| thread.handle_open)
        {
            Some(thread) if thread.suspended && !thread.completed => {}
            Some(_) => {
                unicorn.reg_write(RegisterX86::RAX, 0).map_err(|error| {
                    format!("ResumeThread already-running return failed: {error}")
                })?;
                let rsp = unicorn
                    .reg_read(RegisterX86::RSP)
                    .map_err(|error| format!("ResumeThread stack read failed: {error}"))?;
                let return_address = read_vcomp_u64(unicorn, rsp)?;
                unicorn
                    .reg_write(RegisterX86::RSP, rsp + 8)
                    .map_err(|error| format!("ResumeThread stack advance failed: {error}"))?;
                unicorn
                    .reg_write(RegisterX86::R11, return_address)
                    .map_err(|error| format!("ResumeThread return target failed: {error}"))?;
                return Ok(());
            }
            None => {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
                unicorn
                    .reg_write(RegisterX86::RAX, THREAD_ERROR)
                    .map_err(|error| {
                        format!("ResumeThread invalid-handle return failed: {error}")
                    })?;
                let rsp = unicorn
                    .reg_read(RegisterX86::RSP)
                    .map_err(|error| format!("ResumeThread stack read failed: {error}"))?;
                let return_address = read_vcomp_u64(unicorn, rsp)?;
                unicorn
                    .reg_write(RegisterX86::RSP, rsp + 8)
                    .map_err(|error| format!("ResumeThread stack advance failed: {error}"))?;
                unicorn
                    .reg_write(RegisterX86::R11, return_address)
                    .map_err(|error| format!("ResumeThread return target failed: {error}"))?;
                return Ok(());
            }
        }
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("ResumeThread stack read failed: {error}"))?;
        let return_address = read_vcomp_u64(unicorn, rsp)?;
        dispatch_windows_thread(unicorn, handle, return_address, rsp + 8, 1)
    })();
    if let Err(error) = result {
        fail_windows_thread_callback(unicorn, error);
    }
}

fn emulate_windows_thread_lifecycle(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    const WAIT_OBJECT_0: u64 = 0;
    const WAIT_TIMEOUT: u64 = 258;
    const WAIT_FAILED: u64 = u32::MAX as u64;
    let handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
    if unicorn
        .get_data()
        .windows_objects
        .handles
        .contains_key(&handle)
        && matches!(
            operation,
            LegacyWin64Import::WaitForSingleObject
                | LegacyWin64Import::WaitForSingleObjectEx
                | LegacyWin64Import::CloseHandle
        )
    {
        emulate_windows_kernel_object(unicorn, operation);
        return;
    }
    let returned = match operation {
        LegacyWin64Import::WaitForSingleObject | LegacyWin64Import::WaitForSingleObjectEx => {
            let handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
            let timeout = read_win64_import_argument(unicorn, 1).unwrap_or_default() as u32;
            if operation == LegacyWin64Import::WaitForSingleObjectEx {
                let _ = read_win64_import_argument(unicorn, 2);
            }
            match unicorn
                .get_data()
                .windows_threads
                .get(&handle)
                .filter(|thread| thread.handle_open)
            {
                Some(thread) if thread.completed => WAIT_OBJECT_0,
                Some(_) if timeout == 0 => WAIT_TIMEOUT,
                Some(_) => {
                    unicorn.get_data_mut().callback_error = Some(format!(
                        "blocking wait on incomplete guest thread {handle:#x} is unsupported"
                    ));
                    let _ = unicorn.emu_stop();
                    WAIT_FAILED
                }
                None => {
                    unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
                    WAIT_FAILED
                }
            }
        }
        LegacyWin64Import::CloseHandle => {
            let handle = read_win64_import_argument(unicorn, 0).unwrap_or_default();
            match unicorn
                .get_data()
                .windows_threads
                .get(&handle)
                .filter(|thread| thread.handle_open)
            {
                Some(thread) if thread.completed => {
                    unicorn.get_data_mut().windows_threads.remove(&handle);
                    1
                }
                Some(_) => {
                    unicorn
                        .get_data_mut()
                        .windows_threads
                        .get_mut(&handle)
                        .expect("observed thread")
                        .handle_open = false;
                    1
                }
                None => {
                    unicorn.get_data_mut().windows_last_error = ERROR_INVALID_HANDLE;
                    0
                }
            }
        }
        LegacyWin64Import::SetThreadStackGuarantee => {
            let output = read_win64_import_argument(unicorn, 0).unwrap_or_default();
            let mut bytes = [0u8; 4];
            if output == 0 || unicorn.mem_read(output, &mut bytes).is_err() {
                unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                0
            } else {
                let requested = u32::from_le_bytes(bytes);
                if u64::from(requested) > STACK_SIZE {
                    unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                    0
                } else if unicorn.mem_write(output, &0u32.to_le_bytes()).is_err() {
                    unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
                    0
                } else {
                    1
                }
            }
        }
        _ => 0,
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned);
}

fn emulate_query_performance_counter(unicorn: &mut Unicorn<'_, GuestState>) {
    let origin = *unicorn
        .get_data_mut()
        .performance_counter_origin
        .get_or_insert_with(std::time::Instant::now);
    match i64::try_from(origin.elapsed().as_nanos() / 100) {
        Ok(value) => {
            emulate_query_performance_value(unicorn, "QueryPerformanceCounter", value as u64)
        }
        Err(_) => {
            unicorn.get_data_mut().callback_error = Some("performance counter overflow".into());
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    }
}

fn emulate_query_performance_frequency(unicorn: &mut Unicorn<'_, GuestState>) {
    // Monotonic host elapsed time is expressed in 100 ns ticks. Its epoch is
    // private to the guest engine and independent of wall-clock corrections.
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
        if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err(format!("{function} output {output:#x} is not writable"));
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

const WINDOWS_WSADATA_X64_SIZE: u64 = 408;
const WINDOWS_WSA_VERSION_2_2: u16 = 0x0202;
const WINDOWS_WSAEFAULT: u32 = 10_014;
const WINDOWS_WSAEPROCLIM: u32 = 10_067;
const WINDOWS_WSAVERNOTSUPPORTED: u32 = 10_092;
const WINDOWS_WSANOTINITIALISED: u32 = 10_093;
const MAX_WINDOWS_SOCKET_STARTUPS: u32 = 1_024;

fn emulate_wsa_startup(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u32, String> {
        let requested = read_win64_import_argument(unicorn, 0)? as u16;
        let output = read_win64_import_argument(unicorn, 1)?;
        if !matches!(requested, 0x0001 | 0x0101 | 0x0002 | 0x0102 | 0x0202) {
            return Ok(WINDOWS_WSAVERNOTSUPPORTED);
        }
        if output == 0
            || !guest_range_has_permission(unicorn, output, WINDOWS_WSADATA_X64_SIZE, Prot::WRITE)?
        {
            return Ok(WINDOWS_WSAEFAULT);
        }
        let count = unicorn.get_data().windows_socket_startups;
        if count >= MAX_WINDOWS_SOCKET_STARTUPS {
            return Ok(WINDOWS_WSAEPROCLIM);
        }
        let next_count = count + 1;
        let mut data = [0u8; WINDOWS_WSADATA_X64_SIZE as usize];
        data[0..2].copy_from_slice(&requested.to_le_bytes());
        data[2..4].copy_from_slice(&WINDOWS_WSA_VERSION_2_2.to_le_bytes());
        let description = b"AEXCompat deterministic Winsock 2.2 guest";
        data[16..16 + description.len()].copy_from_slice(description);
        let status = b"Running";
        data[273..273 + status.len()].copy_from_slice(status);
        unicorn
            .mem_write(output, &data)
            .map_err(|error| format!("write WSAStartup WSADATA: {error}"))?;
        unicorn.get_data_mut().windows_socket_startups = next_count;
        Ok(0)
    })();
    match result {
        Ok(code) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(code));
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(WINDOWS_WSAEFAULT));
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_wsa_cleanup(unicorn: &mut Unicorn<'_, GuestState>) {
    let count = unicorn.get_data().windows_socket_startups;
    let code = if count == 0 {
        unicorn.get_data_mut().windows_last_error = WINDOWS_WSANOTINITIALISED;
        u32::MAX
    } else {
        unicorn.get_data_mut().windows_socket_startups = count - 1;
        0
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(code));
}

fn emulate_rtl_pc_to_file_header(unicorn: &mut Unicorn<'_, GuestState>) {
    let pc = read_win64_import_argument(unicorn, 0).unwrap_or_default();
    let output = read_win64_import_argument(unicorn, 1).unwrap_or_default();
    if output != 0 && !guest_range_has_permission(unicorn, output, 8, Prot::WRITE).unwrap_or(false)
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    let module = guest_module_from_address(unicorn.get_data(), pc)
        .or_else(|| (pc == WINDOWS_KERNEL32_MODULE_TOKEN).then_some(WINDOWS_KERNEL32_MODULE_TOKEN))
        .unwrap_or_default();
    if output != 0 && unicorn.mem_write(output, &module.to_le_bytes()).is_err() {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, module);
}

fn emulate_raise_exception(unicorn: &mut Unicorn<'_, GuestState>) {
    const EXCEPTION_NONCONTINUABLE: u32 = 0x1;
    const EXCEPTION_MAXIMUM_PARAMETERS: u32 = 15;
    let result = (|| -> Result<String, String> {
        let code = read_win64_import_argument(unicorn, 0)? as u32;
        let flags = read_win64_import_argument(unicorn, 1)? as u32;
        let count = read_win64_import_argument(unicorn, 2)? as u32;
        let arguments = read_win64_import_argument(unicorn, 3)?;
        if flags & !EXCEPTION_NONCONTINUABLE != 0 {
            return Err(format!("RaiseException flags {flags:#x} are invalid"));
        }
        if count > EXCEPTION_MAXIMUM_PARAMETERS {
            return Err(format!(
                "RaiseException parameter count {count} exceeds {EXCEPTION_MAXIMUM_PARAMETERS}"
            ));
        }
        let byte_count = u64::from(count) * 8;
        if count != 0
            && (arguments == 0
                || !guest_range_has_permission(unicorn, arguments, byte_count, Prot::READ)?)
        {
            return Err(format!(
                "RaiseException parameter array {arguments:#x} is not fully readable for {count} entries"
            ));
        }
        let mut values = Vec::with_capacity(count as usize);
        for index in 0..count {
            let address = arguments + u64::from(index) * 8;
            let bytes = unicorn.mem_read_as_vec(address, 8).map_err(|error| {
                format!("RaiseException parameter {index} read failed: {error}")
            })?;
            values.push(u64::from_le_bytes(bytes.try_into().map_err(|_| {
                format!("RaiseException parameter {index} has the wrong size")
            })?));
        }
        let values = values
            .iter()
            .map(|value| format!("{value:#x}"))
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            "unhandled guest RaiseException code={code:#x} flags={flags:#x} parameters=[{values}]; x64 SEH dispatch is not modeled"
        ))
    })();
    let error = result.unwrap_or_else(|error| error);
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(error);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    let _ = unicorn.emu_stop();
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

// The guest begins with an empty application registry. No host registry,
// installation records or activation data are synthesized. Mutating APIs and
// special performance pseudo-keys remain explicit unsupported imports/paths.
fn guest_registry_predefined_key(key: u64) -> bool {
    matches!(
        key,
        0xffff_ffff_8000_0000..=0xffff_ffff_8000_0003 | 0xffff_ffff_8000_0005
    )
}

fn emulate_reg_open_key_ex_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let key = read_win64_import_argument(unicorn, 0)?;
        let subkey = read_win64_import_argument(unicorn, 1)?;
        let options = read_win64_import_argument(unicorn, 2)? as u32;
        let access = read_win64_import_argument(unicorn, 3)? as u32;
        let output = read_win64_import_argument(unicorn, 4)?;
        if !guest_registry_predefined_key(key) {
            return Err(format!(
                "RegOpenKeyExA unsupported or foreign registry handle {key:#x}"
            ));
        }
        if options != 0 {
            return Err(format!("RegOpenKeyExA unsupported options {options:#x}"));
        }
        if access & 0x300 == 0x300 {
            return Ok(87);
        }
        if output == 0 {
            return Ok(87);
        }
        if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err("RegOpenKeyExA result is not writable".into());
        }
        let name = if subkey == 0 {
            Vec::new()
        } else {
            read_crt_stdio_c_string(unicorn, subkey, 32768, "RegOpenKeyExA subkey")?
        };
        let (status, handle) = if name.is_empty() { (0, key) } else { (2, 0) };
        unicorn
            .mem_write(output, &u64::to_le_bytes(handle))
            .map_err(|error| format!("RegOpenKeyExA output: {error}"))?;
        Ok(status)
    })();
    finish_registry_import(unicorn, result);
}

fn emulate_reg_close_key(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = read_win64_import_argument(unicorn, 0).and_then(|key| {
        if guest_registry_predefined_key(key) {
            Ok(0)
        } else {
            // RegCloseKey reports an LSTATUS error for a handle it cannot
            // close. No registry object is created or successfully closed.
            Ok(u64::from(ERROR_INVALID_HANDLE))
        }
    });
    finish_registry_import(unicorn, result);
}

fn finish_registry_import(unicorn: &mut Unicorn<'_, GuestState>, result: Result<u64, String>) {
    match result {
        Ok(status) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, status);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

// Win64 absolute SECURITY_DESCRIPTOR: four header bytes, four padding bytes,
// followed by owner/group/SACL/DACL pointers. This initializes guest data only;
// it does not change host permissions or grant access to a kernel object.
fn emulate_initialize_security_descriptor(unicorn: &mut Unicorn<'_, GuestState>) {
    let output = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let revision = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default() as u32;
    let error = if revision != 1 {
        Some(1305u32)
    }
    // ERROR_UNKNOWN_REVISION
    else if output == 0
        || !guest_range_has_permission(unicorn, output, 40, Prot::WRITE).unwrap_or(false)
    {
        Some(ERROR_INVALID_PARAMETER)
    } else {
        let mut descriptor = [0u8; 40];
        descriptor[0] = 1;
        unicorn
            .mem_write(output, &descriptor)
            .err()
            .map(|_| ERROR_INVALID_PARAMETER)
    };
    if let Some(error) = error {
        unicorn.get_data_mut().windows_last_error = error;
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, 1);
    }
}

fn emulate_set_security_descriptor_dacl(unicorn: &mut Unicorn<'_, GuestState>) {
    let output = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let present = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default() as u32 != 0;
    let dacl = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    let defaulted = unicorn.reg_read(RegisterX86::R9).unwrap_or_default() as u32 != 0;
    let result = (|| -> Result<(), u32> {
        if output == 0
            || !guest_range_has_permission(unicorn, output, 40, Prot::READ | Prot::WRITE)
                .unwrap_or(false)
        {
            return Err(ERROR_INVALID_PARAMETER);
        }
        let mut descriptor = [0u8; 40];
        unicorn
            .mem_read(output, &mut descriptor)
            .map_err(|_| ERROR_INVALID_PARAMETER)?;
        if descriptor[0] != 1 {
            return Err(1305);
        }
        let mut control = u16::from_le_bytes([descriptor[2], descriptor[3]]);
        if control & 0x8000 != 0 {
            return Err(1338);
        } // self-relative descriptor is not accepted
        if present {
            control = (control | 0x4) & !0x8;
            if defaulted {
                control |= 0x8;
            }
            // Preserve a reference, including NULL; this setter neither copies
            // ACL bytes nor makes an access decision for any object.
            descriptor[32..40].copy_from_slice(&dacl.to_le_bytes());
        } else {
            control &= !0x4; // pointer and defaulted arguments are ignored
        }
        descriptor[2..4].copy_from_slice(&control.to_le_bytes());
        unicorn
            .mem_write(output, &descriptor)
            .map_err(|_| ERROR_INVALID_PARAMETER)
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 1);
        }
        Err(error) => {
            unicorn.get_data_mut().windows_last_error = error;
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    }
}

fn windows_filetime(time: std::time::SystemTime) -> Result<u64, String> {
    const UNIX_EPOCH_FILETIME: u128 = 116_444_736_000_000_000;
    let ticks = match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => UNIX_EPOCH_FILETIME.checked_add(duration.as_nanos() / 100),
        Err(error) => UNIX_EPOCH_FILETIME.checked_sub(error.duration().as_nanos().div_ceil(100)),
    }
    .ok_or("wall clock is outside FILETIME range")?;
    u64::try_from(ticks).map_err(|_| "wall clock exceeds FILETIME range".into())
}

// UCRT __time64_t supports UTC dates from 1970 through the end of 3000.
fn crt_time64_seconds(time: std::time::SystemTime) -> i64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
        .filter(|seconds| *seconds <= 32_535_215_999)
        .map_or(-1, |seconds| seconds as i64)
}

fn emulate_crt_time64(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<i64, String> {
        let output = read_win64_import_argument(unicorn, 0)?;
        if output != 0 && !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err(format!("_time64 output {output:#x} is not writable"));
        }
        let seconds = crt_time64_seconds(std::time::SystemTime::now());
        if output != 0 {
            unicorn
                .mem_write(output, &seconds.to_le_bytes())
                .map_err(|error| format!("_time64 output write failed: {error}"))?;
        }
        Ok(seconds)
    })();
    match result {
        Ok(seconds) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, seconds as u64);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_load_library_ex_a(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        let file = read_win64_import_argument(unicorn, 1)?;
        let flags = read_win64_import_argument(unicorn, 2)? as u32;
        if pointer == 0 || file != 0 {
            unicorn.get_data_mut().windows_last_error = ERROR_INVALID_PARAMETER;
            return Ok(0);
        }
        // Supported executable searches: default or System32 only. Other valid
        // resource/search policies need their own semantics, not a false miss.
        if flags != 0 && flags != 0x800 {
            return Err(format!("unsupported LoadLibraryExA flags {flags:#x}"));
        }
        let path = read_crt_stdio_c_string(unicorn, pointer, 1025, "LoadLibraryExA path")?;
        if !path.is_ascii() {
            return Err("unsupported LoadLibraryExA path encoding".into());
        }
        let mut path = String::from_utf8(path)
            .unwrap()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if path.is_empty() {
            unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
            return Ok(0);
        }
        let basename = path.rsplit('/').next().unwrap();
        if !basename.contains('.') {
            path.push_str(".dll");
        } else if path.ends_with('.') {
            path.pop();
        }
        // An existing basename wins before directory search policy is applied.
        if let Some(library) = guest_library_by_name(unicorn.get_data(), &path) {
            if !library.initialized {
                return Err("LoadLibraryExA DLL initialization incomplete".into());
            }
            return Ok(library.base);
        }
        if flags == 0x800 && !path.contains('/') && !path.contains(':') {
            path = format!("c:/windows/system32/{path}");
        }
        if let Some(library) = guest_library_by_name(unicorn.get_data(), &path) {
            if !library.initialized {
                return Err("LoadLibraryExA DLL initialization incomplete".into());
            }
            // Explicit dependency images are pinned for the engine lifetime.
            return Ok(library.base);
        }
        if path == "kernel32.dll" || path == "c:/windows/system32/kernel32.dll" {
            return Ok(WINDOWS_KERNEL32_MODULE_TOKEN);
        }
        unicorn.get_data_mut().windows_last_error = ERROR_MOD_NOT_FOUND;
        Ok(0)
    })();
    finish_guest_stdio(unicorn, result);
}
