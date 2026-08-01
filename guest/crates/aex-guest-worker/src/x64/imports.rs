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
    CxxThrowException,
    CosF,
    ExpF,
    FloorF,
    PowF,
    Pow,
    SinF,
    OmpGetMaxThreads,
    VcompFork,
    VcompForDynamicInit,
    VcompForDynamicNext,
    VcompForStaticSimpleInit,
    VcompNoOp,
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
    match classify_gpu_import_library(library) {
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
    let legacy = match symbol {
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
        name if name.starts_with("_vcomp_") => return Win64ImportDispatch::UnsupportedVcomp,
        name if msvc_udt_by_value_return_import(name) => {
            return Win64ImportDispatch::UnsupportedLegacyImport;
        }
        _ => return Win64ImportDispatch::UnsupportedLegacyImport,
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
