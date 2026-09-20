//! Read-only Darwin network configuration. Missing keys remain missing: callers
//! must distinguish absent configuration from disabled or empty configuration.
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringGetLength},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Property {
    Text(String),
    TextArray(Vec<String>),
    Boolean(bool),
}

pub type Configuration = BTreeMap<String, BTreeMap<String, Property>>;

/// Service identifiers associated with the native interface. State names take
/// precedence over setup names for running VPNs. IPv4 and IPv6 state names may
/// differ; each interface must retain the matching service in that case.
pub fn interface_services(
    configuration: &Configuration,
    interface_name: &str,
) -> Result<Vec<String>, String> {
    if interface_name.is_empty()
        || interface_name.len() >= libc::IFNAMSIZ
        || interface_name.contains(['/', '\0'])
    {
        return Err("invalid native interface name".into());
    }
    let mut services = std::collections::BTreeSet::new();
    for key in configuration.keys() {
        let rest = key
            .strip_prefix("Setup:/Network/Service/")
            .or_else(|| key.strip_prefix("State:/Network/Service/"));
        if let Some(rest) = rest {
            if let Some((id, entity)) = rest.split_once('/') {
                if !id.is_empty() && !entity.contains('/') {
                    services.insert(id.to_string());
                }
            }
        }
    }
    if services.len() > 4096 {
        return Err("network service count exceeds bound".into());
    }
    let get_name = |key: &str, field: &str| -> Result<Option<&str>, String> {
        match configuration.get(key).and_then(|record| record.get(field)) {
            None => Ok(None),
            Some(Property::Text(value)) => Ok(Some(value)),
            Some(_) => Err("network service interface name has invalid type".into()),
        }
    };
    let mut result = Vec::new();
    for id in services {
        let ipv4 = get_name(
            &format!("State:/Network/Service/{id}/IPv4"),
            "InterfaceName",
        )?;
        let ipv6 = get_name(
            &format!("State:/Network/Service/{id}/IPv6"),
            "InterfaceName",
        )?;
        let matches = if ipv4.is_some() || ipv6.is_some() {
            ipv4 == Some(interface_name) || ipv6 == Some(interface_name)
        } else {
            get_name(
                &format!("Setup:/Network/Service/{id}/Interface"),
                "DeviceName",
            )? == Some(interface_name)
        };
        if matches {
            result.push(id);
        }
    }
    Ok(result)
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCDynamicStoreCopyMultiple(
        store: CFTypeRef,
        keys: CFTypeRef,
        patterns: CFTypeRef,
    ) -> CFTypeRef;
}

fn text(value: &CFType) -> Result<String, String> {
    let string = value
        .downcast::<CFString>()
        .ok_or("network configuration expected string")?;
    let length = unsafe { CFStringGetLength(string.as_concrete_TypeRef()) };
    if !(0..=4096).contains(&length) {
        return Err("network configuration string exceeds bound".into());
    }
    let result = string.to_string();
    if result.contains('\0') {
        return Err("network configuration string contains NUL".into());
    }
    Ok(result)
}

fn property(value: &CFType, kind: u8) -> Result<Property, String> {
    match kind {
        0 => Ok(Property::Text(text(value)?)),
        1 => {
            let array = value
                .downcast::<CFArray>()
                .ok_or("network configuration expected array")?;
            if !(0..=256).contains(&array.len()) {
                return Err("network configuration array exceeds bound".into());
            }
            let mut result = Vec::new();
            for pointer in array.iter() {
                let item = unsafe { CFType::wrap_under_get_rule(*pointer) };
                result.push(text(&item)?);
            }
            Ok(Property::TextArray(result))
        }
        2 => {
            let boolean = value
                .downcast::<CFBoolean>()
                .ok_or("network configuration expected boolean")?;
            Ok(Property::Boolean(boolean.into()))
        }
        _ => Err("unknown network configuration property type".into()),
    }
}

fn decode(root: &CFDictionary) -> Result<Configuration, String> {
    if root.len() > 16384 {
        return Err("network configuration record count exceeds bound".into());
    }
    let mut result = BTreeMap::new();
    let mut bytes = 0usize;
    let (keys, values) = root.get_keys_and_values();
    for (key, value) in keys.into_iter().zip(values) {
        let key = text(&unsafe { CFType::wrap_under_get_rule(key) })?;
        bytes += key.len();
        let value = unsafe { CFType::wrap_under_get_rule(value) };
        let dictionary = value
            .downcast::<CFDictionary>()
            .ok_or("network configuration expected dictionary")?;
        if dictionary.len() > 256 {
            return Err("network configuration property count exceeds bound".into());
        }
        let mut record = BTreeMap::new();
        // Only copy documented configuration fields consumed by IP Helper
        // translation. Credentials and opaque network signatures are excluded.
        for (name, kind) in [
            ("DeviceName", 0),
            ("Hardware", 0),
            ("Type", 0),
            ("SubType", 0),
            ("UserDefinedName", 0),
            ("ConfigMethod", 0),
            ("InterfaceName", 0),
            ("DomainName", 0),
            ("SearchDomains", 1),
            ("ServerAddresses", 1),
            ("Active", 2),
        ] {
            let property_key = CFString::new(name);
            if let Some(value) = dictionary.find(property_key.as_CFTypeRef()) {
                let value = unsafe { CFType::wrap_under_get_rule(*value) };
                let value = property(&value, kind)?;
                bytes += match &value {
                    Property::Text(text) => text.len(),
                    Property::TextArray(array) => array.iter().map(String::len).sum(),
                    Property::Boolean(_) => 1,
                };
                if bytes > 4 * 1024 * 1024 {
                    return Err("network configuration snapshot exceeds bound".into());
                }
                record.insert(name.to_string(), value);
            }
        }
        if bytes > 4 * 1024 * 1024 {
            return Err("network configuration snapshot exceeds bound".into());
        }
        result.insert(key, record);
    }
    Ok(result)
}

