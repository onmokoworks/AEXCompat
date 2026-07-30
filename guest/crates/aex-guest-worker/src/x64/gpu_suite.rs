const PF_ERR_NONE: u64 = 0;
const PF_ERR_OUT_OF_MEMORY: u64 = 4;
const PF_ERR_BAD_CALLBACK_PARAM: u64 = 516;
const PF_GPU_FRAMEWORK_OPENCL: i32 = 1;
const PF_PIXEL_FORMAT_GPU_BGRA128: i32 = 0x4144_4340;
const PF_GPU_DEVICE_INFO_SIZE: usize = 56;
const GPU_MAX_ALLOCATIONS: usize = 256;
const GPU_MAX_LIVE_BYTES: usize = 256 * 1024 * 1024;
const GPU_MAX_WORLD_DIMENSION: i32 = 4096;
const GPU_HOST_MEMORY_BASE: u64 = 0x0000_0020_0000_0000;
const GPU_HOST_MEMORY_END: u64 = GPU_HOST_MEMORY_BASE + 0x4000_0000;
const GPU_WORLD_DESCRIPTOR_BASE: u64 = 0x0000_0021_0000_0000;
const GPU_WORLD_DESCRIPTOR_END: u64 = GPU_WORLD_DESCRIPTOR_BASE + 0x0100_0000;
const GPU_FILL_CHUNK_BYTES: usize = 1024 * 1024;
const GPU_MAX_EXCLUSIVE_DEPTH: u32 = 64;

#[derive(Clone, Debug)]
struct GpuHostAllocation {
    device_index: u32,
    bytes: usize,
    mapped_bytes: u64,
}

#[derive(Clone, Debug)]
struct GpuWorld {
    device_index: u32,
    buffer_token: u64,
    bytes: usize,
    mapped_bytes: u64,
}

#[derive(Clone, Debug)]
struct BorrowedGpuWorld {
    device_index: u32,
    buffer_token: u64,
    bytes: usize,
}

#[derive(Clone, Debug)]
struct GpuTransportWorld {
    world: u64,
    host_data: u64,
    device_token: u64,
    width: i32,
    height: i32,
    host_rowbytes: i32,
    device_bytes: usize,
}

#[derive(Clone, Debug)]
struct GpuRenderTransport {
    input: GpuTransportWorld,
    output: GpuTransportWorld,
    previous_render_pixel_format: i32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuSuiteEvidence {
    pub(crate) allocations_created: u64,
    pub(crate) allocations_freed: u64,
    pub(crate) upload_bytes: u64,
    pub(crate) download_bytes: u64,
    pub(crate) worlds_created: u64,
    pub(crate) worlds_disposed: u64,
    pub(crate) invalid_operations: u64,
    pub(crate) exclusive_access_depth: u32,
    pub(crate) live_host_allocations: usize,
    pub(crate) live_gpu_worlds: usize,
    pub(crate) live_device_allocations: usize,
    pub(crate) live_bytes: usize,
    pub(crate) transport_active: bool,
    pub(crate) last_error: Option<String>,
}

#[derive(Default)]
struct GpuSuiteState {
    host_allocations: HashMap<u64, GpuHostAllocation>,
    worlds: HashMap<u64, GpuWorld>,
    borrowed_worlds: HashMap<u64, BorrowedGpuWorld>,
    transport: Option<GpuRenderTransport>,
    allocations_created: u64,
    allocations_freed: u64,
    upload_bytes: u64,
    download_bytes: u64,
    worlds_created: u64,
    worlds_disposed: u64,
    invalid_operations: u64,
    exclusive_access_depth: u32,
    last_error: Option<String>,
}

impl GpuSuiteState {
    fn evidence(&self, runtime: &GpuRuntime) -> GpuSuiteEvidence {
        GpuSuiteEvidence {
            allocations_created: self.allocations_created,
            allocations_freed: self.allocations_freed,
            upload_bytes: self.upload_bytes,
            download_bytes: self.download_bytes,
            worlds_created: self.worlds_created,
            worlds_disposed: self.worlds_disposed,
            invalid_operations: self.invalid_operations,
            exclusive_access_depth: self.exclusive_access_depth,
            live_host_allocations: self.host_allocations.len(),
            live_gpu_worlds: self.worlds.len(),
            live_device_allocations: runtime.live_buffer_count(),
            live_bytes: self
                .host_allocations
                .values()
                .map(|allocation| allocation.bytes)
                .sum::<usize>()
                .saturating_add(runtime.live_buffer_bytes()),
            transport_active: self.transport.is_some(),
            last_error: self.last_error.clone(),
        }
    }

    fn mapped_regions(&self) -> Vec<(u64, u64)> {
        self.host_allocations
            .iter()
            .map(|(address, allocation)| (*address, allocation.mapped_bytes))
            .chain(
                self.worlds
                    .iter()
                    .map(|(address, world)| (*address, world.mapped_bytes)),
            )
            .collect()
    }

    fn clear_for_drop(&mut self) {
        self.host_allocations.clear();
        self.worlds.clear();
        self.borrowed_worlds.clear();
        self.transport = None;
        self.exclusive_access_depth = 0;
    }
}

#[derive(Debug)]
struct GpuCallbackFailure {
    code: u64,
    message: String,
}

impl GpuCallbackFailure {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            code: PF_ERR_BAD_CALLBACK_PARAM,
            message: message.into(),
        }
    }

    fn out_of_memory(message: impl Into<String>) -> Self {
        Self {
            code: PF_ERR_OUT_OF_MEMORY,
            message: message.into(),
        }
    }
}

