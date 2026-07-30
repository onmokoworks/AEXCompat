use std::{
    collections::HashMap,
    ffi::{CString, c_char},
    ptr,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
};

use crate::{
    BufferAccess, Error, GpuDevice, KernelScalar, MAX_BUFFER_BYTES, MAX_BUILD_LOG_BYTES,
    MAX_BUILD_OPTIONS_BYTES, MAX_GLOBAL_WORK_ITEMS, MAX_GPU_DEVICE_COUNT,
    MAX_KERNEL_ARGUMENT_BYTES, MAX_KERNEL_NAME_BYTES, MAX_PLATFORM_COUNT, MAX_PROGRAM_SOURCE_BYTES,
    ObjectTracker,
    common::{MAX_INFO_BYTES, MAX_WORK_DIMENSIONS, ObjectCounters},
    ffi,
};

struct RawGpuDevice {
    platform: ffi::ClPlatformId,
    device: ffi::ClDeviceId,
    info: GpuDevice,
}

struct SessionState {
    context: ffi::ClContext,
    queue: ffi::ClCommandQueue,
    device_id: ffi::ClDeviceId,
    device: GpuDevice,
    counters: Arc<ObjectCounters>,
}

impl Drop for SessionState {
    fn drop(&mut self) {
        release(
            &self.counters,
            &self.counters.command_queues,
            // SAFETY: this state exclusively owns one retained queue handle.
            unsafe { ffi::clReleaseCommandQueue(self.queue) },
        );
        release(
            &self.counters,
            &self.counters.contexts,
            // SAFETY: this state exclusively owns one retained context handle.
            unsafe { ffi::clReleaseContext(self.context) },
        );
    }
}

pub struct Session {
    state: Rc<SessionState>,
}

pub struct Buffer {
    inner: Rc<BufferState>,
}

struct BufferState {
    handle: ffi::ClMem,
    bytes: usize,
    access: BufferAccess,
    session: Rc<SessionState>,
}

pub struct Program {
    handle: ffi::ClProgram,
    build_log: String,
    state: Rc<SessionState>,
}

pub struct Kernel {
    handle: ffi::ClKernel,
    state: Rc<SessionState>,
    bound_buffers: HashMap<u32, Buffer>,
}

pub fn enumerate_gpu_devices() -> Result<Vec<GpuDevice>, Error> {
    Ok(enumerate_raw_gpu_devices()?
        .into_iter()
        .map(|device| device.info)
        .collect())
}

impl Session {
    pub fn select_gpu(index: usize) -> Result<Self, Error> {
        let devices = enumerate_raw_gpu_devices()?;
        let available = devices.len();
        let selected = devices
            .into_iter()
            .nth(index)
            .ok_or(Error::DeviceIndexOutOfRange {
                requested: index,
                available,
            })?;

        let properties = [
            ffi::CL_CONTEXT_PLATFORM,
            selected.platform as ffi::ClContextProperties,
            0,
        ];
        let mut status = ffi::CL_SUCCESS;
        // SAFETY: all pointers reference live fixed-size inputs for the duration
        // of this synchronous OpenCL call; no callback or user data is supplied.
        let context = unsafe {
            ffi::clCreateContext(
                properties.as_ptr(),
                1,
                &selected.device,
                None,
                ptr::null_mut(),
                &mut status,
            )
        };
        if status != ffi::CL_SUCCESS || context.is_null() {
            return Err(Error::api("clCreateContext", status));
        }

        // SAFETY: context and device were returned by the same OpenCL platform.
        let queue = unsafe { ffi::clCreateCommandQueue(context, selected.device, 0, &mut status) };
        if status != ffi::CL_SUCCESS || queue.is_null() {
            // SAFETY: context is a live handle owned by this function.
            let release_status = unsafe { ffi::clReleaseContext(context) };
            if release_status != ffi::CL_SUCCESS {
                return Err(Error::api_with_detail(
                    "clCreateCommandQueue",
                    status,
                    format!("context cleanup also failed with status {release_status}"),
                ));
            }
            return Err(Error::api("clCreateCommandQueue", status));
        }

        let counters = Arc::new(ObjectCounters::default());
        counters.contexts.store(1, Ordering::Release);
        counters.command_queues.store(1, Ordering::Release);
        Ok(Self {
            state: Rc::new(SessionState {
                context,
                queue,
                device_id: selected.device,
                device: selected.info,
                counters,
            }),
        })
    }