pub fn snapshot() -> Result<Configuration, String> {
    let patterns = CFArray::from_CFTypes(&[
        CFString::new("Setup:/Network/Service/[^/]+/(Interface|IPv4|IPv6|DNS)$"),
        CFString::new("State:/Network/Service/[^/]+/(IPv4|IPv6|DNS)$"),
        CFString::new("State:/Network/Interface/[^/]+/Link$"),
    ]);
    // NULL uses the native temporary session; this call only reads the store.
    let pointer = unsafe {
        SCDynamicStoreCopyMultiple(std::ptr::null(), std::ptr::null(), patterns.as_CFTypeRef())
    };
    if pointer.is_null() {
        return Err("native network configuration lookup failed".into());
    }
    let owned = unsafe { CFType::wrap_under_create_rule(pointer) };
    let dictionary = owned
        .downcast::<CFDictionary>()
        .ok_or("native network configuration is not a dictionary")?;
    decode(&dictionary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_correlation_uses_runtime_names_and_keeps_multiple_services() {
        let mut config = Configuration::new();
        for (path, field, value) in [
            (
                "Setup:/Network/Service/ether/Interface",
                "DeviceName",
                "en0",
            ),
            ("Setup:/Network/Service/vpn/Interface", "DeviceName", "en0"),
            ("State:/Network/Service/vpn/IPv4", "InterfaceName", "utun0"),
            ("State:/Network/Service/vpn/IPv6", "InterfaceName", "utun1"),
            (
                "State:/Network/Service/second/IPv4",
                "InterfaceName",
                "utun0",
            ),
        ] {
            config.insert(
                path.into(),
                BTreeMap::from([(field.into(), Property::Text(value.into()))]),
            );
        }
        assert_eq!(interface_services(&config, "en0").unwrap(), ["ether"]);
        assert_eq!(
            interface_services(&config, "utun0").unwrap(),
            ["second", "vpn"]
        );
        assert_eq!(interface_services(&config, "utun1").unwrap(), ["vpn"]);
        assert!(interface_services(&config, "lo0").unwrap().is_empty());
        assert!(interface_services(&config, "../en0").is_err());
        config
            .get_mut("State:/Network/Service/vpn/IPv4")
            .unwrap()
            .insert("InterfaceName".into(), Property::Boolean(true));
        assert!(interface_services(&config, "utun0").is_err());
    }

    #[test]
    fn decoding_preserves_missing_fields_and_rejects_wrong_native_types() {
        let key = CFString::new("Setup:/Network/Service/test/IPv4");
        let fields = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("ConfigMethod"),
                CFString::new("DHCP").as_CFType(),
            ),
            (
                CFString::new("OpaqueCredential"),
                CFString::new("excluded").as_CFType(),
            ),
        ]);
        let root = CFDictionary::from_CFType_pairs(&[(key.clone(), fields)]);
        let copied = decode(&root.to_untyped()).unwrap();
        drop(root);
        let record = &copied["Setup:/Network/Service/test/IPv4"];
        assert_eq!(record.len(), 1);
        assert_eq!(record["ConfigMethod"], Property::Text("DHCP".into()));
        assert!(!record.contains_key("Active"));
        let wrong = CFDictionary::from_CFType_pairs(&[(
            CFString::new("ConfigMethod"),
            CFBoolean::true_value().as_CFType(),
        )]);
        let root = CFDictionary::from_CFType_pairs(&[(key, wrong)]);
        assert!(decode(&root.to_untyped()).is_err());
    }

    #[test]
    fn configuration_types_bounds_and_owned_values() {
        let value = CFString::new("DHCP").as_CFType();
        assert_eq!(property(&value, 0).unwrap(), Property::Text("DHCP".into()));
        assert!(property(&value, 2).is_err());
        assert!(text(&CFString::new(&"x".repeat(4097)).as_CFType()).is_err());
        let array = CFArray::from_CFTypes(&[CFString::new("example.test")]);
        let copied = property(&array.as_CFType(), 1).unwrap();
        drop(array);
        assert_eq!(copied, Property::TextArray(vec!["example.test".into()]));
        let wrong = CFArray::from_CFTypes(&[CFBoolean::true_value()]);
        assert!(property(&wrong.as_CFType(), 1).is_err());
        assert_eq!(
            property(&CFBoolean::false_value().as_CFType(), 2).unwrap(),
            Property::Boolean(false)
        );
        let first = snapshot().unwrap();
        let saved = first.clone();
        drop(snapshot().unwrap());
        assert_eq!(first, saved);
    }
}