fn finish_gpu_callback(
    unicorn: &mut Unicorn<'_, GuestState>,
    result: Result<(), GpuCallbackFailure>,
) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, PF_ERR_NONE);
        }
        Err(failure) => {
            let suite = &mut unicorn.get_data_mut().gpu_suite;
            if failure.code == PF_ERR_BAD_CALLBACK_PARAM {
                suite.invalid_operations = suite.invalid_operations.saturating_add(1);
            }
            suite.last_error = Some(failure.message);
            let _ = unicorn.reg_write(RegisterX86::RAX, failure.code);
        }
    }
}

fn gpu_active_device(unicorn: &Unicorn<'_, GuestState>) -> Result<u32, GpuCallbackFailure> {
    unicorn
        .get_data()
        .gpu_runtime
        .device_index()
        .ok_or_else(|| GpuCallbackFailure::bad("GPU runtime is not active"))
}

fn validate_gpu_device(
    unicorn: &Unicorn<'_, GuestState>,
    requested: u32,
) -> Result<(), GpuCallbackFailure> {
    let active = gpu_active_device(unicorn)?;
    if active != requested {
        return Err(GpuCallbackFailure::bad(format!(
            "GPU device index {requested} does not match active device {active}"
        )));
    }
    Ok(())
}

fn gpu_register(
    unicorn: &Unicorn<'_, GuestState>,
    register: RegisterX86,
    label: &str,
) -> Result<u64, GpuCallbackFailure> {
    unicorn
        .reg_read(register)
        .map_err(|error| GpuCallbackFailure::bad(format!("{label}: {error}")))
}

fn gpu_stack_arg(
    unicorn: &Unicorn<'_, GuestState>,
    offset: u64,
    label: &str,
) -> Result<u64, GpuCallbackFailure> {
    aegp_stack_arg(unicorn, offset)
        .map_err(|error| GpuCallbackFailure::bad(format!("{label}: {error}")))
}

fn write_gpu_output(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    bytes: &[u8],
    label: &str,
) -> Result<(), GpuCallbackFailure> {
    if address == 0 {
        return Err(GpuCallbackFailure::bad(format!(
            "{label} output pointer is null"
        )));
    }
    unicorn
        .mem_write(address, bytes)
        .map_err(|error| GpuCallbackFailure::bad(format!("{label}: {error}")))
}

fn gpu_allocation_capacity(state: &GuestState, requested: usize) -> Result<(), GpuCallbackFailure> {
    let allocation_count =
        state.gpu_runtime.live_buffer_count() + state.gpu_suite.host_allocations.len();
    let host_bytes = state
        .gpu_suite
        .host_allocations
        .values()
        .map(|allocation| allocation.bytes)
        .sum::<usize>();
    if allocation_count >= GPU_MAX_ALLOCATIONS {
        return Err(GpuCallbackFailure::bad(format!(
            "GPU allocation count exceeds {GPU_MAX_ALLOCATIONS}"
        )));
    }
    if requested == 0 || requested > GPU_MAX_LIVE_BYTES {
        return Err(GpuCallbackFailure::bad(format!(
            "GPU allocation size {requested} is outside 1..={GPU_MAX_LIVE_BYTES}"
        )));
    }
    if host_bytes
        .checked_add(state.gpu_runtime.live_buffer_bytes())
        .and_then(|bytes| bytes.checked_add(requested))
        .is_none_or(|bytes| bytes > GPU_MAX_LIVE_BYTES)
    {
        return Err(GpuCallbackFailure::bad(format!(
            "GPU live allocation bytes exceed {GPU_MAX_LIVE_BYTES}"
        )));
    }
    Ok(())
}

fn aligned_gpu_bytes(bytes: usize) -> Result<u64, GpuCallbackFailure> {
    let bytes = u64::try_from(bytes)
        .map_err(|_| GpuCallbackFailure::bad("GPU allocation size does not fit u64"))?;
    bytes
        .checked_add(PAGE_SIZE - 1)
        .map(|value| value & !(PAGE_SIZE - 1))
        .filter(|value| *value != 0)
        .ok_or_else(|| GpuCallbackFailure::bad("GPU mapped size overflow"))
}

fn find_gpu_host_region(
    suite: &GpuSuiteState,
    mapped_bytes: u64,
) -> Result<u64, GpuCallbackFailure> {
    let mut occupied = suite
        .host_allocations
        .iter()
        .map(|(address, allocation)| (*address, *address + allocation.mapped_bytes))
        .collect::<Vec<_>>();
    occupied.sort_unstable();
    find_gpu_region(
        GPU_HOST_MEMORY_BASE,
        GPU_HOST_MEMORY_END,
        mapped_bytes,
        occupied,
        "GPU host memory",
    )
}

fn find_gpu_world_region(suite: &GpuSuiteState) -> Result<u64, GpuCallbackFailure> {
    let mut occupied = suite
        .worlds
        .keys()
        .map(|address| (*address, *address + PAGE_SIZE))
        .collect::<Vec<_>>();
    occupied.sort_unstable();
    find_gpu_region(
        GPU_WORLD_DESCRIPTOR_BASE,
        GPU_WORLD_DESCRIPTOR_END,
        PAGE_SIZE,
        occupied,
        "GPU world descriptor",
    )
}

fn find_gpu_region(
    start: u64,
    end: u64,
    bytes: u64,
    occupied: Vec<(u64, u64)>,
    label: &str,
) -> Result<u64, GpuCallbackFailure> {
    let mut candidate = start;
    for (occupied_start, occupied_end) in occupied {
        if candidate
            .checked_add(bytes)
            .is_some_and(|candidate_end| candidate_end <= occupied_start)
        {
            return Ok(candidate);
        }
        candidate = candidate.max(occupied_end);
    }
    if candidate
        .checked_add(bytes)
        .is_some_and(|candidate_end| candidate_end <= end)
    {
        Ok(candidate)
    } else {
        Err(GpuCallbackFailure::out_of_memory(format!(
            "{label} address space exhausted"
        )))
    }
}

