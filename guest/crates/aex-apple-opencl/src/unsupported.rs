use crate::{BufferAccess, Error, GpuDevice, KernelScalar, ObjectTracker};

pub struct Session;
pub struct Buffer;
pub struct Program;
pub struct Kernel;

pub fn enumerate_gpu_devices() -> Result<Vec<GpuDevice>, Error> {
    Err(Error::UnsupportedPlatform)
}

impl Session {
    pub fn select_gpu(_index: usize) -> Result<Self, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn device(&self) -> &GpuDevice {
        panic!("an OpenCL session cannot exist on this platform")
    }

    pub fn object_tracker(&self) -> ObjectTracker {
        ObjectTracker::default()
    }

    pub fn create_buffer(&self, _bytes: usize, _access: BufferAccess) -> Result<Buffer, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn write_buffer(
        &self,
        _buffer: &Buffer,
        _offset: usize,
        _source: &[u8],
    ) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn read_buffer(
        &self,
        _buffer: &Buffer,
        _offset: usize,
        _destination: &mut [u8],
    ) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn build_program(&self, _source: &str, _options: Option<&str>) -> Result<Program, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn enqueue_nd_range(
        &self,
        _kernel: &Kernel,
        _global: &[usize],
        _local: Option<&[usize]>,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn finish(&self) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }
}

impl Buffer {
    pub const fn len(&self) -> usize {
        0
    }

    pub const fn is_empty(&self) -> bool {
        true
    }

    pub const fn access(&self) -> BufferAccess {
        BufferAccess::ReadWrite
    }
}

impl Program {
    pub fn build_log(&self) -> &str {
        ""
    }

    pub fn create_kernel(&self, _name: &str) -> Result<Kernel, Error> {
        Err(Error::UnsupportedPlatform)
    }
}

impl Kernel {
    pub fn set_scalar_arg<T: KernelScalar>(&mut self, _index: u32, _value: T) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn set_buffer_arg(&mut self, _index: u32, _buffer: &Buffer) -> Result<(), Error> {
        Err(Error::UnsupportedPlatform)
    }
}
