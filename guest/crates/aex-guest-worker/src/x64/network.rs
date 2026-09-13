// IP_ADAPTER_ADDRESSES_XP (SP1): Length explicitly advertises this 184-byte
// version. Address lists/prefixes and Vista extensions remain separate gaps.
#[derive(Clone, Debug)]
struct WindowsAdapter {
    name: String,
    description: String,
    dns_suffix: String,
    index: u32,
    physical_address: Vec<u8>,
    flags: u32,
    mtu: u32,
    kind: u32,
    status: u32,
    ipv4: bool,
    ipv6: bool,
}

#[cfg(target_os = "macos")]
fn native_windows_adapters() -> Result<Vec<WindowsAdapter>, String> {
    let interfaces = aex_host_identity::adapters::interfaces()?;
    let config = aex_host_identity::network_configuration::snapshot()?;
    windows_adapters_from_native(interfaces, &config)
}

#[cfg(target_os = "macos")]
fn windows_adapters_from_native(
    interfaces: Vec<aex_host_identity::adapters::Interface>,
    config: &aex_host_identity::network_configuration::Configuration,
) -> Result<Vec<WindowsAdapter>, String> {
    use aex_host_identity::network_configuration::{self, Property};
    let field = |key: &str, name: &str| -> Result<Option<String>, String> {
        match config.get(key).and_then(|record| record.get(name)) {
            None => Ok(None),
            Some(Property::Text(value)) => Ok(Some(value.clone())),
            Some(_) => Err("adapter configuration field is not text".into()),
        }
    };
    let mut result = Vec::new();
    for interface in interfaces {
        let name = String::from_utf8(interface.name).map_err(|_| "adapter name is not UTF-8")?;
        let services = network_configuration::interface_services(&config, &name)?;
        let mut descriptions = std::collections::BTreeSet::new();
        let mut suffixes = std::collections::BTreeSet::new();
        let mut wifi = false;
        let mut dhcp = false;
        let mut ipv4 = interface
            .addresses
            .iter()
            .any(|address| address.family == 2);
        let mut ipv6 = interface
            .addresses
            .iter()
            .any(|address| address.family == 30);
        for service in services {
            let setup = format!("Setup:/Network/Service/{service}");
            let state = format!("State:/Network/Service/{service}");
            if let Some(device) = field(&format!("{setup}/Interface"), "DeviceName")? {
                if device != name {
                    return Err("adapter setup and runtime devices differ; configuration translation is not implemented".into());
                }
            }
            if let Some(description) = field(&format!("{setup}/Interface"), "UserDefinedName")? {
                descriptions.insert(description);
            }
            wifi |= field(&format!("{setup}/Interface"), "Hardware")?.as_deref() == Some("AirPort");
            for (entity, enabled) in [("IPv4", &mut ipv4), ("IPv6", &mut ipv6)] {
                // If this service splits families over different interfaces,
                // only apply the configuration for this interface's family.
                if let Some(bound) = field(&format!("{state}/{entity}"), "InterfaceName")? {
                    if bound != name {
                        continue;
                    }
                } else if field(&format!("{setup}/Interface"), "DeviceName")?.as_deref()
                    != Some(&name)
                {
                    continue;
                }
                if let Some(method) = field(&format!("{setup}/{entity}"), "ConfigMethod")? {
                    match method.as_str() {
                        "Off" => {}
                        "Automatic" | "BOOTP" | "DHCP" | "INFORM" | "LinkLocal" | "Manual"
                        | "PPP" | "VPN" | "6to4" => *enabled = true,
                        _ => return Err("unsupported native adapter configuration method".into()),
                    }
                    dhcp |= entity == "IPv4" && method == "DHCP";
                }
            }
            // State is the effective resolver configuration. Fall back to
            // setup only when there is no state DNS dictionary at all.
            let dns = if config.contains_key(&format!("{state}/DNS")) {
                &state
            } else {
                &setup
            };
            if let Some(suffix) = field(&format!("{dns}/DNS"), "DomainName")? {
                suffixes.insert(suffix);
            }
        }
        if suffixes.len() > 1 {
            return Err("adapter has multiple DNS suffixes; precedence is not implemented".into());
        }
        let active = config
            .get(&format!("State:/Network/Interface/{name}/Link"))
            .and_then(|record| record.get("Active"));
        let status = if interface.native_flags & 1 == 0 {
            2
        } else {
            match active {
                Some(Property::Boolean(true)) => 1,
                Some(Property::Boolean(false)) => 2,
                None => 4, // IfOperStatusUnknown; IFF_RUNNING is not link status.
                _ => return Err("adapter link state is not boolean".into()),
            }
        };
        let mut flags = 0;
        if dhcp {
            flags |= 4;
        }
        if interface.native_flags & 0x8000 == 0 {
            flags |= 0x10;
        }
        if ipv4 {
            flags |= 0x80;
        }
        if ipv6 {
            flags |= 0x100;
        }
        // The guest provides no DDNS registration, NetBIOS or managed IPv6
        // configuration service; those capability bits remain clear.
        let kind = if wifi {
            71
        } else {
            match interface.native_type {
                6 => 6,
                24 => 24,
                23 => 23,
                // BSD type numbers outside these verified common IANA types
                // are not assumed to be Windows interface type numbers.
                _ => 1,
            }
        };
        result.push(WindowsAdapter {
            description: if descriptions.len() == 1 {
                descriptions.into_iter().next().unwrap()
            } else {
                name.clone()
            },
            dns_suffix: suffixes.into_iter().next().unwrap_or_default(),
            name,
            index: interface.index,
            physical_address: interface.physical_address,
            flags,
            mtu: interface.mtu,
            kind,
            status,
            ipv4,
            ipv6,
        });
    }
    Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn native_windows_adapters() -> Result<Vec<WindowsAdapter>, String> {
    Err("native Windows adapter translation is not implemented on this host".into())
}

fn serialize_windows_adapters(
    adapters: &[WindowsAdapter],
    base: u64,
    skip_friendly: bool,
) -> Result<Vec<u8>, String> {
    const HEADER: usize = 184;
    const LIMIT: usize = 4 * 1024 * 1024;
    if adapters.len() > 4096 {
        return Err("adapter count exceeds bound".into());
    }
    let mut bytes = vec![0; adapters.len() * HEADER];
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn put64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (i, adapter) in adapters.iter().enumerate() {
        if adapter.name.is_empty()
            || !adapter.name.is_ascii()
            || adapter.name.len() > 255
            || adapter.physical_address.len() > 8
            || adapter.index == 0
        {
            return Err("adapter identity cannot be represented in Windows ABI".into());
        }
        let at = i * HEADER;
        put32(&mut bytes, at, HEADER as u32);
        put32(
            &mut bytes,
            at + 4,
            if adapter.ipv4 { adapter.index } else { 0 },
        );
        if i + 1 < adapters.len() {
            put64(
                &mut bytes,
                at + 8,
                base.checked_add(((i + 1) * HEADER) as u64)
                    .ok_or("adapter pointer overflow")?,
            );
        }
        for (offset, value, wide) in [
            (16, &adapter.name, false),
            (56, &adapter.dns_suffix, true),
            (64, &adapter.description, true),
            (72, &adapter.description, true),
        ] {
            if offset == 72 && skip_friendly {
                continue;
            }
            if value.contains('\0') || value.encode_utf16().count() > 4096 {
                return Err("adapter string exceeds bound or contains NUL".into());
            }
            if wide && bytes.len() % 2 != 0 {
                bytes.push(0);
            }
            let pointer = base
                .checked_add(bytes.len() as u64)
                .ok_or("adapter pointer overflow")?;
            put64(&mut bytes, at + offset, pointer);
            if wide {
                for unit in value.encode_utf16().chain(std::iter::once(0)) {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
            } else {
                bytes.extend_from_slice(value.as_bytes());
                bytes.push(0);
            }
            if bytes.len() > LIMIT {
                return Err("adapter output exceeds bound".into());
            }
        }
        bytes[at + 80..at + 80 + adapter.physical_address.len()]
            .copy_from_slice(&adapter.physical_address);
        put32(&mut bytes, at + 88, adapter.physical_address.len() as u32);
        put32(&mut bytes, at + 92, adapter.flags);
        put32(&mut bytes, at + 96, adapter.mtu);
        put32(&mut bytes, at + 100, adapter.kind);
        put32(&mut bytes, at + 104, adapter.status);
        put32(
            &mut bytes,
            at + 108,
            if adapter.ipv6 { adapter.index } else { 0 },
        );
        if adapter.ipv6 {
            // IPv6 interface/link-local scope uses the native interface index.
            put32(&mut bytes, at + 112 + 4, adapter.index);
            put32(&mut bytes, at + 112 + 8, adapter.index);
        }
    }
    base.checked_add(bytes.len() as u64)
        .ok_or("adapter output range overflow")?;
    Ok(bytes)
}

fn guest_get_adapters_addresses(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let family = read_win64_import_argument(unicorn, 0)? as u32;
    let flags = read_win64_import_argument(unicorn, 1)? as u32;
    let reserved = read_win64_import_argument(unicorn, 2)?;
    let output = read_win64_import_argument(unicorn, 3)?;
    let size_pointer = read_win64_import_argument(unicorn, 4)?;
    if !matches!(family, 0 | 2 | 23) || size_pointer == 0 || reserved != 0 {
        return Ok(87);
    }
    // This path supplies adapter metadata when all address lists are skipped.
    // Requests for unicast/anycast/multicast/DNS/prefix lists remain explicit.
    if flags & 15 != 15 || flags & !(15 | 0x20 | 0x100) != 0 {
        return Err(
            "GetAdaptersAddresses address lists or requested flags are not implemented".into(),
        );
    }
    if !guest_range_has_permission(unicorn, size_pointer, 4, Prot::READ | Prot::WRITE)? {
        return Err("GetAdaptersAddresses size pointer is not readable/writable".into());
    }
    let mut size = [0; 4];
    unicorn
        .mem_read(size_pointer, &mut size)
        .map_err(|e| format!("adapter size read: {e}"))?;
    let adapters: Vec<_> = native_windows_adapters()?
        .into_iter()
        .filter(|a| {
            flags & 0x100 != 0
                || match family {
                    2 => a.ipv4,
                    23 => a.ipv6,
                    _ => a.ipv4 || a.ipv6,
                }
        })
        .collect();
    if adapters.is_empty() {
        return Ok(232);
    }
    let bytes = serialize_windows_adapters(&adapters, 0, flags & 0x20 != 0)?;
    if output == 0 || (u32::from_le_bytes(size) as usize) < bytes.len() {
        unicorn
            .mem_write(size_pointer, &(bytes.len() as u32).to_le_bytes())
            .map_err(|e| format!("adapter required size write: {e}"))?;
        return Ok(111);
    }
    let bytes = serialize_windows_adapters(&adapters, output, flags & 0x20 != 0)?;
    let end = output
        .checked_add(bytes.len() as u64)
        .ok_or("adapter output range overflow")?;
    if output < size_pointer + 4 && size_pointer < end {
        return Err("adapter output overlaps size pointer".into());
    }
    if !guest_range_has_permission(unicorn, output, bytes.len() as u64, Prot::WRITE)? {
        return Err("adapter output is not writable".into());
    }
    unicorn
        .mem_write(output, &bytes)
        .map_err(|e| format!("adapter output write: {e}"))?;
    unicorn
        .mem_write(size_pointer, &(bytes.len() as u32).to_le_bytes())
        .map_err(|e| format!("adapter size write: {e}"))?;
    Ok(0)
}
