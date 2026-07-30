use std::ffi::{c_char, c_void};

pub type ClInt = i32;
pub type ClUint = u32;
pub type ClUlong = u64;
pub type ClBitfield = ClUlong;
pub type ClDeviceType = ClBitfield;
pub type ClMemFlags = ClBitfield;
pub type ClCommandQueueProperties = ClBitfield;
pub type ClContextProperties = isize;
pub type ClBool = ClUint;
pub type ClPlatformInfo = ClUint;
pub type ClDeviceInfo = ClUint;
pub type ClProgramBuildInfo = ClUint;

pub enum ClPlatformIdOpaque {}
pub enum ClDeviceIdOpaque {}
pub enum ClContextOpaque {}
pub enum ClCommandQueueOpaque {}
pub enum ClMemOpaque {}
pub enum ClProgramOpaque {}
pub enum ClKernelOpaque {}
pub enum ClEventOpaque {}

pub type ClPlatformId = *mut ClPlatformIdOpaque;
pub type ClDeviceId = *mut ClDeviceIdOpaque;
pub type ClContext = *mut ClContextOpaque;
pub type ClCommandQueue = *mut ClCommandQueueOpaque;
pub type ClMem = *mut ClMemOpaque;
pub type ClProgram = *mut ClProgramOpaque;
pub type ClKernel = *mut ClKernelOpaque;
pub type ClEvent = *mut ClEventOpaque;

pub const CL_SUCCESS: ClInt = 0;
pub const CL_DEVICE_NOT_FOUND: ClInt = -1;
pub const CL_DEVICE_TYPE_GPU: ClDeviceType = 1 << 2;
pub const CL_TRUE: ClBool = 1;

pub const CL_PLATFORM_NAME: ClPlatformInfo = 0x0902;
pub const CL_DEVICE_MAX_COMPUTE_UNITS: ClDeviceInfo = 0x1002;
pub const CL_DEVICE_NAME: ClDeviceInfo = 0x102B;
pub const CL_DEVICE_VENDOR: ClDeviceInfo = 0x102C;
pub const CL_CONTEXT_PLATFORM: ClContextProperties = 0x1084;
pub const CL_PROGRAM_BUILD_LOG: ClProgramBuildInfo = 0x1183;

pub const CL_MEM_READ_WRITE: ClMemFlags = 1 << 0;
pub const CL_MEM_WRITE_ONLY: ClMemFlags = 1 << 1;
pub const CL_MEM_READ_ONLY: ClMemFlags = 1 << 2;

type ContextNotify = Option<
    unsafe extern "C" fn(
        error_info: *const c_char,
        private_info: *const c_void,
        private_info_size: usize,
        user_data: *mut c_void,
    ),
>;
type ProgramNotify = Option<unsafe extern "C" fn(program: ClProgram, user_data: *mut c_void)>;

#[link(name = "OpenCL", kind = "framework")]
unsafe extern "C" {
    pub fn clGetPlatformIDs(
        num_entries: ClUint,
        platforms: *mut ClPlatformId,
        num_platforms: *mut ClUint,
    ) -> ClInt;
    pub fn clGetPlatformInfo(
        platform: ClPlatformId,
        param_name: ClPlatformInfo,
        param_value_size: usize,
        param_value: *mut c_void,
        param_value_size_ret: *mut usize,
    ) -> ClInt;
    pub fn clGetDeviceIDs(
        platform: ClPlatformId,
        device_type: ClDeviceType,
        num_entries: ClUint,
        devices: *mut ClDeviceId,
        num_devices: *mut ClUint,
    ) -> ClInt;
    pub fn clGetDeviceInfo(
        device: ClDeviceId,
        param_name: ClDeviceInfo,
        param_value_size: usize,
        param_value: *mut c_void,
        param_value_size_ret: *mut usize,
    ) -> ClInt;
    pub fn clCreateContext(
        properties: *const ClContextProperties,
        num_devices: ClUint,
        devices: *const ClDeviceId,
        notify: ContextNotify,
        user_data: *mut c_void,
        status: *mut ClInt,
    ) -> ClContext;
    pub fn clReleaseContext(context: ClContext) -> ClInt;
    pub fn clCreateCommandQueue(
        context: ClContext,
        device: ClDeviceId,
        properties: ClCommandQueueProperties,
        status: *mut ClInt,
    ) -> ClCommandQueue;
    pub fn clReleaseCommandQueue(queue: ClCommandQueue) -> ClInt;
    pub fn clCreateBuffer(
        context: ClContext,
        flags: ClMemFlags,
        size: usize,
        host_ptr: *mut c_void,
        status: *mut ClInt,
    ) -> ClMem;
    pub fn clReleaseMemObject(memory: ClMem) -> ClInt;
    pub fn clEnqueueWriteBuffer(
        queue: ClCommandQueue,
        buffer: ClMem,
        blocking: ClBool,
        offset: usize,
        size: usize,
        source: *const c_void,
        event_wait_count: ClUint,
        event_wait_list: *const ClEvent,
        event: *mut ClEvent,
    ) -> ClInt;
    pub fn clEnqueueReadBuffer(
        queue: ClCommandQueue,
        buffer: ClMem,
        blocking: ClBool,
        offset: usize,
        size: usize,
        destination: *mut c_void,
        event_wait_count: ClUint,
        event_wait_list: *const ClEvent,
        event: *mut ClEvent,
    ) -> ClInt;
    pub fn clCreateProgramWithSource(
        context: ClContext,
        count: ClUint,
        strings: *const *const c_char,
        lengths: *const usize,
        status: *mut ClInt,
    ) -> ClProgram;
    pub fn clBuildProgram(
        program: ClProgram,
        device_count: ClUint,
        devices: *const ClDeviceId,
        options: *const c_char,
        notify: ProgramNotify,
        user_data: *mut c_void,
    ) -> ClInt;
    pub fn clGetProgramBuildInfo(
        program: ClProgram,
        device: ClDeviceId,
        param_name: ClProgramBuildInfo,
        param_value_size: usize,
        param_value: *mut c_void,
        param_value_size_ret: *mut usize,
    ) -> ClInt;
    pub fn clReleaseProgram(program: ClProgram) -> ClInt;
    pub fn clCreateKernel(
        program: ClProgram,
        kernel_name: *const c_char,
        status: *mut ClInt,
    ) -> ClKernel;
    pub fn clReleaseKernel(kernel: ClKernel) -> ClInt;
    pub fn clSetKernelArg(
        kernel: ClKernel,
        argument_index: ClUint,
        argument_size: usize,
        argument_value: *const c_void,
    ) -> ClInt;
    pub fn clEnqueueNDRangeKernel(
        queue: ClCommandQueue,
        kernel: ClKernel,
        work_dimensions: ClUint,
        global_work_offset: *const usize,
        global_work_size: *const usize,
        local_work_size: *const usize,
        event_wait_count: ClUint,
        event_wait_list: *const ClEvent,
        event: *mut ClEvent,
    ) -> ClInt;
    pub fn clFinish(queue: ClCommandQueue) -> ClInt;
}
