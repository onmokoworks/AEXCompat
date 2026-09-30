//! Owned native interface inventory for the Windows IP Helper translation.
//! Values here retain native semantics; in particular BSD flags and sockaddr
//! bytes must not be copied directly into Windows structures.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAddress {
    pub family: u8,
    pub sockaddr: Vec<u8>,
    pub netmask: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: Vec<u8>,
    pub index: u32,
    pub native_flags: u32,
    pub native_type: u8,
    pub mtu: u32,
    pub physical_address: Vec<u8>,
    pub addresses: Vec<InterfaceAddress>,
}

#[cfg(target_os = "macos")]
pub fn interfaces() -> Result<Vec<Interface>, String> {
    use std::collections::BTreeMap;

    struct List(*mut libc::ifaddrs);
    impl Drop for List {
        fn drop(&mut self) {
            // Only the list returned by getifaddrs, never a guest pointer.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }

    // The prefix of Darwin struct if_data in <net/if_var.h>. Only AF_LINK
    // ifa_data has this layout. Using a prefix avoids depending on statistics
    // fields (notably Darwin's packed timeval) that we do not consume.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct IfDataPrefix {
        attributes: [u8; 8],
        mtu: u32,
    }

    // Darwin sockaddr starts with an allocation length and address family.
    // Netmasks can use a shorter, zero-padded representation, including len=0.
    unsafe fn copy_sockaddr(pointer: *const libc::sockaddr) -> Vec<u8> {
        let length = unsafe { pointer.cast::<u8>().read() } as usize;
        unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) }.to_vec()
    }

    let mut head = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(format!(
            "host interface lookup failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let _owned = List(head);
    let mut links = BTreeMap::new();
    let mut addresses: BTreeMap<Vec<u8>, Vec<InterfaceAddress>> = BTreeMap::new();
    let mut cursor = head;
    let mut count = 0usize;
    while !cursor.is_null() {
        count += 1;
        if count > 16384 {
            return Err("host interface list exceeds bound".into());
        }
        let item = unsafe { cursor.read_unaligned() };
        cursor = item.ifa_next;
        if item.ifa_name.is_null() {
            return Err("host interface has no name".into());
        }
        let name_len = unsafe { libc::strnlen(item.ifa_name, libc::IFNAMSIZ) };
        if name_len == 0 || name_len == libc::IFNAMSIZ {
            return Err("host interface name exceeds bound or is empty".into());
        }
        let name =
            unsafe { std::slice::from_raw_parts(item.ifa_name.cast::<u8>(), name_len).to_vec() };
        if item.ifa_addr.is_null() {
            continue;
        }
        let bytes = unsafe { copy_sockaddr(item.ifa_addr) };
        if bytes.len() < 2 {
            return Err("host interface address has no family".into());
        }
        let family = bytes[1];
        if family as i32 == libc::AF_LINK {
            // sockaddr_dl has an eight-byte header followed by name, link
            // address and selector. sdl_data[12] in the C declaration is not
            // the allocation bound; longer names/addresses extend it.
            if bytes.len() < 8 {
                return Err("host link address has a truncated header".into());
            }
            let start = 8 + bytes[5] as usize;
            let end = start + bytes[6] as usize;
            if end + bytes[7] as usize > bytes.len() || item.ifa_data.is_null() {
                return Err("host link address or interface data is incomplete".into());
            }
            let index = u16::from_ne_bytes([bytes[2], bytes[3]]) as u32;
            if index == 0 || bytes[8..start] != name {
                return Err("host interface index/name is inconsistent".into());
            }
            let data = unsafe { item.ifa_data.cast::<IfDataPrefix>().read_unaligned() };
            let interface = Interface {
                name: name.clone(),
                index,
                native_flags: item.ifa_flags,
                native_type: bytes[4],
                mtu: data.mtu,
                physical_address: bytes[start..end].to_vec(),
                addresses: Vec::new(),
            };
            if links.insert(name, interface).is_some() || links.len() > 4096 {
                return Err("host interface list is duplicate or exceeds bound".into());
            }
        } else if family as i32 == libc::AF_INET || family as i32 == libc::AF_INET6 {
            let minimum = if family as i32 == libc::AF_INET {
                16
            } else {
                28
            };
            if bytes.len() < minimum {
                return Err("host IP address is truncated".into());
            }
            let netmask = if item.ifa_netmask.is_null() {
                None
            } else {
                Some(unsafe { copy_sockaddr(item.ifa_netmask) })
            };
            let entries = addresses.entry(name).or_default();
            if entries.len() >= 256 {
                return Err("host interface address count exceeds bound".into());
            }
            entries.push(InterfaceAddress {
                family,
                sockaddr: bytes,
                netmask,
            });
        }
    }
    for (name, entries) in addresses {
        links
            .get_mut(&name)
            .ok_or("host IP address has no matching link interface")?
            .addresses = entries;
    }
    let mut result: Vec<_> = links.into_values().collect();
    result.sort_by_key(|interface| interface.index);
    if result.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err("host interface indices are duplicate".into());
    }
    Ok(result)
}

#[cfg(not(target_os = "macos"))]
pub fn interfaces() -> Result<Vec<Interface>, String> {
    Err("native interface inventory is not implemented on this host".into())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn owned_interfaces_match_native_ifconfig() {
        let first = super::interfaces().unwrap();
        let saved = first.clone();
        let _second = super::interfaces().unwrap();
        assert_eq!(first, saved);
        let loopback = first.iter().find(|item| item.name == b"lo0").unwrap();
        assert!(loopback.physical_address.is_empty());
        assert!(
            loopback
                .addresses
                .iter()
                .any(|address| address.family as i32 == libc::AF_INET
                    && address.sockaddr[4..8] == [127, 0, 0, 1])
        );
        for item in first {
            let name = std::str::from_utf8(&item.name).unwrap();
            let result = std::process::Command::new("/sbin/ifconfig")
                .arg(name)
                .output()
                .unwrap();
            assert!(result.status.success());
            let report = String::from_utf8(result.stdout).unwrap();
            let header: Vec<_> = report.lines().next().unwrap().split_whitespace().collect();
            let mtu = header.windows(2).find(|pair| pair[0] == "mtu").unwrap()[1];
            assert_eq!(item.mtu, mtu.parse::<u32>().unwrap());
            if item.physical_address.len() == 6 {
                let mac = item
                    .physical_address
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(":");
                assert!(
                    report
                        .lines()
                        .any(|line| line.split_whitespace().any(|word| word == mac))
                );
            }
        }
    }
}
