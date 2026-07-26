use serde::Serialize;
use thiserror::Error;

pub const CALLBACK_REJECTED: i32 = 3;
pub const EFFECT_KIND: i32 = 0x6546_4b54; // MSVC 'eFKT'
pub const MAX_REGISTRATIONS: usize = 64;
const MAX_NAME_BYTES: usize = 256;
const MAX_CATEGORY_BYTES: usize = 256;
const MAX_ENTRYPOINT_BYTES: usize = 128;
const MAX_SUPPORT_URL_BYTES: usize = 1024;
const MAX_API_MAJOR: i32 = 13;
const MAX_API_MINOR: i32 = 29;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EffectRegistration {
    pub name: Vec<u8>,
    pub match_name: Vec<u8>,
    pub category: Vec<u8>,
    pub entrypoint: String,
    pub api_major: i32,
    pub api_minor: i32,
    pub reserved_info: i32,
    pub support_url: Option<Vec<u8>>,
}

impl EffectRegistration {
    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    pub fn match_name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.match_name).into_owned()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RegistrationPointers {
    pub name: u64,
    pub match_name: u64,
    pub category: u64,
    pub entrypoint: u64,
    pub kind: i32,
    pub api_major: i32,
    pub api_minor: i32,
    pub reserved_info: i32,
    pub support_url: Option<u64>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RegistrationError {
    #[error("{field} pointer is null")]
    Null { field: &'static str },
    #[error("{field} is unreadable at {address:#x}")]
    Unreadable { field: &'static str, address: u64 },
    #[error("{field} is not NUL terminated within {limit} bytes")]
    Unterminated { field: &'static str, limit: usize },
    #[error("{field} contains a control byte")]
    ControlByte { field: &'static str },
    #[error("{field} is required")]
    Empty { field: &'static str },
    #[error("entrypoint is not an ASCII export identifier")]
    InvalidEntrypoint,
    #[error("plugin kind {0:#x} is not an Effect registration")]
    InvalidKind(i32),
    #[error("PluginData API version {major}.{minor} is unsupported")]
    UnsupportedVersion { major: i32, minor: i32 },
    #[error("registration count exceeds {MAX_REGISTRATIONS}")]
    TooManyRegistrations,
    #[error("duplicate effect identity {0}")]
    DuplicateIdentity(String),
    #[error("PluginData registration produced no effects")]
    EmptyRegistry,
    #[error("effect selector {0} did not match a registration")]
    SelectionMissing(String),
    #[error("effect selector {0} is ambiguous")]
    SelectionAmbiguous(String),
    #[error("effect index {index} is outside 0..{count}")]
    SelectionIndex { index: usize, count: usize },
}

#[derive(Clone, Debug, Default)]
pub struct EffectRegistry {
    registrations: Vec<EffectRegistration>,
}

impl EffectRegistry {
    pub fn push(&mut self, registration: EffectRegistration) -> Result<(), RegistrationError> {
        if self.registrations.len() >= MAX_REGISTRATIONS {
            return Err(RegistrationError::TooManyRegistrations);
        }
        if self.registrations.iter().any(|current| {
            current.match_name == registration.match_name
                && current.entrypoint == registration.entrypoint
        }) {
            return Err(RegistrationError::DuplicateIdentity(
                registration.match_name_lossy(),
            ));
        }
        self.registrations.push(registration);
        Ok(())
    }

    pub fn registrations(&self) -> &[EffectRegistration] {
        &self.registrations
    }

    pub fn select(&self, selector: Option<&str>) -> Result<&EffectRegistration, RegistrationError> {
        if self.registrations.is_empty() {
            return Err(RegistrationError::EmptyRegistry);
        }
        let Some(selector) = selector else {
            return Ok(&self.registrations[0]);
        };
        if let Some(index) = selector.strip_prefix('#') {
            let index = index
                .parse::<usize>()
                .map_err(|_| RegistrationError::SelectionMissing(selector.to_string()))?;
            return self
                .registrations
                .get(index)
                .ok_or(RegistrationError::SelectionIndex {
                    index,
                    count: self.registrations.len(),
                });
        }
        let matches = self
            .registrations
            .iter()
            .filter(|registration| registration.match_name == selector.as_bytes())
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [registration] => Ok(registration),
            [] => Err(RegistrationError::SelectionMissing(selector.to_string())),
            _ => Err(RegistrationError::SelectionAmbiguous(selector.to_string())),
        }
    }
}

pub fn decode_registration(
    pointers: RegistrationPointers,
    mut read_byte: impl FnMut(u64) -> Option<u8>,
) -> Result<EffectRegistration, RegistrationError> {
    if pointers.kind != EFFECT_KIND {
        return Err(RegistrationError::InvalidKind(pointers.kind));
    }
    if pointers.api_major <= 0
        || pointers.api_major > MAX_API_MAJOR
        || pointers.api_minor < 0
        || (pointers.api_major == MAX_API_MAJOR && pointers.api_minor > MAX_API_MINOR)
    {
        return Err(RegistrationError::UnsupportedVersion {
            major: pointers.api_major,
            minor: pointers.api_minor,
        });
    }
    let name = read_text("name", pointers.name, MAX_NAME_BYTES, true, &mut read_byte)?;
    let match_name = read_text(
        "match_name",
        pointers.match_name,
        MAX_NAME_BYTES,
        true,
        &mut read_byte,
    )?;
    let category = read_text(
        "category",
        pointers.category,
        MAX_CATEGORY_BYTES,
        true,
        &mut read_byte,
    )?;
    let entrypoint_bytes = read_text(
        "entrypoint",
        pointers.entrypoint,
        MAX_ENTRYPOINT_BYTES,
        true,
        &mut read_byte,
    )?;
    if !valid_export_name(&entrypoint_bytes) {
        return Err(RegistrationError::InvalidEntrypoint);
    }
    let entrypoint =
        String::from_utf8(entrypoint_bytes).map_err(|_| RegistrationError::InvalidEntrypoint)?;
    let support_url = match pointers.support_url {
        Some(0) | None => None,
        Some(address) => Some(read_text(
            "support_url",
            address,
            MAX_SUPPORT_URL_BYTES,
            false,
            &mut read_byte,
        )?),
    };
    Ok(EffectRegistration {
        name,
        match_name,
        category,
        entrypoint,
        api_major: pointers.api_major,
        api_minor: pointers.api_minor,
        reserved_info: pointers.reserved_info,
        support_url,
    })
}

fn read_text(
    field: &'static str,
    address: u64,
    limit: usize,
    required: bool,
    read_byte: &mut impl FnMut(u64) -> Option<u8>,
) -> Result<Vec<u8>, RegistrationError> {
    if address == 0 {
        return Err(RegistrationError::Null { field });
    }
    let mut bytes = Vec::with_capacity(limit.min(64));
    for offset in 0..limit {
        let current = address
            .checked_add(offset as u64)
            .ok_or(RegistrationError::Unreadable { field, address })?;
        let value = read_byte(current).ok_or(RegistrationError::Unreadable {
            field,
            address: current,
        })?;
        if value == 0 {
            if required && bytes.is_empty() {
                return Err(RegistrationError::Empty { field });
            }
            return Ok(bytes);
        }
        if value < 0x20 || value == 0x7f {
            return Err(RegistrationError::ControlByte { field });
        }
        bytes.push(value);
    }
    Err(RegistrationError::Unterminated { field, limit })
}

fn valid_export_name(bytes: &[u8]) -> bool {
    let Some(first) = bytes.first() else {
        return false;
    };
    (first.is_ascii_alphabetic() || *first == b'_')
        && bytes[1..]
            .iter()
            .all(|value| value.is_ascii_alphanumeric() || *value == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_memory(parts: &[&[u8]]) -> (Vec<u8>, Vec<u64>) {
        let mut memory = vec![0u8; 0x100];
        let mut addresses = Vec::new();
        let mut cursor = 0x10usize;
        for part in parts {
            addresses.push(cursor as u64);
            memory[cursor..cursor + part.len()].copy_from_slice(part);
            cursor += part.len();
        }
        (memory, addresses)
    }

    #[test]
    fn decodes_localized_metadata_and_opaque_reserved_info() {
        let (memory, address) = fixture_memory(&[
            b"\x82\xa0\x00",
            b"match\x00",
            b"category\x00",
            b"FilterMain\x00",
        ]);
        let registration = decode_registration(
            RegistrationPointers {
                name: address[0],
                match_name: address[1],
                category: address[2],
                entrypoint: address[3],
                kind: EFFECT_KIND,
                api_major: 13,
                api_minor: 29,
                reserved_info: 9,
                support_url: None,
            },
            |address| memory.get(address as usize).copied(),
        )
        .unwrap();
        assert_eq!(registration.name, b"\x82\xa0");
        assert_eq!(registration.entrypoint, "FilterMain");
        assert_eq!(registration.reserved_info, 9);
    }

    #[test]
    fn selects_multiple_effects_by_index_or_exact_match_name() {
        let make = |name: &str, entrypoint: &str| EffectRegistration {
            name: name.as_bytes().to_vec(),
            match_name: name.as_bytes().to_vec(),
            category: b"test".to_vec(),
            entrypoint: entrypoint.to_string(),
            api_major: 13,
            api_minor: 29,
            reserved_info: 0,
            support_url: None,
        };
        let mut registry = EffectRegistry::default();
        registry.push(make("first", "FilterMain")).unwrap();
        registry.push(make("second", "EffectMainExtra")).unwrap();
        assert_eq!(registry.select(None).unwrap().entrypoint, "FilterMain");
        assert_eq!(
            registry.select(Some("#1")).unwrap().entrypoint,
            "EffectMainExtra"
        );
        assert_eq!(
            registry.select(Some("second")).unwrap().entrypoint,
            "EffectMainExtra"
        );
    }

    #[test]
    fn fails_closed_on_unterminated_invalid_and_duplicate_registration() {
        let memory = vec![b'a'; MAX_NAME_BYTES + 1];
        assert!(matches!(
            read_text("name", 0, MAX_NAME_BYTES, true, &mut |address| memory
                .get(address as usize)
                .copied()),
            Err(RegistrationError::Null { .. })
        ));
        assert!(matches!(
            read_text("name", 1, MAX_NAME_BYTES, true, &mut |address| memory
                .get(address as usize)
                .copied()),
            Err(RegistrationError::Unterminated { .. })
        ));

        let registration = EffectRegistration {
            name: b"duplicate".to_vec(),
            match_name: b"duplicate".to_vec(),
            category: b"test".to_vec(),
            entrypoint: "FilterMain".to_string(),
            api_major: 13,
            api_minor: 29,
            reserved_info: 0,
            support_url: None,
        };
        let mut registry = EffectRegistry::default();
        registry.push(registration.clone()).unwrap();
        assert!(matches!(
            registry.push(registration),
            Err(RegistrationError::DuplicateIdentity(_))
        ));
    }
}
