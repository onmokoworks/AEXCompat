use crate::{AdapterReport, DispatchDescriptor, DispatchReport, Error, ObjectCounts};

pub struct Session {
    _private: (),
}

impl Session {
    pub fn new_metal() -> Result<Self, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn adapter_report(&self) -> &AdapterReport {
        unreachable!("unsupported platforms cannot construct a wgpu Metal session")
    }

    pub fn live_objects(&self) -> ObjectCounts {
        ObjectCounts::default()
    }

    pub fn dispatch(&self, _descriptor: &DispatchDescriptor<'_>) -> Result<DispatchReport, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