fn emulate_gpu_get_device_count(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let output = gpu_register(unicorn, RegisterX86::RDX, "GetDeviceCount output")?;
        let count = u32::from(unicorn.get_data().gpu_runtime.is_active());
        write_gpu_output(unicorn, output, &count.to_le_bytes(), "GetDeviceCount")
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_get_device_info(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "GetDeviceInfo device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let output = gpu_register(unicorn, RegisterX86::R8, "GetDeviceInfo output")?;
        let tokens = unicorn
            .get_data()
            .gpu_runtime
            .device_tokens()
            .ok_or_else(|| GpuCallbackFailure::bad("GPU device tokens are unavailable"))?;
        let mut info = [0u8; PF_GPU_DEVICE_INFO_SIZE];
        info[0..4].copy_from_slice(&PF_GPU_FRAMEWORK_OPENCL.to_le_bytes());
        info[4] = 1;
        for (offset, token) in [
            (8, tokens.platform),
            (16, tokens.device),
            (24, tokens.context),
            (32, tokens.queue),
        ] {
            info[offset..offset + 8].copy_from_slice(&token.to_le_bytes());
        }
        write_gpu_output(unicorn, output, &info, "GetDeviceInfo")
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_acquire_exclusive(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "AcquireExclusive device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let suite = &mut unicorn.get_data_mut().gpu_suite;
        if suite.exclusive_access_depth >= GPU_MAX_EXCLUSIVE_DEPTH {
            return Err(GpuCallbackFailure::bad(
                "GPU exclusive access depth exceeds its bound",
            ));
        }
        suite.exclusive_access_depth += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_release_exclusive(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "ReleaseExclusive device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let suite = &mut unicorn.get_data_mut().gpu_suite;
        if suite.exclusive_access_depth == 0 {
            return Err(GpuCallbackFailure::bad(
                "GPU exclusive access release is unbalanced",
            ));
        }
        suite.exclusive_access_depth -= 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_allocate_device(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index = gpu_register(
            unicorn,
            RegisterX86::RDX,
            "AllocateDeviceMemory device index",
        )? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let bytes = usize::try_from(gpu_register(
            unicorn,
            RegisterX86::R8,
            "AllocateDeviceMemory size",
        )?)
        .map_err(|_| GpuCallbackFailure::bad("device allocation size does not fit usize"))?;
        gpu_allocation_capacity(unicorn.get_data(), bytes)?;
        let output = gpu_register(unicorn, RegisterX86::R9, "AllocateDeviceMemory output")?;
        write_gpu_output(unicorn, output, &0u64.to_le_bytes(), "AllocateDeviceMemory")?;
        let token = unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(device_index, bytes, BufferAccess::ReadWrite)
            .map_err(GpuCallbackFailure::out_of_memory)?;
        if let Err(failure) = write_gpu_output(
            unicorn,
            output,
            &token.to_le_bytes(),
            "AllocateDeviceMemory",
        ) {
            let _ = unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, token);
            return Err(failure);
        }
        unicorn.get_data_mut().gpu_suite.allocations_created += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_free_device(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "FreeDeviceMemory device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let token = gpu_register(unicorn, RegisterX86::R8, "FreeDeviceMemory token")?;
        unicorn
            .get_data_mut()
            .gpu_runtime
            .free_device(device_index, token)
            .map_err(GpuCallbackFailure::bad)?;
        unicorn.get_data_mut().gpu_suite.allocations_freed += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_purge_device(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_gpu_purge(unicorn, "PurgeDeviceMemory");
}

fn emulate_gpu_purge_host(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_gpu_purge(unicorn, "PurgeHostMemory");
}

fn emulate_gpu_purge(unicorn: &mut Unicorn<'_, GuestState>, label: &str) {
    let result = (|| {
        let device_index = gpu_register(unicorn, RegisterX86::RDX, label)? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let output = gpu_register(unicorn, RegisterX86::R9, label)?;
        if output != 0 {
            write_gpu_output(unicorn, output, &0u64.to_le_bytes(), label)?;
        }
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_allocate_host(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "AllocateHostMemory device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let bytes = usize::try_from(gpu_register(
            unicorn,
            RegisterX86::R8,
            "AllocateHostMemory size",
        )?)
        .map_err(|_| GpuCallbackFailure::bad("host allocation size does not fit usize"))?;
        gpu_allocation_capacity(unicorn.get_data(), bytes)?;
        let output = gpu_register(unicorn, RegisterX86::R9, "AllocateHostMemory output")?;
        write_gpu_output(unicorn, output, &0u64.to_le_bytes(), "AllocateHostMemory")?;
        let mapped_bytes = aligned_gpu_bytes(bytes)?;
        let address = find_gpu_host_region(&unicorn.get_data().gpu_suite, mapped_bytes)?;
        unicorn
            .mem_map(address, mapped_bytes, Prot::READ | Prot::WRITE)
            .map_err(|error| {
                GpuCallbackFailure::out_of_memory(format!("AllocateHostMemory map: {error}"))
            })?;
        if let Err(failure) = write_gpu_output(
            unicorn,
            output,
            &address.to_le_bytes(),
            "AllocateHostMemory",
        ) {
            let _ = unicorn.mem_unmap(address, mapped_bytes);
            return Err(failure);
        }
        unicorn.get_data_mut().gpu_suite.host_allocations.insert(
            address,
            GpuHostAllocation {
                device_index,
                bytes,
                mapped_bytes,
            },
        );
        unicorn.get_data_mut().gpu_suite.allocations_created += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_free_host(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "FreeHostMemory device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let address = gpu_register(unicorn, RegisterX86::R8, "FreeHostMemory pointer")?;
        let allocation = unicorn
            .get_data()
            .gpu_suite
            .host_allocations
            .get(&address)
            .cloned()
            .ok_or_else(|| {
                GpuCallbackFailure::bad(format!("GPU host pointer {address:#x} is stale or forged"))
            })?;
        if allocation.device_index != device_index {
            return Err(GpuCallbackFailure::bad(format!(
                "GPU host pointer {address:#x} belongs to device {}",
                allocation.device_index
            )));
        }
        unicorn
            .mem_unmap(address, allocation.mapped_bytes)
            .map_err(|error| GpuCallbackFailure::bad(format!("FreeHostMemory unmap: {error}")))?;
        unicorn
            .get_data_mut()
            .gpu_suite
            .host_allocations
            .remove(&address);
        unicorn.get_data_mut().gpu_suite.allocations_freed += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn initialize_device_buffer(
    runtime: &GpuRuntime,
    token: u64,
    bytes: usize,
    value: u8,
) -> Result<(), GpuCallbackFailure> {
    let chunk = vec![value; bytes.min(GPU_FILL_CHUNK_BYTES)];
    let mut offset = 0usize;
    while offset < bytes {
        let length = (bytes - offset).min(chunk.len());
        runtime
            .write_device(token, offset, &chunk[..length])
            .map_err(GpuCallbackFailure::bad)?;
        offset += length;
    }
    Ok(())
}

fn emulate_gpu_create_world(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let device_index =
            gpu_register(unicorn, RegisterX86::RDX, "CreateGPUWorld device index")? as u32;
        validate_gpu_device(unicorn, device_index)?;
        let width = gpu_register(unicorn, RegisterX86::R8, "CreateGPUWorld width")? as u32 as i32;
        let height = gpu_register(unicorn, RegisterX86::R9, "CreateGPUWorld height")? as u32 as i32;
        let scale = gpu_stack_arg(unicorn, 0x28, "CreateGPUWorld pixel aspect")?;
        let field = gpu_stack_arg(unicorn, 0x30, "CreateGPUWorld field")? as u32 as i32;
        let pixel_format =
            gpu_stack_arg(unicorn, 0x38, "CreateGPUWorld pixel format")? as u32 as i32;
        let clear = gpu_stack_arg(unicorn, 0x40, "CreateGPUWorld clear")?;
        let output = gpu_stack_arg(unicorn, 0x48, "CreateGPUWorld output")?;
        write_gpu_output(unicorn, output, &0u64.to_le_bytes(), "CreateGPUWorld")?;
        let aspect_numerator = scale as u32 as i32;
        let aspect_denominator = (scale >> 32) as u32;
        if width <= 0
            || height <= 0
            || width > GPU_MAX_WORLD_DIMENSION
            || height > GPU_MAX_WORLD_DIMENSION
            || aspect_numerator <= 0
            || aspect_denominator == 0
            || !(0..=2).contains(&field)
            || pixel_format != PF_PIXEL_FORMAT_GPU_BGRA128
            || clear > 1
        {
            return Err(GpuCallbackFailure::bad(format!(
                "invalid GPU world request {width}x{height}, aspect={aspect_numerator}/{aspect_denominator}, field={field}, format={pixel_format:#x}, clear={clear}"
            )));
        }
        let bytes = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(16))
            .and_then(|rowbytes| rowbytes.checked_mul(height as usize))
            .ok_or_else(|| GpuCallbackFailure::bad("GPU world size overflow"))?;
        gpu_allocation_capacity(unicorn.get_data(), bytes)?;
        let descriptor = find_gpu_world_region(&unicorn.get_data().gpu_suite)?;
        let token = unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(device_index, bytes, BufferAccess::ReadWrite)
            .map_err(GpuCallbackFailure::out_of_memory)?;
        if let Err(failure) = initialize_device_buffer(
            &unicorn.get_data().gpu_runtime,
            token,
            bytes,
            if clear != 0 { 0 } else { 0xcd },
        ) {
            let _ = unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, token);
            return Err(failure);
        }
        if let Err(error) = unicorn.mem_map(descriptor, PAGE_SIZE, Prot::READ | Prot::WRITE) {
            let _ = unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, token);
            return Err(GpuCallbackFailure::out_of_memory(format!(
                "CreateGPUWorld descriptor map: {error}"
            )));
        }
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        definition[abi::LAYER_WORLD_FLAGS_OFFSET..abi::LAYER_WORLD_FLAGS_OFFSET + 4]
            .copy_from_slice(&3i32.to_le_bytes());
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&token.to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&width.saturating_mul(16).to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&width.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&height.to_le_bytes());
        for (index, value) in [0, 0, width, height].into_iter().enumerate() {
            let offset = abi::LAYER_EXTENT_HINT_OFFSET + index * 4;
            definition[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        definition[88..92].copy_from_slice(&aspect_numerator.to_le_bytes());
        definition[92..96].copy_from_slice(&aspect_denominator.to_le_bytes());
        if let Err(error) = unicorn.mem_write(descriptor, &definition) {
            let _ = unicorn.mem_unmap(descriptor, PAGE_SIZE);
            let _ = unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, token);
            return Err(GpuCallbackFailure::bad(format!(
                "CreateGPUWorld descriptor write: {error}"
            )));
        }
        if let Err(failure) =
            write_gpu_output(unicorn, output, &descriptor.to_le_bytes(), "CreateGPUWorld")
        {
            let _ = unicorn.mem_unmap(descriptor, PAGE_SIZE);
            let _ = unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, token);
            return Err(failure);
        }
        let suite = &mut unicorn.get_data_mut().gpu_suite;
        suite.worlds.insert(
            descriptor,
            GpuWorld {
                device_index,
                buffer_token: token,
                bytes,
                mapped_bytes: PAGE_SIZE,
            },
        );
        suite.allocations_created += 1;
        suite.worlds_created += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_dispose_world(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let world = gpu_register(unicorn, RegisterX86::RDX, "DisposeGPUWorld world")?;
        let record = unicorn
            .get_data()
            .gpu_suite
            .worlds
            .get(&world)
            .cloned()
            .ok_or_else(|| {
                GpuCallbackFailure::bad(format!(
                    "GPU world {world:#x} is stale, forged, or host-owned"
                ))
            })?;
        unicorn
            .get_data_mut()
            .gpu_runtime
            .free_device(record.device_index, record.buffer_token)
            .map_err(GpuCallbackFailure::bad)?;
        if let Err(error) = unicorn.mem_unmap(world, record.mapped_bytes) {
            return Err(GpuCallbackFailure::bad(format!(
                "DisposeGPUWorld descriptor unmap: {error}"
            )));
        }
        let suite = &mut unicorn.get_data_mut().gpu_suite;
        suite.worlds.remove(&world);
        suite.allocations_freed += 1;
        suite.worlds_disposed += 1;
        Ok(())
    })();
    finish_gpu_callback(unicorn, result);
}

fn gpu_world_record(state: &GuestState, world: u64) -> Option<(u64, usize, u32)> {
    state
        .gpu_suite
        .worlds
        .get(&world)
        .map(|record| (record.buffer_token, record.bytes, record.device_index))
        .or_else(|| {
            state
                .gpu_suite
                .borrowed_worlds
                .get(&world)
                .map(|record| (record.buffer_token, record.bytes, record.device_index))
        })
}

fn emulate_gpu_get_world_data(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let world = gpu_register(unicorn, RegisterX86::RDX, "GetGPUWorldData world")?;
        let output = gpu_register(unicorn, RegisterX86::R8, "GetGPUWorldData output")?;
        let (token, _, _) = gpu_world_record(unicorn.get_data(), world).ok_or_else(|| {
            GpuCallbackFailure::bad(format!("GPU world {world:#x} is not active"))
        })?;
        let descriptor_token = read_guest_u64(
            unicorn,
            world + abi::LAYER_DATA_OFFSET as u64,
            "GPU world data",
        )
        .map_err(GpuCallbackFailure::bad)?;
        if descriptor_token != token || !unicorn.get_data().gpu_runtime.is_buffer_token(token) {
            return Err(GpuCallbackFailure::bad(format!(
                "GPU world {world:#x} contains stale device token {descriptor_token:#x}"
            )));
        }
        write_gpu_output(unicorn, output, &token.to_le_bytes(), "GetGPUWorldData")
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_get_world_size(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let world = gpu_register(unicorn, RegisterX86::RDX, "GetGPUWorldSize world")?;
        let output = gpu_register(unicorn, RegisterX86::R8, "GetGPUWorldSize output")?;
        let (_, bytes, _) = gpu_world_record(unicorn.get_data(), world).ok_or_else(|| {
            GpuCallbackFailure::bad(format!("GPU world {world:#x} is not active"))
        })?;
        write_gpu_output(
            unicorn,
            output,
            &(bytes as u64).to_le_bytes(),
            "GetGPUWorldSize",
        )
    })();
    finish_gpu_callback(unicorn, result);
}

fn emulate_gpu_get_world_device_index(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let world = gpu_register(unicorn, RegisterX86::RDX, "GetGPUWorldDeviceIndex world")?;
        let output = gpu_register(unicorn, RegisterX86::R8, "GetGPUWorldDeviceIndex output")?;
        let (_, _, device_index) =
            gpu_world_record(unicorn.get_data(), world).ok_or_else(|| {
                GpuCallbackFailure::bad(format!("GPU world {world:#x} is not active"))
            })?;
        write_gpu_output(
            unicorn,
            output,
            &device_index.to_le_bytes(),
            "GetGPUWorldDeviceIndex",
        )
    })();
    finish_gpu_callback(unicorn, result);
}

