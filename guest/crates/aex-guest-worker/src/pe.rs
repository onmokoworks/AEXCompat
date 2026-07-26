use std::collections::BTreeMap;

use goblin::pe::PE;
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const AMD64_MACHINE: u16 = 0x8664;
const MAX_FILE_SIZE: usize = 128 * 1024 * 1024;
const MAX_IMAGE_SIZE: usize = 256 * 1024 * 1024;
const MAX_SECTIONS: usize = 96;
const MAX_IMPORTS: usize = 4096;
const MAX_EXPORTS: usize = 4096;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
const MAX_STRING_KEY_PATH_BYTES: usize = 256;
const MAX_STRING_KEY_DIGITS: usize = 10;

#[derive(Debug, Error)]
pub enum PeError {
    #[error("file size {0} is outside 1..={MAX_FILE_SIZE}")]
    FileSize(usize),
    #[error("invalid PE: {0}")]
    Parse(String),
    #[error("only AMD64 PE32+ DLL images are supported")]
    UnsupportedImage,
    #[error("image size {0} is outside 1..={MAX_IMAGE_SIZE}")]
    ImageSize(usize),
    #[error("section count {0} exceeds {MAX_SECTIONS}")]
    SectionCount(usize),
    #[error("import count {0} exceeds {MAX_IMPORTS}")]
    ImportCount(usize),
    #[error("export count {0} exceeds {MAX_EXPORTS}")]
    ExportCount(usize),
    #[error("headers exceed file or mapped image")]
    HeaderRange,
    #[error("section {name} raw range is outside the file")]
    SectionFileRange { name: String },
    #[error("section {name} virtual range is outside the mapped image")]
    SectionImageRange { name: String },
    #[error(
        "effect discovery export was not found (tried EffectMain, entryPointFunc, entry_point, PluginDataEntryFunction2, PluginDataEntryFunction)"
    )]
    MissingEntryExport,
    #[error("duplicate named export {0}")]
    DuplicateExport(String),
    #[error("export {0} does not point into an executable image section")]
    NonExecutableExport(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportSymbol {
    pub name: String,
    pub iat_rva: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportLibrary {
    pub name: String,
    pub symbols: Vec<ImportSymbol>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeReport {
    pub schema_version: u32,
    pub sha256: String,
    pub file_size: usize,
    pub machine: String,
    pub image_base: u64,
    pub image_size: usize,
    pub entry_export: String,
    pub entry_rva: usize,
    pub dll_entry_rva: usize,
    pub section_count: usize,
    pub imports: Vec<ImportLibrary>,
    pub has_tls: bool,
    pub has_exception_directory: bool,
}

#[derive(Debug)]
pub struct PeImage {
    bytes: Vec<u8>,
    sha256: String,
    image_base: u64,
    entry_export: String,
    entry_rva: usize,
    direct_entry: Option<String>,
    exports: BTreeMap<String, usize>,
    dll_entry_rva: usize,
    section_count: usize,
    imports: Vec<ImportLibrary>,
    has_tls: bool,
    has_exception_directory: bool,
    file_size: usize,
    section_protections: Vec<SectionProtection>,
    string_table: Option<BTreeMap<i32, Vec<u8>>>,
}

#[derive(Clone, Debug)]
pub struct SectionProtection {
    pub virtual_address: usize,
    pub virtual_size: usize,
    pub executable: bool,
    pub writable: bool,
}

impl PeImage {
    pub fn parse_and_map(file: &[u8]) -> Result<Self, PeError> {
        if file.is_empty() || file.len() > MAX_FILE_SIZE {
            return Err(PeError::FileSize(file.len()));
        }
        let pe = PE::parse(file).map_err(|error| PeError::Parse(error.to_string()))?;
        if pe.header.coff_header.machine != AMD64_MACHINE || !pe.is_64 || !pe.is_lib {
            return Err(PeError::UnsupportedImage);
        }
        if pe.sections.len() > MAX_SECTIONS {
            return Err(PeError::SectionCount(pe.sections.len()));
        }
        if pe.imports.len() > MAX_IMPORTS {
            return Err(PeError::ImportCount(pe.imports.len()));
        }
        if pe.exports.len() > MAX_EXPORTS {
            return Err(PeError::ExportCount(pe.exports.len()));
        }

        let optional = pe
            .header
            .optional_header
            .as_ref()
            .ok_or(PeError::UnsupportedImage)?;
        let image_size = optional.windows_fields.size_of_image as usize;
        let dll_entry_rva = optional.standard_fields.address_of_entry_point as usize;
        let header_size = optional.windows_fields.size_of_headers as usize;
        if image_size == 0 || image_size > MAX_IMAGE_SIZE {
            return Err(PeError::ImageSize(image_size));
        }
        if header_size > file.len() || header_size > image_size {
            return Err(PeError::HeaderRange);
        }

        let mut mapped = vec![0u8; image_size];
        let mut section_protections = Vec::with_capacity(pe.sections.len());
        mapped[..header_size].copy_from_slice(&file[..header_size]);
        for section in &pe.sections {
            let name = section.name().unwrap_or("<invalid>").to_string();
            let raw_start = section.pointer_to_raw_data as usize;
            let raw_size = section.size_of_raw_data as usize;
            let raw_end = raw_start
                .checked_add(raw_size)
                .ok_or_else(|| PeError::SectionFileRange { name: name.clone() })?;
            if raw_end > file.len() {
                return Err(PeError::SectionFileRange { name });
            }
            let virtual_start = section.virtual_address as usize;
            let virtual_end = virtual_start
                .checked_add(raw_size)
                .ok_or_else(|| PeError::SectionImageRange { name: name.clone() })?;
            if virtual_end > mapped.len() {
                return Err(PeError::SectionImageRange { name });
            }
            if raw_size != 0 {
                mapped[virtual_start..virtual_end].copy_from_slice(&file[raw_start..raw_end]);
            }
            section_protections.push(SectionProtection {
                virtual_address: virtual_start,
                virtual_size: (section.virtual_size as usize).max(raw_size),
                executable: section.characteristics & IMAGE_SCN_MEM_EXECUTE != 0,
                writable: section.characteristics & IMAGE_SCN_MEM_WRITE != 0,
            });
        }

        let mut exports = BTreeMap::new();
        for export in pe.exports.iter().filter(|export| export.reexport.is_none()) {
            let Some(name) = export.name else {
                continue;
            };
            if exports.insert(name.to_string(), export.rva).is_some() {
                return Err(PeError::DuplicateExport(name.to_string()));
            }
        }
        let executable_export = |name: &str, rva: usize| {
            section_protections
                .iter()
                .any(|section| {
                    section.executable
                        && rva >= section.virtual_address
                        && rva < section.virtual_address.saturating_add(section.virtual_size)
                })
                .then_some((name.to_string(), rva))
        };
        for (name, rva) in &exports {
            if ["EffectMain", "entryPointFunc", "entry_point"]
                .into_iter()
                .chain(["PluginDataEntryFunction2", "PluginDataEntryFunction"])
                .any(|candidate| candidate == name)
                && executable_export(name, *rva).is_none()
            {
                return Err(PeError::NonExecutableExport(name.clone()));
            }
        }

        let direct_candidates = ["EffectMain", "entryPointFunc", "entry_point"];
        let direct_entry = direct_candidates
            .iter()
            .find(|candidate| exports.contains_key(**candidate))
            .map(|candidate| (*candidate).to_string());
        let discovery_entry = direct_entry.clone().or_else(|| {
            ["PluginDataEntryFunction2", "PluginDataEntryFunction"]
                .into_iter()
                .find(|candidate| exports.contains_key(*candidate))
                .map(str::to_string)
        });
        let entry_export = discovery_entry.ok_or(PeError::MissingEntryExport)?;
        let entry_rva = exports[&entry_export];

        let mut grouped: BTreeMap<String, Vec<ImportSymbol>> = BTreeMap::new();
        for import in &pe.imports {
            grouped
                .entry(import.dll.to_ascii_lowercase())
                .or_default()
                .push(ImportSymbol {
                    name: import.name.to_string(),
                    iat_rva: import.offset,
                });
        }
        let imports = grouped
            .into_iter()
            .map(|(name, mut symbols)| {
                symbols.sort_by_key(|symbol| symbol.iat_rva);
                ImportLibrary { name, symbols }
            })
            .collect();
        let string_table = parse_readonly_string_table(file, &pe.sections);

        Ok(Self {
            bytes: mapped,
            sha256: format!("{:x}", Sha256::digest(file)),
            image_base: pe.image_base,
            entry_export,
            entry_rva,
            direct_entry,
            exports,
            dll_entry_rva,
            section_count: pe.sections.len(),
            imports,
            has_tls: pe.tls_data.is_some(),
            has_exception_directory: pe.exception_data.is_some(),
            file_size: file.len(),
            section_protections,
            string_table,
        })
    }

    pub fn mapped_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    pub fn entry_address(&self) -> Option<u64> {
        self.direct_entry
            .as_deref()
            .and_then(|name| self.export_address(name))
    }

    pub fn export_address(&self, name: &str) -> Option<u64> {
        let rva = *self.exports.get(name)?;
        self.section_protections
            .iter()
            .any(|section| {
                section.executable
                    && rva >= section.virtual_address
                    && rva < section.virtual_address.saturating_add(section.virtual_size)
            })
            .then(|| self.image_base.checked_add(rva as u64))
            .flatten()
    }

    pub fn dll_entry_address(&self) -> Option<u64> {
        (self.dll_entry_rva != 0).then_some(self.image_base + self.dll_entry_rva as u64)
    }

    pub fn imports(&self) -> &[ImportLibrary] {
        &self.imports
    }

    pub fn section_protections(&self) -> &[SectionProtection] {
        &self.section_protections
    }

    pub fn string_table(&self) -> Option<&BTreeMap<i32, Vec<u8>>> {
        self.string_table.as_ref()
    }

    pub fn report(&self) -> PeReport {
        PeReport {
            schema_version: 1,
            sha256: self.sha256.clone(),
            file_size: self.file_size,
            machine: "x86_64-windows".to_string(),
            image_base: self.image_base,
            image_size: self.bytes.len(),
            entry_export: self.entry_export.clone(),
            entry_rva: self.entry_rva,
            dll_entry_rva: self.dll_entry_rva,
            section_count: self.section_count,
            imports: self.imports.clone(),
            has_tls: self.has_tls,
            has_exception_directory: self.has_exception_directory,
        }
    }
}

enum StringCandidate {
    Lstr {
        group: String,
        id: i32,
        value: Vec<u8>,
    },
    Ordinal {
        id: i32,
        value: Vec<u8>,
    },
}

fn structural_ascii(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|byte| (0x20..=0x7e).contains(byte))
}

fn parse_string_candidate(candidate: &[u8], next_ordinal: i32) -> Option<StringCandidate> {
    const PREFIX: &[u8] = b"$$$/";
    const MARKER: &[u8] = b"/LStr/";
    if !candidate.starts_with(PREFIX) {
        return None;
    }
    let marker = candidate[PREFIX.len()..]
        .windows(MARKER.len())
        .position(|window| window == MARKER)
        .map(|offset| offset + PREFIX.len());
    let Some(marker) = marker else {
        let equals = candidate[PREFIX.len()..]
            .iter()
            .position(|byte| *byte == b'=')
            .map(|offset| offset + PREFIX.len())?;
        let key = &candidate[PREFIX.len()..equals];
        if key.len() > MAX_STRING_KEY_PATH_BYTES || !structural_ascii(key) {
            return None;
        }
        return Some(StringCandidate::Ordinal {
            id: next_ordinal,
            value: candidate[equals + 1..].to_vec(),
        });
    };
    let group = &candidate[PREFIX.len()..marker];
    let digits_begin = marker + MARKER.len();
    let equals = candidate[digits_begin..]
        .iter()
        .position(|byte| *byte == b'=')
        .map(|offset| offset + digits_begin)?;
    let digits = &candidate[digits_begin..equals];
    if group.is_empty()
        || !structural_ascii(group)
        || digits.is_empty()
        || digits.len() > MAX_STRING_KEY_DIGITS
        || !digits.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let id = digits.iter().try_fold(0i32, |value, digit| {
        value.checked_mul(10)?.checked_add(i32::from(digit - b'0'))
    })?;
    Some(StringCandidate::Lstr {
        group: String::from_utf8(group.to_vec()).ok()?,
        id,
        value: candidate[equals + 1..].to_vec(),
    })
}

fn parse_readonly_string_table(
    file: &[u8],
    sections: &[goblin::pe::section_table::SectionTable],
) -> Option<BTreeMap<i32, Vec<u8>>> {
    let mut groups = BTreeMap::<String, BTreeMap<i32, Vec<u8>>>::new();
    let mut ordinals = BTreeMap::new();
    let mut next_ordinal = 0i32;
    for section in sections {
        if section.characteristics & IMAGE_SCN_MEM_READ == 0
            || section.characteristics & (IMAGE_SCN_MEM_WRITE | IMAGE_SCN_MEM_EXECUTE) != 0
        {
            continue;
        }
        let start = section.pointer_to_raw_data as usize;
        let size = section.size_of_raw_data as usize;
        let end = start.checked_add(size)?;
        let raw = file.get(start..end)?;
        let mut offset = 0usize;
        while offset + 4 <= raw.len() {
            if &raw[offset..offset + 4] != b"$$$/" {
                offset += 1;
                continue;
            }
            let end = raw[offset..].iter().position(|byte| *byte == 0)? + offset;
            if let Some(candidate) = parse_string_candidate(&raw[offset..end], next_ordinal) {
                let inserted = match candidate {
                    StringCandidate::Lstr { group, id, value } => {
                        groups.entry(group).or_default().insert(id, value).is_none()
                    }
                    StringCandidate::Ordinal { id, value } => ordinals.insert(id, value).is_none(),
                };
                if !inserted {
                    return None;
                }
                next_ordinal = next_ordinal.checked_add(1)?;
            }
            offset = end + 1;
        }
    }
    let has_lstr = !groups.is_empty();
    let values = if groups.len() == 1 {
        groups
            .pop_first()
            .map(|(_, values)| values)
            .unwrap_or_default()
    } else if groups.len() > 1 {
        let mut primary = groups
            .iter()
            .filter(|(_, values)| {
                values
                    .get(&0)
                    .is_some_and(|value| value.windows(4).any(|window| window == b", v%"))
            })
            .map(|(group, _)| group.clone());
        let group = primary.next()?;
        if primary.next().is_some() {
            return None;
        }
        groups.remove(&group)?
    } else if !has_lstr {
        ordinals
    } else {
        BTreeMap::new()
    };
    (!values.is_empty()).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_oversized_inputs_fail_before_parse() {
        assert!(matches!(
            PeImage::parse_and_map(&[]),
            Err(PeError::FileSize(0))
        ));
        assert!(matches!(
            PeImage::parse_and_map(&vec![0; MAX_FILE_SIZE + 1]),
            Err(PeError::FileSize(_))
        ));
    }

    #[test]
    fn non_pe_input_fails_as_parse_error() {
        assert!(matches!(
            PeImage::parse_and_map(b"not a PE image"),
            Err(PeError::Parse(_))
        ));
    }

    #[test]
    fn string_candidates_preserve_values_and_validate_keys() {
        match parse_string_candidate(b"$$$/AE/Test/LStr/0069=Rows & Columns", 3).unwrap() {
            StringCandidate::Lstr { group, id, value } => {
                assert_eq!(group, "AE/Test");
                assert_eq!(id, 69);
                assert_eq!(value, b"Rows & Columns");
            }
            StringCandidate::Ordinal { .. } => panic!("expected LStr candidate"),
        }
        match parse_string_candidate(b"$$$/MediaCore/Test/0003=\xe6\x97\xa5", 7).unwrap() {
            StringCandidate::Ordinal { id, value } => {
                assert_eq!(id, 7);
                assert_eq!(value, b"\xe6\x97\xa5");
            }
            StringCandidate::Lstr { .. } => panic!("expected ordinal candidate"),
        }
        assert!(parse_string_candidate(b"$$$/AE/Test/LStr/x=no", 0).is_none());
    }
}
