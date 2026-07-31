use crate::{
    AdapterReport, DispatchDescriptor, DispatchReport, Error, NagaDispatchDescriptor, ObjectCounts,
    ValidatedNagaModule,
};

pub struct Session {
    _private: (),
}

impl Session {
    pub fn new_metal() -> Result<Self, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn enumerate_metal_adapters() -> Result<Vec<AdapterReport>, Error> {
        Err(Error::UnsupportedPlatform)
    }

    pub fn select_metal(_index: usize) -> Result<Self, Error> {
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

    pub fn dispatch_naga(
        &self,
        _module: &ValidatedNagaModule,
        _descriptor: &NagaDispatchDescriptor<'_>,
    ) -> Result<DispatchReport, Error> {
        Err(Error::UnsupportedPlatform)
    }
}
