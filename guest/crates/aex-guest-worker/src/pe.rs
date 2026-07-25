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
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

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
    #[error("entry export was not found (tried EffectMain, entryPointFunc, entry_point)")]
    MissingEntryExport,
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
    dll_entry_rva: usize,
    section_count: usize,
    imports: Vec<ImportLibrary>,
    has_tls: bool,
    has_exception_directory: bool,
    file_size: usize,
    section_protections: Vec<SectionProtection>,
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

        let candidates = ["EffectMain", "entryPointFunc", "entry_point"];
        let (entry_export, entry_rva) = candidates
            .iter()
            .find_map(|candidate| {
                pe.exports
                    .iter()
                    .find(|export| export.name == Some(*candidate) && export.reexport.is_none())
                    .map(|export| ((*candidate).to_string(), export.rva))
            })
            .ok_or(PeError::MissingEntryExport)?;

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

        Ok(Self {
            bytes: mapped,
            sha256: format!("{:x}", Sha256::digest(file)),
            image_base: pe.image_base,
            entry_export,
            entry_rva,
            dll_entry_rva,
            section_count: pe.sections.len(),
            imports,
            has_tls: pe.tls_data.is_some(),
            has_exception_directory: pe.exception_data.is_some(),
            file_size: file.len(),
            section_protections,
        })
    }

    pub fn mapped_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    pub fn entry_address(&self) -> u64 {
        self.image_base + self.entry_rva as u64
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
}