    #[must_use]
    pub fn device(&self) -> &GpuDevice {
        &self.state.device
    }

    #[must_use]
    pub fn object_tracker(&self) -> ObjectTracker {
        ObjectTracker::new(Arc::clone(&self.state.counters))
    }

    pub fn create_buffer(&self, bytes: usize, access: BufferAccess) -> Result<Buffer, Error> {
        if bytes == 0 {
            return Err(Error::ZeroBufferSize);
        }
        if bytes > MAX_BUFFER_BYTES {
            return Err(Error::LimitExceeded {
                resource: "buffer bytes",
                actual: bytes,
                maximum: MAX_BUFFER_BYTES,
            });
        }
        let flags = match access {
            BufferAccess::ReadWrite => ffi::CL_MEM_READ_WRITE,
            BufferAccess::ReadOnly => ffi::CL_MEM_READ_ONLY,
            BufferAccess::WriteOnly => ffi::CL_MEM_WRITE_ONLY,
        };
        let mut status = ffi::CL_SUCCESS;
        // SAFETY: context is live, size is non-zero and bounded, and no host
        // pointer flag is used.
        let handle = unsafe {
            ffi::clCreateBuffer(
                self.state.context,
                flags,
                bytes,
                ptr::null_mut(),
                &mut status,
            )
        };
        if status != ffi::CL_SUCCESS || handle.is_null() {
            return Err(Error::api("clCreateBuffer", status));
        }
        self.state.counters.buffers.fetch_add(1, Ordering::AcqRel);
        Ok(Buffer {
            inner: Rc::new(BufferState {
                handle,
                bytes,
                access,
                session: Rc::clone(&self.state),
            }),
        })
    }

