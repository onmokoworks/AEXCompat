use crate::runtime_module_policy::{GpuPlatformIdentity, RuntimeBackend};
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuAdapterObservation {
    pub adapter_luid: u64,
    pub pci_vendor_id: u16,
    pub pci_device_id: u16,
    pub pci_subsystem_id: u32,
    pub pci_revision_id: u8,
}

/// Enumerates hardware adapters through DXGI. The LUID and PCI identity come
/// from the same DXGI descriptor; software adapters are excluded.
pub fn enumerate_gpu_adapters() -> io::Result<Vec<GpuAdapterObservation>> {
    platform::enumerate_gpu_adapters()
}

/// Collects the active Windows display-driver package for one exact DXGI LUID.
///
/// The collector opens that LUID through D3DKMT, binds it to a PCI
/// bus/device/function address, requires one matching present SetupAPI display
/// device, resolves that device's active driver key and DriverStore INF, and
/// proves the INF is a member of the signed installed catalog. Only the INF
/// basename and catalog SHA-256 leave this boundary; private absolute paths do
/// not enter the policy identity.
pub fn collect_gpu_platform_identity(
    adapter_luid: u64,
    backend: RuntimeBackend,
) -> io::Result<GpuPlatformIdentity> {
    if backend == RuntimeBackend::Cpu {
        return Err(invalid("GPU platform collector rejects the CPU backend"));
    }
    platform::collect_gpu_platform_identity(adapter_luid, backend)
}

