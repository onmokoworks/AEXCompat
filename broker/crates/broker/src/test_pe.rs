//! Synthetic PE images for tests (issue #304).
//!
//! `plugin_dependency_closure` walks real PE import tables, so its tests need
//! real PE bytes rather than a stubbed parser. These builders emit the smallest
//! PE32+ image that carries an import directory and a delay-load import
//! directory: one `.rdata` section holding the descriptors and the imported DLL
//! names. Nothing here is loadable by Windows, and nothing outside tests uses it.

const HEADER_SIZE: u32 = 0x200;
const SECTION_RVA: u32 = 0x1000;
const IMPORT_DESCRIPTOR_SIZE: usize = 20;
const DELAY_DESCRIPTOR_SIZE: usize = 32;

/// A PE32+ image importing `imports` through the normal import directory.
pub fn pe64_importing(imports: &[&str]) -> Vec<u8> {
    pe64_with_imports(imports, &[])
}

/// A PE32+ image importing `imports` normally and `delay_imports` through the
/// delay-load import directory.
pub fn pe64_with_imports(imports: &[&str], delay_imports: &[&str]) -> Vec<u8> {
    let import_table_size = (imports.len() + 1) * IMPORT_DESCRIPTOR_SIZE;
    let delay_table_size = (delay_imports.len() + 1) * DELAY_DESCRIPTOR_SIZE;
    let names_offset = import_table_size + delay_table_size;

    // Lay the name strings out first so every descriptor can point at one.
    let mut names = Vec::new();
    let mut name_rva = Vec::new();
    for name in imports.iter().chain(delay_imports) {
        name_rva.push(SECTION_RVA + (names_offset + names.len()) as u32);
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }

    let mut section = Vec::with_capacity(names_offset + names.len());
    for rva in name_rva.iter().take(imports.len()) {
        push_u32(&mut section, 0); // original_first_thunk
        push_u32(&mut section, 0); // time_date_stamp
        push_u32(&mut section, 0); // forwarder_chain
        push_u32(&mut section, *rva); // name
        push_u32(&mut section, 0); // first_thunk
    }
    section.extend(std::iter::repeat_n(0u8, IMPORT_DESCRIPTOR_SIZE));
    for rva in name_rva.iter().skip(imports.len()) {
        push_u32(&mut section, 1); // attributes: RVA-based descriptor
        push_u32(&mut section, *rva); // dll_name_rva
        for _ in 0..6 {
            push_u32(&mut section, 0);
        }
    }
    section.extend(std::iter::repeat_n(0u8, DELAY_DESCRIPTOR_SIZE));
    section.extend_from_slice(&names);
    let section_size = section.len() as u32;
    while section.len() % HEADER_SIZE as usize != 0 {
        section.push(0);
    }

    let mut image = Vec::new();
    // DOS header: only the magic and the NT header offset matter.
    image.extend_from_slice(b"MZ");
    image.extend(std::iter::repeat_n(0u8, 0x3a));
    push_u32(&mut image, 0x40);

    image.extend_from_slice(b"PE\0\0");
    push_u16(&mut image, 0x8664); // machine: x86-64
    push_u16(&mut image, 1); // number_of_sections
    push_u32(&mut image, 0); // time_date_stamp
    push_u32(&mut image, 0); // pointer_to_symbol_table
    push_u32(&mut image, 0); // number_of_symbols
    push_u16(&mut image, 240); // size_of_optional_header (PE32+, 16 directories)
    push_u16(&mut image, 0x2022); // executable | large address aware | DLL

    push_u16(&mut image, 0x20b); // PE32+
    image.extend_from_slice(&[14, 0]); // linker version
    push_u32(&mut image, 0); // size_of_code
    push_u32(&mut image, section_size); // size_of_initialized_data
    push_u32(&mut image, 0); // size_of_uninitialized_data
    push_u32(&mut image, 0); // address_of_entry_point
    push_u32(&mut image, SECTION_RVA); // base_of_code
    push_u64(&mut image, 0x1_8000_0000); // image_base
    push_u32(&mut image, 0x1000); // section_alignment
    push_u32(&mut image, HEADER_SIZE); // file_alignment
    push_u16(&mut image, 6); // major_operating_system_version
    push_u16(&mut image, 0);
    push_u16(&mut image, 0); // major_image_version
    push_u16(&mut image, 0);
    push_u16(&mut image, 6); // major_subsystem_version
    push_u16(&mut image, 0);
    push_u32(&mut image, 0); // win32_version_value
    push_u32(&mut image, SECTION_RVA + 0x1000); // size_of_image
    push_u32(&mut image, HEADER_SIZE); // size_of_headers
    push_u32(&mut image, 0); // checksum
    push_u16(&mut image, 3); // subsystem: console
    push_u16(&mut image, 0); // dll_characteristics
    for _ in 0..4 {
        push_u64(&mut image, 0x1000); // stack/heap reserve and commit
    }
    push_u32(&mut image, 0); // loader_flags
    push_u32(&mut image, 16); // number_of_rva_and_sizes
    for index in 0..16 {
        match index {
            1 => {
                push_u32(&mut image, SECTION_RVA);
                push_u32(&mut image, import_table_size as u32);
            }
            13 => {
                push_u32(&mut image, SECTION_RVA + import_table_size as u32);
                push_u32(&mut image, delay_table_size as u32);
            }
            _ => {
                push_u32(&mut image, 0);
                push_u32(&mut image, 0);
            }
        }
    }

    image.extend_from_slice(b".rdata\0\0");
    push_u32(&mut image, section_size); // virtual_size
    push_u32(&mut image, SECTION_RVA); // virtual_address
    push_u32(&mut image, section.len() as u32); // size_of_raw_data
    push_u32(&mut image, HEADER_SIZE); // pointer_to_raw_data
    push_u32(&mut image, 0); // pointer_to_relocations
    push_u32(&mut image, 0); // pointer_to_linenumbers
    push_u16(&mut image, 0); // number_of_relocations
    push_u16(&mut image, 0); // number_of_linenumbers
    push_u32(&mut image, 0x4000_0040); // initialized data, read

    while image.len() < HEADER_SIZE as usize {
        image.push(0);
    }
    image.extend_from_slice(&section);
    image
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