    pub fn write_buffer(&self, buffer: &Buffer, offset: usize, source: &[u8]) -> Result<(), Error> {
        ensure_same_session(&self.state, &buffer.inner.session, "buffer")?;
        checked_range("buffer write", offset, source.len(), buffer.inner.bytes)?;
        if source.is_empty() {
            return Ok(());
        }
        // SAFETY: range checking proves OpenCL may read exactly source.len()
        // bytes, both the queue and buffer are live, and the blocking call
        // finishes before the slice can be invalidated.
        let status = unsafe {
            ffi::clEnqueueWriteBuffer(
                self.state.queue,
                buffer.inner.handle,
                ffi::CL_TRUE,
                offset,
                source.len(),
                source.as_ptr().cast(),
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        check("clEnqueueWriteBuffer", status)
    }

    pub fn read_buffer(
        &self,
        buffer: &Buffer,
        offset: usize,
        destination: &mut [u8],
    ) -> Result<(), Error> {
        ensure_same_session(&self.state, &buffer.inner.session, "buffer")?;
        checked_range("buffer read", offset, destination.len(), buffer.inner.bytes)?;
        if destination.is_empty() {
            return Ok(());
        }
        // SAFETY: range checking proves OpenCL may write exactly
        // destination.len() bytes, both handles are live, and this call blocks.
        let status = unsafe {
            ffi::clEnqueueReadBuffer(
                self.state.queue,
                buffer.inner.handle,
                ffi::CL_TRUE,
                offset,
                destination.len(),
                destination.as_mut_ptr().cast(),
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        check("clEnqueueReadBuffer", status)
    }

    pub fn build_program(&self, source: &str, options: Option<&str>) -> Result<Program, Error> {
        if source.is_empty() {
            return Err(Error::EmptyProgramSource);
        }
        if source.len() > MAX_PROGRAM_SOURCE_BYTES {
            return Err(Error::LimitExceeded {
                resource: "program source bytes",
                actual: source.len(),
                maximum: MAX_PROGRAM_SOURCE_BYTES,
            });
        }
        let options = options.map(build_options).transpose()?;
        let source_pointer = source.as_ptr().cast::<c_char>();
        let source_length = source.len();
        let mut status = ffi::CL_SUCCESS;
        // SAFETY: source pointer and explicit byte length remain valid for the
        // synchronous create call.
        let handle = unsafe {
            ffi::clCreateProgramWithSource(
                self.state.context,
                1,
                &source_pointer,
                &source_length,
                &mut status,
            )
        };
        if status != ffi::CL_SUCCESS || handle.is_null() {
            return Err(Error::api("clCreateProgramWithSource", status));
        }

        // SAFETY: program and selected device are live. OpenCL copies the
        // options during this synchronous build because no callback is used.
        status = unsafe {
            ffi::clBuildProgram(
                handle,
                1,
                &self.state.device_id,
                options.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
                None,
                ptr::null_mut(),
            )
        };
        let build_log = program_build_log(handle, self.state.device_id)
            .unwrap_or_else(|error| format!("<build log unavailable: {error}>"));
        if status != ffi::CL_SUCCESS {
            // SAFETY: handle is live and owned by this function.
            let release_status = unsafe { ffi::clReleaseProgram(handle) };
            let log = if release_status == ffi::CL_SUCCESS {
                build_log
            } else {
                self.state
                    .counters
                    .release_errors
                    .fetch_add(1, Ordering::AcqRel);
                format!("{build_log}\nprogram cleanup also failed with status {release_status}")
            };
            return Err(Error::ProgramBuild { code: status, log });
        }

        self.state.counters.programs.fetch_add(1, Ordering::AcqRel);
        Ok(Program {
            handle,
            build_log,
            state: Rc::clone(&self.state),
        })
    }

    pub fn enqueue_nd_range(
        &self,
        kernel: &Kernel,
        global: &[usize],
        local: Option<&[usize]>,
    ) -> Result<(), Error> {
        self.enqueue_nd_range_with_offset(kernel, None, global, local)
    }

    pub fn enqueue_nd_range_with_offset(
        &self,
        kernel: &Kernel,
        global_offset: Option<&[usize]>,
        global: &[usize],
        local: Option<&[usize]>,
    ) -> Result<(), Error> {
        ensure_same_session(&self.state, &kernel.state, "kernel")?;
        validate_work_sizes(global_offset, global, local)?;
        // SAFETY: work-size slices are validated and remain live for this
        // enqueue call. Kernel and queue belong to the same live context.
        let status = unsafe {
            ffi::clEnqueueNDRangeKernel(
                self.state.queue,
                kernel.handle,
                global.len() as ffi::ClUint,
                global_offset.map_or(ptr::null(), <[usize]>::as_ptr),
                global.as_ptr(),
                local.map_or(ptr::null(), <[usize]>::as_ptr),
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        check("clEnqueueNDRangeKernel", status)
    }

    pub fn finish(&self) -> Result<(), Error> {
        // SAFETY: queue is live for the lifetime of the session state.
        check("clFinish", unsafe { ffi::clFinish(self.state.queue) })
    }
}

impl Buffer {
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.bytes
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.bytes == 0
    }

    #[must_use]
    pub fn access(&self) -> BufferAccess {
        self.inner.access
    }
}

impl Drop for BufferState {
    fn drop(&mut self) {
        release(
            &self.session.counters,
            &self.session.counters.buffers,
            // SAFETY: this wrapper exclusively owns one retained memory handle.
            unsafe { ffi::clReleaseMemObject(self.handle) },
        );
    }
}

impl Program {
    #[must_use]
    pub fn build_log(&self) -> &str {
        &self.build_log
    }

    pub fn create_kernel(&self, name: &str) -> Result<Kernel, Error> {
        if name.len() > MAX_KERNEL_NAME_BYTES {
            return Err(Error::LimitExceeded {
                resource: "kernel name bytes",
                actual: name.len(),
                maximum: MAX_KERNEL_NAME_BYTES,
            });
        }
        let name = CString::new(name).map_err(|_| Error::InteriorNul {
            field: "kernel name",
        })?;
        let mut status = ffi::CL_SUCCESS;
        // SAFETY: program is live and CString supplies a terminated name.
        let handle = unsafe { ffi::clCreateKernel(self.handle, name.as_ptr(), &mut status) };
        if status != ffi::CL_SUCCESS || handle.is_null() {
            return Err(Error::api("clCreateKernel", status));
        }
        self.state.counters.kernels.fetch_add(1, Ordering::AcqRel);
        Ok(Kernel {
            handle,
            state: Rc::clone(&self.state),
            bound_buffers: HashMap::new(),
        })
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        release(
            &self.state.counters,
            &self.state.counters.programs,
            // SAFETY: this wrapper exclusively owns one retained program handle.
            unsafe { ffi::clReleaseProgram(self.handle) },
        );
    }
}

impl Kernel {
    pub fn set_scalar_arg<T: KernelScalar>(&mut self, index: u32, value: T) -> Result<(), Error> {
        // SAFETY: KernelScalar is sealed to padding-free primitive values.
        let bytes =
            unsafe { std::slice::from_raw_parts((&value as *const T).cast(), size_of::<T>()) };
        self.set_raw_arg(index, bytes)
    }

    /// Copies an opaque, bounded argument value into the native kernel.
    ///
    /// This is the ABI-preserving path for a foreign OpenCL caller whose
    /// scalar/vector argument type is not known to the host. Buffer handles
    /// must still use [`Self::set_buffer_arg`] so session ownership is checked.
    pub fn set_raw_arg(&mut self, index: u32, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Err(Error::ZeroKernelArgumentSize);
        }
        if bytes.len() > MAX_KERNEL_ARGUMENT_BYTES {
            return Err(Error::LimitExceeded {
                resource: "kernel argument bytes",
                actual: bytes.len(),
                maximum: MAX_KERNEL_ARGUMENT_BYTES,
            });
        }
        // SAFETY: bytes is non-empty and bounded, and OpenCL copies exactly
        // bytes.len() bytes during this synchronous call.
        let status =
            unsafe { ffi::clSetKernelArg(self.handle, index, bytes.len(), bytes.as_ptr().cast()) };
        check("clSetKernelArg(raw)", status)?;
        self.bound_buffers.remove(&index);
        Ok(())
    }

    pub fn set_buffer_arg(&mut self, index: u32, buffer: &Buffer) -> Result<(), Error> {
        ensure_same_session(&self.state, &buffer.inner.session, "buffer")?;
        let handle = buffer.inner.handle;
        // SAFETY: clSetKernelArg copies one live cl_mem handle. Session identity
        // checking prevents a cross-context memory argument.
        let status = unsafe {
            ffi::clSetKernelArg(
                self.handle,
                index,
                size_of::<ffi::ClMem>(),
                (&handle as *const ffi::ClMem).cast(),
            )
        };
        check("clSetKernelArg(buffer)", status)?;
        self.bound_buffers.insert(
            index,
            Buffer {
                inner: Rc::clone(&buffer.inner),
            },
        );
        Ok(())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        release(
            &self.state.counters,
            &self.state.counters.kernels,
            // SAFETY: this wrapper exclusively owns one retained kernel handle.
            unsafe { ffi::clReleaseKernel(self.handle) },
        );
    }
}

fn enumerate_raw_gpu_devices() -> Result<Vec<RawGpuDevice>, Error> {
    let mut platform_count = 0;
    // SAFETY: the count-only query writes one ClUint to platform_count.
    let status = unsafe { ffi::clGetPlatformIDs(0, ptr::null_mut(), &mut platform_count) };
    check("clGetPlatformIDs(count)", status)?;
    let platform_count = usize::try_from(platform_count).expect("ClUint fits usize");
    if platform_count > MAX_PLATFORM_COUNT {
        return Err(Error::LimitExceeded {
            resource: "OpenCL platform count",
            actual: platform_count,
            maximum: MAX_PLATFORM_COUNT,
        });
    }
    if platform_count == 0 {
        return Err(Error::NoGpuDevice);
    }

    let mut platforms = vec![ptr::null_mut(); platform_count];
    // SAFETY: platforms has exactly platform_count writable handle slots.
    let status = unsafe {
        ffi::clGetPlatformIDs(
            platform_count as ffi::ClUint,
            platforms.as_mut_ptr(),
            ptr::null_mut(),
        )
    };
    check("clGetPlatformIDs(values)", status)?;

    let mut result = Vec::new();
    for platform in platforms {
        let mut device_count = 0;
        // SAFETY: platform came from OpenCL and this is a count-only query.
        let status = unsafe {
            ffi::clGetDeviceIDs(
                platform,
                ffi::CL_DEVICE_TYPE_GPU,
                0,
                ptr::null_mut(),
                &mut device_count,
            )
        };
        if status == ffi::CL_DEVICE_NOT_FOUND {
            continue;
        }
        check("clGetDeviceIDs(count)", status)?;
        let device_count = usize::try_from(device_count).expect("ClUint fits usize");
        if device_count == 0 {
            continue;
        }
        let total = result
            .len()
            .checked_add(device_count)
            .ok_or(Error::LimitExceeded {
                resource: "OpenCL GPU device count",
                actual: usize::MAX,
                maximum: MAX_GPU_DEVICE_COUNT,
            })?;
        if total > MAX_GPU_DEVICE_COUNT {
            return Err(Error::LimitExceeded {
                resource: "OpenCL GPU device count",
                actual: total,
                maximum: MAX_GPU_DEVICE_COUNT,
            });
        }

        let mut devices = vec![ptr::null_mut(); device_count];
        // SAFETY: devices has exactly device_count writable handle slots.
        let status = unsafe {
            ffi::clGetDeviceIDs(
                platform,
                ffi::CL_DEVICE_TYPE_GPU,
                device_count as ffi::ClUint,
                devices.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        check("clGetDeviceIDs(values)", status)?;

        let platform_name = platform_string(platform, ffi::CL_PLATFORM_NAME)?;
        for device in devices {
            let compute_units = device_u32(device, ffi::CL_DEVICE_MAX_COMPUTE_UNITS)?;
            let info = GpuDevice::new(
                result.len(),
                platform_name.clone(),
                device_string(device, ffi::CL_DEVICE_NAME)?,
                device_string(device, ffi::CL_DEVICE_VENDOR)?,
                compute_units,
            );
            result.push(RawGpuDevice {
                platform,
                device,
                info,
            });
        }
    }
    if result.is_empty() {
        return Err(Error::NoGpuDevice);
    }
    Ok(result)
}

fn platform_string(
    platform: ffi::ClPlatformId,
    parameter: ffi::ClPlatformInfo,
) -> Result<String, Error> {
    let mut bytes = 0;
    // SAFETY: count-only query writes one usize.
    let status =
        unsafe { ffi::clGetPlatformInfo(platform, parameter, 0, ptr::null_mut(), &mut bytes) };
    check("clGetPlatformInfo(size)", status)?;
    let mut value = bounded_info_buffer("OpenCL platform info", bytes)?;
    // SAFETY: value has the reported number of writable bytes.
    let status = unsafe {
        ffi::clGetPlatformInfo(
            platform,
            parameter,
            value.len(),
            value.as_mut_ptr().cast(),
            ptr::null_mut(),
        )
    };
    check("clGetPlatformInfo(value)", status)?;
    Ok(nul_terminated_string(value))
}

fn device_string(device: ffi::ClDeviceId, parameter: ffi::ClDeviceInfo) -> Result<String, Error> {
    let mut bytes = 0;
    // SAFETY: count-only query writes one usize.
    let status = unsafe { ffi::clGetDeviceInfo(device, parameter, 0, ptr::null_mut(), &mut bytes) };
    check("clGetDeviceInfo(size)", status)?;
    let mut value = bounded_info_buffer("OpenCL device info", bytes)?;
    // SAFETY: value has the reported number of writable bytes.
    let status = unsafe {
        ffi::clGetDeviceInfo(
            device,
            parameter,
            value.len(),
            value.as_mut_ptr().cast(),
            ptr::null_mut(),
        )
    };
    check("clGetDeviceInfo(value)", status)?;
    Ok(nul_terminated_string(value))
}

fn device_u32(device: ffi::ClDeviceId, parameter: ffi::ClDeviceInfo) -> Result<u32, Error> {
    let mut value = 0u32;
    // SAFETY: value is a writable u32 of the requested property's type.
    let status = unsafe {
        ffi::clGetDeviceInfo(
            device,
            parameter,
            size_of::<u32>(),
            (&mut value as *mut u32).cast(),
            ptr::null_mut(),
        )
    };
    check("clGetDeviceInfo(u32)", status)?;
    Ok(value)
}

fn program_build_log(program: ffi::ClProgram, device: ffi::ClDeviceId) -> Result<String, Error> {
    let mut bytes = 0;
    // SAFETY: count-only query writes one usize.
    let status = unsafe {
        ffi::clGetProgramBuildInfo(
            program,
            device,
            ffi::CL_PROGRAM_BUILD_LOG,
            0,
            ptr::null_mut(),
            &mut bytes,
        )
    };
    check("clGetProgramBuildInfo(size)", status)?;
    if bytes > MAX_BUILD_LOG_BYTES {
        return Err(Error::LimitExceeded {
            resource: "OpenCL build log bytes",
            actual: bytes,
            maximum: MAX_BUILD_LOG_BYTES,
        });
    }
    if bytes == 0 {
        return Ok(String::new());
    }
    let mut log = vec![0u8; bytes];
    // SAFETY: log has the exact number of writable bytes reported by OpenCL.
    let status = unsafe {
        ffi::clGetProgramBuildInfo(
            program,
            device,
            ffi::CL_PROGRAM_BUILD_LOG,
            log.len(),
            log.as_mut_ptr().cast(),
            ptr::null_mut(),
        )
    };
    check("clGetProgramBuildInfo(value)", status)?;
    Ok(nul_terminated_string(log))
}

fn build_options(options: &str) -> Result<CString, Error> {
    if options.len() > MAX_BUILD_OPTIONS_BYTES {
        return Err(Error::LimitExceeded {
            resource: "build option bytes",
            actual: options.len(),
            maximum: MAX_BUILD_OPTIONS_BYTES,
        });
    }
    CString::new(options).map_err(|_| Error::InteriorNul {
        field: "build options",
    })
}

fn bounded_info_buffer(resource: &'static str, bytes: usize) -> Result<Vec<u8>, Error> {
    if bytes == 0 || bytes > MAX_INFO_BYTES {
        return Err(Error::LimitExceeded {
            resource,
            actual: bytes,
            maximum: MAX_INFO_BYTES,
        });
    }
    Ok(vec![0; bytes])
}

fn nul_terminated_string(mut bytes: Vec<u8>) -> String {
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn checked_range(
    operation: &'static str,
    offset: usize,
    bytes: usize,
    buffer_len: usize,
) -> Result<(), Error> {
    let end = offset
        .checked_add(bytes)
        .ok_or(Error::RangeOverflow { operation })?;
    if end > buffer_len {
        return Err(Error::BufferRange {
            operation,
            offset,
            end,
            buffer_len,
        });
    }
    Ok(())
}

fn validate_work_sizes(
    global_offset: Option<&[usize]>,
    global: &[usize],
    local: Option<&[usize]>,
) -> Result<(), Error> {
    if global.is_empty() || global.len() > MAX_WORK_DIMENSIONS {
        return Err(Error::InvalidWorkDimensions);
    }
    if global_offset.is_some_and(|offset| offset.len() != global.len()) {
        return Err(Error::GlobalOffsetDimensionMismatch);
    }
    let mut total = 1usize;
    for (dimension, value) in global.iter().copied().enumerate() {
        if value == 0 {
            return Err(Error::ZeroGlobalWorkSize { dimension });
        }
        total = total.checked_mul(value).ok_or(Error::LimitExceeded {
            resource: "global work item count",
            actual: usize::MAX,
            maximum: MAX_GLOBAL_WORK_ITEMS,
        })?;
        if total > MAX_GLOBAL_WORK_ITEMS {
            return Err(Error::LimitExceeded {
                resource: "global work item count",
                actual: total,
                maximum: MAX_GLOBAL_WORK_ITEMS,
            });
        }
    }
    if let Some(local) = local {
        if local.len() != global.len() {
            return Err(Error::LocalWorkDimensionMismatch);
        }
        for (dimension, (&global, &local)) in global.iter().zip(local).enumerate() {
            if local == 0 {
                return Err(Error::ZeroLocalWorkSize { dimension });
            }
            if global % local != 0 {
                return Err(Error::NonDivisibleLocalWorkSize {
                    dimension,
                    global,
                    local,
                });
            }
        }
    }
    Ok(())
}

fn ensure_same_session(
    expected: &Rc<SessionState>,
    actual: &Rc<SessionState>,
    object: &'static str,
) -> Result<(), Error> {
    if Rc::ptr_eq(expected, actual) {
        Ok(())
    } else {
        Err(Error::SessionMismatch { object })
    }
}

fn check(operation: &'static str, status: ffi::ClInt) -> Result<(), Error> {
    if status == ffi::CL_SUCCESS {
        Ok(())
    } else {
        Err(Error::api(operation, status))
    }
}

fn release(
    counters: &ObjectCounters,
    counter: &std::sync::atomic::AtomicUsize,
    status: ffi::ClInt,
) {
    if status != ffi::CL_SUCCESS {
        counters.release_errors.fetch_add(1, Ordering::AcqRel);
    }
    counter.fetch_sub(1, Ordering::AcqRel);
}