/// Emits only shareable adapter/package identity. Absolute INF, catalog, and
/// DriverStore paths stay inside the collector and are never serialized.
pub fn privacy_bounded_identity_report(identity: &GpuPlatformIdentity) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "backend": match identity.backend {
            RuntimeBackend::Cpu => "cpu",
            RuntimeBackend::Cuda => "cuda",
            RuntimeBackend::Opencl => "opencl",
            RuntimeBackend::Directx => "directx",
            RuntimeBackend::Opengl => "opengl",
        },
        "adapter_luid": format!("{:016x}", identity.adapter_luid),
        "pci_vendor_id": format!("{:04x}", identity.pci_vendor_id),
        "pci_device_id": format!("{:04x}", identity.pci_device_id),
        "pci_subsystem_id": format!("{:08x}", identity.pci_subsystem_id),
        "pci_revision_id": format!("{:02x}", identity.pci_revision_id),
        "driver_inf": identity.driver_inf,
        "driver_catalog_sha256": hex(&identity.driver_catalog_sha256),
        "driver_version": identity.driver_version,
        "os_build": identity.os_build,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn enumerate_gpu_adapters() -> io::Result<Vec<GpuAdapterObservation>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "GPU platform collection requires Windows",
        ))
    }

    pub fn collect_gpu_platform_identity(
        _: u64,
        _: RuntimeBackend,
    ) -> io::Result<GpuPlatformIdentity> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "GPU platform collection requires Windows",
        ))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::ffi::OsStr;
    use std::fs::{self, File, OpenOptions};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use windows::Wdk::Graphics::Direct3D::{
        D3DKMT_ADAPTERADDRESS, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID,
        D3DKMT_PHYSICAL_ADAPTER_COUNT, D3DKMT_QUERY_DEVICE_IDS, D3DKMT_QUERYADAPTERINFO,
        D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo,
        KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_PHYSICALADAPTERCOUNT,
        KMTQAITYPE_PHYSICALADAPTERDEVICEIDS, KMTQUERYADAPTERINFOTYPE,
    };
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        DICS_FLAG_GLOBAL, DIGCF_PRESENT, DIREG_DRV, GUID_DEVCLASS_DISPLAY, HDEVINFO,
        SETUP_DI_REGISTRY_PROPERTY, SP_DEVINFO_DATA, SP_INF_SIGNER_INFO_V2_W, SPDRP_ADDRESS,
        SPDRP_BUSNUMBER, SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
        SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, SetupDiOpenDevRegKey,
        SetupGetInfDriverStoreLocationW, SetupGetInfPublishedNameW, SetupVerifyInfFileW,
    };
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND, LUID, MAX_PATH, NTSTATUS};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, IDXGIFactory1,
    };
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    use windows::Win32::System::Registry::{HKEY, KEY_READ, REG_SZ, RegCloseKey, RegQueryValueExW};
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    use windows::core::{PCWSTR, w};
    use windows_sys::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE, WTD_REVOKE_NONE,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
        WinVerifyTrust,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    const PCI_BUS_TYPES: std::ops::RangeInclusive<u32> = 1..=3;
    const MAX_ADAPTERS: u32 = 64;
    const MAX_PROPERTY_BYTES: usize = 16 * 1024;

    pub fn enumerate_gpu_adapters() -> io::Result<Vec<GpuAdapterObservation>> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(win_error)?;
        let mut adapters = Vec::new();
        for ordinal in 0..MAX_ADAPTERS {
            let adapter = match unsafe { factory.EnumAdapters1(ordinal) } {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(win_error(error)),
            };
            let desc = unsafe { adapter.GetDesc1() }.map_err(win_error)?;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let observation = GpuAdapterObservation {
                adapter_luid: luid_u64(desc.AdapterLuid),
                pci_vendor_id: narrow_u16("DXGI vendor id", desc.VendorId)?,
                pci_device_id: narrow_u16("DXGI device id", desc.DeviceId)?,
                pci_subsystem_id: desc.SubSysId,
                pci_revision_id: narrow_u8("DXGI revision id", desc.Revision)?,
            };
            if observation.adapter_luid == 0
                || observation.pci_vendor_id == 0
                || observation.pci_device_id == 0
            {
                continue;
            }
            adapters.push(observation);
        }
        if adapters.is_empty() {
            return Err(invalid("DXGI reported no hardware GPU adapters"));
        }
        Ok(adapters)
    }

    pub fn collect_gpu_platform_identity(
        adapter_luid: u64,
        backend: RuntimeBackend,
    ) -> io::Result<GpuPlatformIdentity> {
        let dxgi = enumerate_gpu_adapters()?
            .into_iter()
            .find(|adapter| adapter.adapter_luid == adapter_luid)
            .ok_or_else(|| {
                invalid("requested adapter LUID is not a current DXGI hardware adapter")
            })?;
        let adapter = KmtAdapter::open(adapter_luid)?;
        let count: D3DKMT_PHYSICAL_ADAPTER_COUNT =
            adapter.query(KMTQAITYPE_PHYSICALADAPTERCOUNT)?;
        if count.Count != 1 {
            return Err(invalid(
                "linked or multi-physical DXGI adapters are not supported by this collector",
            ));
        }
        let address: D3DKMT_ADAPTERADDRESS = adapter.query(KMTQAITYPE_ADAPTERADDRESS)?;
        let device_ids: D3DKMT_QUERY_DEVICE_IDS =
            adapter.query(KMTQAITYPE_PHYSICALADAPTERDEVICEIDS)?;
        let ids = device_ids.DeviceIds;
        if !PCI_BUS_TYPES.contains(&ids.BusType) {
            return Err(invalid(format!(
                "requested DXGI adapter is not a PCI display adapter (bus type {})",
                ids.BusType
            )));
        }
        let d3dkmt_subsystem = (ids.SubSystemID << 16) | ids.SubVendorID;
        if ids.VendorID != u32::from(dxgi.pci_vendor_id)
            || ids.DeviceID != u32::from(dxgi.pci_device_id)
            || d3dkmt_subsystem != dxgi.pci_subsystem_id
            || ids.RevisionID != u32::from(dxgi.pci_revision_id)
        {
            return Err(invalid("DXGI and D3DKMT PCI identities do not match"));
        }

        let package = find_active_driver_package(address, dxgi)?;
        let catalog_sha256 = verified_inf_catalog_digest(&package.driver_store_inf)?;
        let os_build = os_build()?;
        Ok(GpuPlatformIdentity {
            backend,
            adapter_luid,
            pci_vendor_id: dxgi.pci_vendor_id,
            pci_device_id: dxgi.pci_device_id,
            pci_subsystem_id: dxgi.pci_subsystem_id,
            pci_revision_id: dxgi.pci_revision_id,
            driver_inf: package.inf_basename,
            driver_catalog_sha256: catalog_sha256,
            driver_version: package.driver_version,
            os_build,
        })
    }

    struct KmtAdapter(u32);

    impl KmtAdapter {
        fn open(luid: u64) -> io::Result<Self> {
            let mut open = D3DKMT_OPENADAPTERFROMLUID {
                AdapterLuid: u64_luid(luid),
                ..Default::default()
            };
            nt_ok(
                unsafe { D3DKMTOpenAdapterFromLuid(&mut open) },
                "D3DKMTOpenAdapterFromLuid",
            )?;
            if open.hAdapter == 0 {
                return Err(invalid("D3DKMT returned a null adapter handle"));
            }
            Ok(Self(open.hAdapter))
        }

        fn query<T: Default>(&self, kind: KMTQUERYADAPTERINFOTYPE) -> io::Result<T> {
            let mut value = T::default();
            let mut query = D3DKMT_QUERYADAPTERINFO {
                hAdapter: self.0,
                Type: kind,
                pPrivateDriverData: (&mut value as *mut T).cast(),
                PrivateDriverDataSize: size_of::<T>() as u32,
            };
            nt_ok(
                unsafe { D3DKMTQueryAdapterInfo(&mut query) },
                "D3DKMTQueryAdapterInfo",
            )?;
            Ok(value)
        }
    }

    impl Drop for KmtAdapter {
        fn drop(&mut self) {
            let close = D3DKMT_CLOSEADAPTER { hAdapter: self.0 };
            unsafe {
                let _ = D3DKMTCloseAdapter(&close);
            }
        }
    }

    struct DeviceInfoSet(HDEVINFO);

    impl Drop for DeviceInfoSet {
        fn drop(&mut self) {
            unsafe {
                let _ = SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    struct RegistryKey(HKEY);

    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    struct DriverPackage {
        inf_basename: String,
        driver_version: String,
        driver_store_inf: PathBuf,
    }

    fn find_active_driver_package(
        address: D3DKMT_ADAPTERADDRESS,
        expected: GpuAdapterObservation,
    ) -> io::Result<DriverPackage> {
        let set = DeviceInfoSet(
            unsafe {
                SetupDiGetClassDevsW(
                    Some(&GUID_DEVCLASS_DISPLAY),
                    PCWSTR::null(),
                    HWND::default(),
                    DIGCF_PRESENT,
                )
            }
            .map_err(win_error)?,
        );
        let expected_address = (address.DeviceNumber << 16) | address.FunctionNumber;
        let mut matches = Vec::new();
        for index in 0..1024 {
            let mut device = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..unsafe { zeroed() }
            };
            match unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) } {
                Ok(()) => {}
                Err(error) if is_no_more_items(&error) => break,
                Err(error) => return Err(win_error(error)),
            }
            if property_u32(set.0, &device, SPDRP_BUSNUMBER).ok() != Some(address.BusNumber)
                || property_u32(set.0, &device, SPDRP_ADDRESS).ok() != Some(expected_address)
            {
                continue;
            }
            let hardware_ids = property_strings(set.0, &device, SPDRP_HARDWAREID)?;
            if !hardware_ids
                .iter()
                .any(|id| hardware_id_matches(id, expected))
            {
                continue;
            }
            matches.push(device);
        }
        if matches.len() != 1 {
            return Err(invalid(format!(
                "expected one present SetupAPI display device for the adapter address; found {}",
                matches.len()
            )));
        }
        let device = matches.pop().unwrap();
        let key = RegistryKey(
            unsafe {
                SetupDiOpenDevRegKey(set.0, &device, DICS_FLAG_GLOBAL.0, 0, DIREG_DRV, KEY_READ.0)
            }
            .map_err(win_error)?,
        );
        let inf_basename = registry_string(key.0, w!("InfPath"))?;
        if !is_safe_inf_basename(&inf_basename) {
            return Err(invalid("active display driver has an unsafe INF basename"));
        }
        let driver_version = registry_string(key.0, w!("DriverVersion"))?;
        if !valid_driver_version(&driver_version) {
            return Err(invalid("active display driver has an invalid version"));
        }
        let windows = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| invalid("SystemRoot is not set"))?;
        let published_inf = windows.join("INF").join(&inf_basename);
        let driver_store_inf = driver_store_location(&published_inf)?;
        let published_again = inf_published_name(&driver_store_inf)?;
        if !published_again
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(&inf_basename))
        {
            return Err(invalid(
                "DriverStore INF does not map back to the active published INF",
            ));
        }
        Ok(DriverPackage {
            inf_basename,
            driver_version,
            driver_store_inf,
        })
    }

    fn property_u32(
        set: HDEVINFO,
        device: &SP_DEVINFO_DATA,
        property: SETUP_DI_REGISTRY_PROPERTY,
    ) -> io::Result<u32> {
        let mut bytes = [0u8; 4];
        let mut value_type = 0u32;
        let mut required = 0u32;
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                device,
                property,
                Some(&mut value_type),
                Some(&mut bytes),
                Some(&mut required),
            )
        }
        .map_err(win_error)?;
        if required != 4 || value_type != windows::Win32::System::Registry::REG_DWORD.0 {
            return Err(invalid("SetupAPI numeric property has the wrong type"));
        }
        Ok(u32::from_ne_bytes(bytes))
    }

    fn property_strings(
        set: HDEVINFO,
        device: &SP_DEVINFO_DATA,
        property: SETUP_DI_REGISTRY_PROPERTY,
    ) -> io::Result<Vec<String>> {
        let mut bytes = vec![0u8; MAX_PROPERTY_BYTES];
        let mut value_type = 0u32;
        let mut required = 0u32;
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                device,
                property,
                Some(&mut value_type),
                Some(&mut bytes),
                Some(&mut required),
            )
        }
        .map_err(win_error)?;
        if required as usize > bytes.len()
            || required % 2 != 0
            || value_type != windows::Win32::System::Registry::REG_MULTI_SZ.0
        {
            return Err(invalid("SetupAPI string-list property has the wrong type"));
        }
        bytes.truncate(required as usize);
        let words = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let mut values = Vec::new();
        for value in words.split(|word| *word == 0) {
            if !value.is_empty() {
                values.push(
                    String::from_utf16(value).map_err(|_| invalid("invalid UTF-16 property"))?,
                );
            }
        }
        Ok(values)
    }

    fn registry_string(key: HKEY, name: PCWSTR) -> io::Result<String> {
        let mut value_type = windows::Win32::System::Registry::REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                name,
                None,
                Some(&mut value_type),
                None,
                Some(&mut size),
            )
        };
        if !status.is_ok() {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        if value_type != REG_SZ || size < 2 || size as usize > MAX_PROPERTY_BYTES || size % 2 != 0 {
            return Err(invalid("driver registry value has the wrong type or size"));
        }
        let mut bytes = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key,
                name,
                None,
                Some(&mut value_type),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if !status.is_ok() {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        let words = bytes[..size as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .take_while(|word| *word != 0)
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| invalid("driver registry value is invalid UTF-16"))
    }

    fn driver_store_location(published_inf: &Path) -> io::Result<PathBuf> {
        let input = wide_null(published_inf.as_os_str());
        let mut output = vec![0u16; MAX_PATH as usize * 4];
        unsafe {
            SetupGetInfDriverStoreLocationW(
                PCWSTR(input.as_ptr()),
                None,
                PCWSTR::null(),
                &mut output,
                None,
            )
        }
        .map_err(win_error)?;
        canonical_absolute(&PathBuf::from(std::ffi::OsString::from_wide(nul_slice(
            &output,
        ))))
    }

    fn inf_published_name(driver_store_inf: &Path) -> io::Result<PathBuf> {
        let input = wide_null(driver_store_inf.as_os_str());
        let mut output = vec![0u16; MAX_PATH as usize * 4];
        unsafe { SetupGetInfPublishedNameW(PCWSTR(input.as_ptr()), &mut output, None) }
            .map_err(win_error)?;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(nul_slice(
            &output,
        ))))
    }

    fn verified_inf_catalog_digest(inf: &Path) -> io::Result<[u8; 32]> {
        let inf = canonical_absolute(inf)?;
        let parent = inf
            .parent()
            .ok_or_else(|| invalid("DriverStore INF has no parent"))?;
        let inf_wide = wide_null(inf.as_os_str());
        let mut signer = SP_INF_SIGNER_INFO_V2_W {
            cbSize: size_of::<SP_INF_SIGNER_INFO_V2_W>() as u32,
            ..Default::default()
        };
        if !unsafe { SetupVerifyInfFileW(PCWSTR(inf_wide.as_ptr()), None, &mut signer) }.as_bool() {
            return Err(io::Error::last_os_error());
        }
        let catalog = PathBuf::from(std::ffi::OsString::from_wide(nul_slice(
            &signer.CatalogFile,
        )));
        let basename = catalog
            .file_name()
            .ok_or_else(|| invalid("verified driver catalog has no basename"))?;
        if basename != catalog.as_os_str() && catalog.parent() != Some(parent) {
            return Err(invalid(
                "verified driver catalog escaped the DriverStore package",
            ));
        }
        let catalog = canonical_absolute(&parent.join(basename))?;
        if catalog.parent() != Some(parent) {
            return Err(invalid("driver catalog escaped the DriverStore package"));
        }
        verify_signed_catalog(&catalog)?;
        hash_regular_file(&catalog)
    }

    fn verify_signed_catalog(catalog: &Path) -> io::Result<()> {
        let catalog_wide = wide_null(catalog.as_os_str());
        let file = open_read(catalog)?;
        let mut file_info = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: catalog_wide.as_ptr(),
            hFile: file.as_raw_handle() as _,
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            pPolicyCallbackData: std::ptr::null_mut(),
            pSIPClientData: std::ptr::null_mut(),
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 {
                pFile: &mut file_info,
            },
            dwStateAction: WTD_STATEACTION_VERIFY,
            hWVTStateData: std::ptr::null_mut(),
            pwszURLReference: std::ptr::null_mut(),
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
            dwUIContext: WTD_UICONTEXT_EXECUTE,
            pSignatureSettings: std::ptr::null_mut(),
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status =
            unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as _) };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        unsafe {
            WinVerifyTrust(std::ptr::null_mut(), &mut action, &mut data as *mut _ as _);
        }
        if status != 0 {
            return Err(invalid(format!(
                "active driver package catalog verification failed: 0x{:08x}",
                status as u32
            )));
        }
        Ok(())
    }

    fn os_build() -> io::Result<u32> {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        nt_ok(unsafe { RtlGetVersion(&mut version) }, "RtlGetVersion")?;
        if version.dwBuildNumber == 0 {
            return Err(invalid("RtlGetVersion returned a zero OS build"));
        }
        Ok(version.dwBuildNumber)
    }

    fn hash_regular_file(path: &Path) -> io::Result<[u8; 32]> {
        let mut file = open_read(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(invalid("driver package member is not a regular file"));
        }
        let mut hash = Sha256::new();
        io::copy(&mut file, &mut hash)?;
        Ok(hash.finalize().into())
    }

    fn open_read(path: &Path) -> io::Result<File> {
        reject_reparse_components(path)?;
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    fn canonical_absolute(path: &Path) -> io::Result<PathBuf> {
        if !path.is_absolute() {
            return Err(invalid("driver package path is not absolute"));
        }
        reject_reparse_components(path)?;
        let canonical = fs::canonicalize(path)?;
        reject_reparse_components(&canonical)?;
        Ok(canonical)
    }

    fn reject_reparse_components(path: &Path) -> io::Result<()> {
        for component in path.ancestors().filter(|part| !part.as_os_str().is_empty()) {
            let metadata = fs::symlink_metadata(component)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err(invalid("driver package path contains a reparse point"));
            }
        }
        Ok(())
    }

    fn hardware_id_matches(value: &str, expected: GpuAdapterObservation) -> bool {
        let value = value.to_ascii_lowercase();
        value.starts_with("pci\\")
            && value.contains(&format!("ven_{:04x}", expected.pci_vendor_id))
            && value.contains(&format!("dev_{:04x}", expected.pci_device_id))
            && value.contains(&format!("subsys_{:08x}", expected.pci_subsystem_id))
            && value.contains(&format!("rev_{:02x}", expected.pci_revision_id))
    }

    fn is_safe_inf_basename(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 255
            && value.to_ascii_lowercase().ends_with(".inf")
            && !value.contains(['/', '\\', ':'])
            && !value.chars().any(char::is_control)
    }

    fn valid_driver_version(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 64
            && value
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }

    fn luid_u64(luid: LUID) -> u64 {
        (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart)
    }

    fn u64_luid(value: u64) -> LUID {
        LUID {
            LowPart: value as u32,
            HighPart: (value >> 32) as u32 as i32,
        }
    }

    fn narrow_u16(kind: &str, value: u32) -> io::Result<u16> {
        u16::try_from(value).map_err(|_| invalid(format!("{kind} exceeds 16 bits")))
    }

    fn narrow_u8(kind: &str, value: u32) -> io::Result<u8> {
        u8::try_from(value).map_err(|_| invalid(format!("{kind} exceeds 8 bits")))
    }

    fn nt_ok(status: NTSTATUS, operation: &str) -> io::Result<()> {
        if status.0 < 0 {
            Err(invalid(format!(
                "{operation} failed with NTSTATUS 0x{:08x}",
                status.0 as u32
            )))
        } else {
            Ok(())
        }
    }

    fn win_error(error: windows::core::Error) -> io::Error {
        io::Error::other(error.to_string())
    }

    fn is_no_more_items(error: &windows::core::Error) -> bool {
        error.code().0 as u32 == 0x8007_0000 | ERROR_NO_MORE_ITEMS.0
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    fn nul_slice(value: &[u16]) -> &[u16] {
        &value[..value
            .iter()
            .position(|word| *word == 0)
            .unwrap_or(value.len())]
    }
}