fn install_gpu_device_suite(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), unicorn_engine::unicorn_const::uc_error> {
    let callbacks: [fn(&mut Unicorn<'_, GuestState>, u64, u32); 15] = [
        emulate_gpu_get_device_count,
        emulate_gpu_get_device_info,
        emulate_gpu_acquire_exclusive,
        emulate_gpu_release_exclusive,
        emulate_gpu_allocate_device,
        emulate_gpu_free_device,
        emulate_gpu_purge_device,
        emulate_gpu_allocate_host,
        emulate_gpu_free_host,
        emulate_gpu_purge_host,
        emulate_gpu_create_world,
        emulate_gpu_dispose_world,
        emulate_gpu_get_world_data,
        emulate_gpu_get_world_size,
        emulate_gpu_get_world_device_index,
    ];
    let mut table = [0u8; 15 * 8];
    for (slot, (address, callback)) in HOST_GPU_SUITE_CALLBACKS
        .into_iter()
        .zip(callbacks)
        .enumerate()
    {
        unicorn.mem_write(address, &[0xc3])?;
        unicorn.add_code_hook(address, address, callback)?;
        table[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
    }
    unicorn.mem_write(HOST_GPU_DEVICE_SUITE_V1, &table)
}

fn read_transport_world(
    unicorn: &Unicorn<'_, GuestState>,
    world: u64,
    label: &str,
) -> Result<(u64, i32, i32, i32), GuestError> {
    if world == 0 {
        return Err(GuestError::Callback(format!("{label} world is null")));
    }
    let data = read_guest_u64(
        unicorn,
        world + abi::LAYER_DATA_OFFSET as u64,
        &format!("{label} data"),
    )
    .map_err(GuestError::Callback)?;
    let rowbytes = read_guest_i32(
        unicorn,
        world + abi::LAYER_ROWBYTES_OFFSET as u64,
        &format!("{label} rowbytes"),
    )
    .map_err(GuestError::Callback)?;
    let width = read_guest_i32(
        unicorn,
        world + abi::LAYER_WIDTH_OFFSET as u64,
        &format!("{label} width"),
    )
    .map_err(GuestError::Callback)?;
    let height = read_guest_i32(
        unicorn,
        world + abi::LAYER_HEIGHT_OFFSET as u64,
        &format!("{label} height"),
    )
    .map_err(GuestError::Callback)?;
    if data == 0
        || width <= 0
        || height <= 0
        || width > GPU_MAX_WORLD_DIMENSION
        || height > GPU_MAX_WORLD_DIMENSION
        || rowbytes < width.saturating_mul(16)
        || rowbytes > GPU_MAX_WORLD_DIMENSION.saturating_mul(16)
    {
        return Err(GuestError::Callback(format!(
            "{label} layout is invalid: data={data:#x}, {width}x{height}, rowbytes={rowbytes}"
        )));
    }
    let bytes = usize::try_from(rowbytes)
        .ok()
        .and_then(|rowbytes| rowbytes.checked_mul(height as usize))
        .filter(|bytes| *bytes <= GPU_MAX_LIVE_BYTES)
        .ok_or_else(|| GuestError::Callback(format!("{label} host byte count is invalid")))?;
    unicorn
        .mem_read_as_vec(data, bytes.min(1))
        .map_err(|error| GuestError::Callback(format!("{label} data is unmapped: {error}")))?;
    Ok((data, rowbytes, width, height))
}

fn read_argb32f_as_bgra128(
    unicorn: &Unicorn<'_, GuestState>,
    data: u64,
    rowbytes: i32,
    width: i32,
    height: i32,
) -> Result<Vec<u8>, GuestError> {
    let packed_rowbytes = width as usize * 16;
    let mut output = vec![0u8; packed_rowbytes * height as usize];
    let mut row = vec![0u8; packed_rowbytes];
    for y in 0..height as usize {
        unicorn
            .mem_read(data + (y * rowbytes as usize) as u64, &mut row)
            .map_err(|error| GuestError::Callback(format!("GPU input row {y}: {error}")))?;
        for (argb, bgra) in row
            .chunks_exact(16)
            .zip(output[y * packed_rowbytes..(y + 1) * packed_rowbytes].chunks_exact_mut(16))
        {
            bgra[0..4].copy_from_slice(&argb[12..16]);
            bgra[4..8].copy_from_slice(&argb[8..12]);
            bgra[8..12].copy_from_slice(&argb[4..8]);
            bgra[12..16].copy_from_slice(&argb[0..4]);
        }
    }
    Ok(output)
}

fn write_bgra128_as_argb32f(
    unicorn: &mut Unicorn<'_, GuestState>,
    world: &GpuTransportWorld,
    bgra: &[u8],
) -> Result<(), GuestError> {
    let packed_rowbytes = world.width as usize * 16;
    if bgra.len() != packed_rowbytes * world.height as usize {
        return Err(GuestError::Callback(
            "GPU output byte count differs from its world".into(),
        ));
    }
    let mut row = vec![0u8; packed_rowbytes];
    for y in 0..world.height as usize {
        for (bgra, argb) in bgra[y * packed_rowbytes..(y + 1) * packed_rowbytes]
            .chunks_exact(16)
            .zip(row.chunks_exact_mut(16))
        {
            argb[0..4].copy_from_slice(&bgra[12..16]);
            argb[4..8].copy_from_slice(&bgra[8..12]);
            argb[8..12].copy_from_slice(&bgra[4..8]);
            argb[12..16].copy_from_slice(&bgra[0..4]);
        }
        unicorn
            .mem_write(
                world.host_data + (y * world.host_rowbytes as usize) as u64,
                &row,
            )
            .map_err(|error| GuestError::Callback(format!("GPU output row {y}: {error}")))?;
    }
    Ok(())
}

impl GuestEngine<'_> {
    pub(crate) fn begin_opencl_gpu(&mut self, device_index: u32) -> Result<(), GuestError> {
        self.unicorn
            .get_data_mut()
            .gpu_runtime
            .begin_opencl(device_index)
            .map(|_| ())
            .map_err(GuestError::Callback)
    }

    pub(crate) fn end_opencl_gpu(&mut self) -> Result<ObjectCounts, GuestError> {
        if self.unicorn.get_data().gpu_suite.transport.is_some() {
            self.finish_gpu_render_transport()?;
        }
        let state = self.unicorn.get_data();
        if !state.gpu_suite.worlds.is_empty()
            || !state.gpu_suite.host_allocations.is_empty()
            || !state.gpu_suite.borrowed_worlds.is_empty()
            || state.gpu_suite.exclusive_access_depth != 0
        {
            return Err(GuestError::Callback(
                "GPU Device Suite resources remain live at OpenCL shutdown".into(),
            ));
        }
        self.unicorn
            .get_data_mut()
            .gpu_runtime
            .end_opencl()
            .map_err(GuestError::Callback)
    }

    pub(crate) fn prepare_gpu_render_transport(&mut self) -> Result<(), GuestError> {
        if self.unicorn.get_data().gpu_suite.transport.is_some() {
            return Err(GuestError::Callback(
                "GPU render transport is already active".into(),
            ));
        }
        if self.unicorn.get_data().smart_pixel_format != crate::pixel::PF_PIXEL_FORMAT_ARGB128 {
            return Err(GuestError::Callback(
                "GPU render transport requires ARGB32F host worlds".into(),
            ));
        }
        let input_world = self.unicorn.get_data().smart_input_world;
        let output_world = self.unicorn.get_data().smart_output_world;
        let (input_host, input_rowbytes, input_width, input_height) =
            read_transport_world(&self.unicorn, input_world, "GPU input")?;
        let (output_host, output_rowbytes, output_width, output_height) =
            read_transport_world(&self.unicorn, output_world, "GPU output")?;
        let input_bytes = input_width as usize * input_height as usize * 16;
        let output_bytes = output_width as usize * output_height as usize * 16;
        gpu_allocation_capacity(self.unicorn.get_data(), input_bytes)
            .map_err(|failure| GuestError::Callback(failure.message))?;
        let device_index = self
            .unicorn
            .get_data()
            .gpu_runtime
            .device_index()
            .ok_or_else(|| GuestError::Callback("GPU runtime is not active".into()))?;
        let input_device = self
            .unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(device_index, input_bytes, BufferAccess::ReadOnly)
            .map_err(GuestError::Callback)?;
        if let Err(failure) = gpu_allocation_capacity(self.unicorn.get_data(), output_bytes) {
            let _ = self
                .unicorn
                .get_data_mut()
                .gpu_runtime
                .free_device(device_index, input_device);
            return Err(GuestError::Callback(failure.message));
        }
        let output_device = match self.unicorn.get_data_mut().gpu_runtime.allocate_device(
            device_index,
            output_bytes,
            BufferAccess::WriteOnly,
        ) {
            Ok(token) => token,
            Err(error) => {
                let _ = self
                    .unicorn
                    .get_data_mut()
                    .gpu_runtime
                    .free_device(device_index, input_device);
                return Err(GuestError::Callback(error));
            }
        };
        let upload = match read_argb32f_as_bgra128(
            &self.unicorn,
            input_host,
            input_rowbytes,
            input_width,
            input_height,
        ) {
            Ok(upload) => upload,
            Err(error) => {
                let state = self.unicorn.get_data_mut();
                let _ = state.gpu_runtime.free_device(device_index, output_device);
                let _ = state.gpu_runtime.free_device(device_index, input_device);
                return Err(error);
            }
        };
        let prepared = self
            .unicorn
            .get_data()
            .gpu_runtime
            .write_device(input_device, 0, &upload)
            .and_then(|()| {
                initialize_device_buffer(
                    &self.unicorn.get_data().gpu_runtime,
                    output_device,
                    output_bytes,
                    0xcc,
                )
                .map_err(|failure| failure.message)
            });
        if let Err(error) = prepared {
            let state = self.unicorn.get_data_mut();
            let _ = state.gpu_runtime.free_device(device_index, output_device);
            let _ = state.gpu_runtime.free_device(device_index, input_device);
            return Err(GuestError::Callback(error));
        }
        let input = GpuTransportWorld {
            world: input_world,
            host_data: input_host,
            device_token: input_device,
            width: input_width,
            height: input_height,
            host_rowbytes: input_rowbytes,
            device_bytes: input_bytes,
        };
        let output = GpuTransportWorld {
            world: output_world,
            host_data: output_host,
            device_token: output_device,
            width: output_width,
            height: output_height,
            host_rowbytes: output_rowbytes,
            device_bytes: output_bytes,
        };
        if let Err(error) = self.swap_and_register_gpu_worlds(device_index, &input, &output) {
            let state = self.unicorn.get_data_mut();
            let _ = state.gpu_runtime.free_device(device_index, output_device);
            let _ = state.gpu_runtime.free_device(device_index, input_device);
            return Err(error);
        }
        let state = self.unicorn.get_data_mut();
        let previous_render_pixel_format = state.render_pixel_format;
        state.render_pixel_format = PF_PIXEL_FORMAT_GPU_BGRA128;
        state.gpu_suite.allocations_created += 2;
        state.gpu_suite.upload_bytes += input_bytes as u64;
        state.gpu_suite.transport = Some(GpuRenderTransport {
            input,
            output,
            previous_render_pixel_format,
        });
        Ok(())
    }

    fn swap_and_register_gpu_worlds(
        &mut self,
        device_index: u32,
        input: &GpuTransportWorld,
        output: &GpuTransportWorld,
    ) -> Result<(), GuestError> {
        self.unicorn
            .mem_write(
                input.world + abi::LAYER_DATA_OFFSET as u64,
                &input.device_token.to_le_bytes(),
            )
            .map_err(|error| GuestError::Callback(format!("GPU input token swap: {error}")))?;
        if let Err(error) = self.unicorn.mem_write(
            output.world + abi::LAYER_DATA_OFFSET as u64,
            &output.device_token.to_le_bytes(),
        ) {
            let _ = self.unicorn.mem_write(
                input.world + abi::LAYER_DATA_OFFSET as u64,
                &input.host_data.to_le_bytes(),
            );
            return Err(GuestError::Callback(format!(
                "GPU output token swap: {error}"
            )));
        }
        if let Err(error) = self.register_borrowed_gpu_world(
            input.world,
            device_index,
            input.device_token,
            input.device_bytes,
        ) {
            let _ = self.restore_gpu_world_pointers(input, output);
            return Err(error);
        }
        if let Err(error) = self.register_borrowed_gpu_world(
            output.world,
            device_index,
            output.device_token,
            output.device_bytes,
        ) {
            self.unregister_borrowed_gpu_world(input.world, input.device_token);
            let _ = self.restore_gpu_world_pointers(input, output);
            return Err(error);
        }
        Ok(())
    }

    fn restore_gpu_world_pointers(
        &mut self,
        input: &GpuTransportWorld,
        output: &GpuTransportWorld,
    ) -> Result<(), String> {
        let input_result = self.unicorn.mem_write(
            input.world + abi::LAYER_DATA_OFFSET as u64,
            &input.host_data.to_le_bytes(),
        );
        let output_result = self.unicorn.mem_write(
            output.world + abi::LAYER_DATA_OFFSET as u64,
            &output.host_data.to_le_bytes(),
        );
        match (input_result, output_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(input), Ok(())) => Err(format!("GPU input pointer restore: {input}")),
            (Ok(()), Err(output)) => Err(format!("GPU output pointer restore: {output}")),
            (Err(input), Err(output)) => Err(format!(
                "GPU input pointer restore: {input}; GPU output pointer restore: {output}"
            )),
        }
    }

    pub(crate) fn finish_gpu_render_transport(&mut self) -> Result<(), GuestError> {
        let transport = self
            .unicorn
            .get_data_mut()
            .gpu_suite
            .transport
            .take()
            .ok_or_else(|| GuestError::Callback("GPU render transport is not active".into()))?;
        let device_index = self
            .unicorn
            .get_data()
            .gpu_runtime
            .device_index()
            .ok_or_else(|| GuestError::Callback("GPU runtime is not active".into()))?;
        let mut first_error = self.unicorn.get_data().gpu_runtime.finish().err();
        let mut output_bytes = vec![0u8; transport.output.device_bytes];
        if first_error.is_none()
            && let Err(error) = self.unicorn.get_data().gpu_runtime.read_device(
                transport.output.device_token,
                0,
                &mut output_bytes,
            )
        {
            first_error = Some(error);
        }
        if first_error.is_none()
            && let Err(error) =
                write_bgra128_as_argb32f(&mut self.unicorn, &transport.output, &output_bytes)
        {
            first_error = Some(error.to_string());
        }
        if let Err(error) = self.restore_gpu_world_pointers(&transport.input, &transport.output)
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if !self.unregister_borrowed_gpu_world(transport.input.world, transport.input.device_token)
            && first_error.is_none()
        {
            first_error = Some("GPU input world registry restore failed".into());
        }
        if !self
            .unregister_borrowed_gpu_world(transport.output.world, transport.output.device_token)
            && first_error.is_none()
        {
            first_error = Some("GPU output world registry restore failed".into());
        }
        let state = self.unicorn.get_data_mut();
        state.render_pixel_format = transport.previous_render_pixel_format;
        if state
            .gpu_runtime
            .free_device(device_index, transport.output.device_token)
            .is_ok()
        {
            state.gpu_suite.allocations_freed += 1;
        } else if first_error.is_none() {
            first_error = Some("GPU output device free failed".into());
        }
        if state
            .gpu_runtime
            .free_device(device_index, transport.input.device_token)
            .is_ok()
        {
            state.gpu_suite.allocations_freed += 1;
        } else if first_error.is_none() {
            first_error = Some("GPU input device free failed".into());
        }
        if first_error.is_none() {
            state.gpu_suite.download_bytes += transport.output.device_bytes as u64;
        } else {
            state.gpu_suite.last_error = first_error.clone();
        }
        first_error.map_or(Ok(()), |error| Err(GuestError::Callback(error)))
    }

    pub(crate) fn register_borrowed_gpu_world(
        &mut self,
        world: u64,
        device_index: u32,
        buffer_token: u64,
        bytes: usize,
    ) -> Result<(), GuestError> {
        if world == 0
            || bytes == 0
            || self
                .unicorn
                .get_data()
                .gpu_suite
                .worlds
                .contains_key(&world)
            || self
                .unicorn
                .get_data()
                .gpu_suite
                .borrowed_worlds
                .contains_key(&world)
            || self.unicorn.get_data().gpu_runtime.device_index() != Some(device_index)
            || self.unicorn.get_data().gpu_runtime.buffer_len(buffer_token) != Some(bytes)
        {
            return Err(GuestError::Callback(
                "invalid borrowed GPU world registration".into(),
            ));
        }
        self.unicorn
            .get_data_mut()
            .gpu_suite
            .borrowed_worlds
            .insert(
                world,
                BorrowedGpuWorld {
                    device_index,
                    buffer_token,
                    bytes,
                },
            );
        Ok(())
    }

    pub(crate) fn unregister_borrowed_gpu_world(&mut self, world: u64, buffer_token: u64) -> bool {
        if self
            .unicorn
            .get_data()
            .gpu_suite
            .borrowed_worlds
            .get(&world)
            .is_none_or(|record| record.buffer_token != buffer_token)
        {
            return false;
        }
        self.unicorn
            .get_data_mut()
            .gpu_suite
            .borrowed_worlds
            .remove(&world);
        true
    }

    pub(crate) fn gpu_suite_evidence(&self) -> GpuSuiteEvidence {
        let state = self.unicorn.get_data();
        state.gpu_suite.evidence(&state.gpu_runtime)
    }
}
