// PiPL category extraction (issue #871): the menu label for a registered
// filter comes from the effect's own PiPL 'catg' property — the string After
// Effects itself files the effect under. Parsed straight from the AEX bytes
// (PE resource directory → named type "PiPL" → first resource → 'catg'
// Pascal string), so no worker round-trip is needed; discovery already holds
// the bytes for hashing.
//
// The input is an arbitrary third-party binary, so every read is
// bounds-checked and every count capped; any malformation yields `None`
// (a filter without a category, never a wrong parse or a panic).

/// Longest accepted category string; PiPL categories are short menu names.
const MAX_CATEGORY_BYTES: usize = 256;
/// Caps on resource-directory fan-out, far above any real plug-in.
const MAX_DIR_ENTRIES: usize = 4096;
const MAX_SECTIONS: usize = 96;
const MAX_PIPL_PROPERTIES: u32 = 1024;
const MAX_PIPL_PROPERTY_BYTES: u32 = 65536;

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    ))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at + 4)?.try_into().ok()?,
    ))
}

/// One PE section header's mapping input: where its RVA range sits in the file.
struct SectionMap {
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

fn rva_to_offset(sections: &[SectionMap], rva: u32) -> Option<usize> {
    sections.iter().find_map(|section| {
        let span = section.virtual_size.max(section.raw_size);
        let end = section.virtual_address.checked_add(span)?;
        (section.virtual_address <= rva && rva < end)
            .then(|| (rva - section.virtual_address).checked_add(section.raw_offset))?
            .map(|offset| offset as usize)
    })
}

/// The resource directory's RVA plus the section table, from the PE headers.
fn resource_directory(bytes: &[u8]) -> Option<(u32, Vec<SectionMap>)> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(bytes, 0x3C)? as usize;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let section_count = (u16_at(bytes, pe + 6)? as usize).min(MAX_SECTIONS);
    let optional_size = u16_at(bytes, pe + 20)? as usize;
    let optional = pe + 24;
    // Data directory 2 (resources); its offset differs between PE32+ and PE32.
    let directories = match u16_at(bytes, optional)? {
        0x20B => optional + 112,
        0x10B => optional + 96,
        _ => return None,
    };
    let resource_rva = u32_at(bytes, directories + 2 * 8)?;
    if resource_rva == 0 {
        return None;
    }
    let section_table = optional + optional_size;
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let at = section_table + index * 40;
        sections.push(SectionMap {
            virtual_size: u32_at(bytes, at + 8)?,
            virtual_address: u32_at(bytes, at + 12)?,
            raw_size: u32_at(bytes, at + 16)?,
            raw_offset: u32_at(bytes, at + 20)?,
        });
    }
    Some((resource_rva, sections))
}

/// The entries of one IMAGE_RESOURCE_DIRECTORY at `dir` (an offset relative to
/// `base`, the resource section's file offset): `(name_or_id, data_field)`.
fn directory_entries(bytes: &[u8], base: usize, dir: u32) -> Option<Vec<(u32, u32)>> {
    let at = base.checked_add(dir as usize)?;
    let named = u16_at(bytes, at + 12)? as usize;
    let ids = u16_at(bytes, at + 14)? as usize;
    let count = named.checked_add(ids)?.min(MAX_DIR_ENTRIES);
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let entry = at + 16 + index * 8;
        entries.push((u32_at(bytes, entry)?, u32_at(bytes, entry + 4)?));
    }
    Some(entries)
}

/// Whether a named resource-directory entry's name equals `expected`
/// (case-insensitively; resource names are case-preserving but matched
/// case-insensitively by the loader).
fn entry_name_matches(bytes: &[u8], base: usize, name_field: u32, expected: &str) -> bool {
    if name_field & 0x8000_0000 == 0 {
        return false;
    }
    let Some(at) = base.checked_add((name_field & 0x7FFF_FFFF) as usize) else {
        return false;
    };
    let Some(length) = u16_at(bytes, at) else {
        return false;
    };
    if length as usize != expected.len() {
        return false;
    }
    expected.encode_utf16().enumerate().all(|(index, want)| {
        u16_at(bytes, at + 2 + index * 2).is_some_and(|unit| {
            // ASCII-only fold: "PiPL" is ASCII, and so is any casing of it.
            let fold = |u: u16| {
                if (b'A' as u16..=b'Z' as u16).contains(&u) {
                    u + 32
                } else {
                    u
                }
            };
            fold(unit) == fold(want)
        })
    })
}

/// A subdirectory offset from an entry's data field, or `None` for a leaf.
fn subdirectory(data_field: u32) -> Option<u32> {
    (data_field & 0x8000_0000 != 0).then_some(data_field & 0x7FFF_FFFF)
}

/// The first "PiPL" resource's bytes. The name level is walked in entry
/// order, so a multi-effect AEX (several PiPL ids) yields its first PiPL —
/// the effect the entry point serves first, which is the one the bridge
/// registers.
fn first_pipl_resource(bytes: &[u8]) -> Option<&[u8]> {
    let (resource_rva, sections) = resource_directory(bytes)?;
    let base = rva_to_offset(&sections, resource_rva)?;
    let types = directory_entries(bytes, base, 0)?;
    let pipl = types
        .into_iter()
        .find(|(name, _)| entry_name_matches(bytes, base, *name, "PiPL"))?;
    let names = directory_entries(bytes, base, subdirectory(pipl.1)?)?;
    let languages = directory_entries(bytes, base, subdirectory(names.first()?.1)?)?;
    let leaf = base.checked_add((languages.first()?.1 & 0x7FFF_FFFF) as usize)?;
    let data_rva = u32_at(bytes, leaf)?;
    let size = u32_at(bytes, leaf + 4)? as usize;
    let offset = rva_to_offset(&sections, data_rva)?;
    bytes.get(offset..offset.checked_add(size)?)
}

/// The 'catg' property out of a PiPL blob. Layout (verified against compiled
/// PiPLs and this repo's probe `.rc` sources): u32 version (1), u16 zero,
/// u32 property count, then per property the byte-swapped vendor `"MIB8"`
/// (= '8BIM'), the byte-swapped key (`"gtac"` = 'catg'), a zero u32, a u32
/// data length (padded to 4), and the data — for 'catg' a Pascal string.
fn pipl_category_of_blob(blob: &[u8]) -> Option<String> {
    if u32_at(blob, 0)? != 1 {
        return None;
    }
    let count = u32_at(blob, 6)?.min(MAX_PIPL_PROPERTIES);
    let mut at = 10usize;
    for _ in 0..count {
        if blob.get(at..at + 4)? != b"MIB8" {
            return None;
        }
        let key = blob.get(at + 4..at + 8)?;
        let length = u32_at(blob, at + 12)?.min(MAX_PIPL_PROPERTY_BYTES) as usize;
        let data = blob.get(at + 16..at.checked_add(16 + length)?)?;
        if key == b"gtac" {
            let pascal_length = *data.first()? as usize;
            return clean_category(data.get(1..1 + pascal_length)?);
        }
        at = at.checked_add(16 + length.next_multiple_of(4))?;
    }
    None
}

/// The category as a menu-safe string: control characters dropped (a `\` is
/// kept — AviUtl2 reads it as menu nesting, which a category may well want),
/// trimmed, bounded, empty folded to `None`. Adobe's own effects store the
/// category as a ZString (`$$$/MediaCore/.../Simulation=Simulation`, measured
/// across a real install); the default text after the last `=` is the
/// display name.
fn clean_category(raw: &[u8]) -> Option<String> {
    if raw.len() > MAX_CATEGORY_BYTES {
        return None;
    }
    let text: String = String::from_utf8_lossy(raw)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let text = match text.trim() {
        zstring if zstring.starts_with("$$$/") => {
            zstring.rsplit_once('=').map(|(_, name)| name).unwrap_or("")
        }
        plain => plain,
    }
    .trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The PiPL 'catg' category of an AEX image, or `None` when the file has no
/// parseable PiPL category (the filter then registers under the bare
/// "AEXCompat" label).
fn pipl_category(bytes: &[u8]) -> Option<String> {
    pipl_category_of_blob(first_pipl_resource(bytes)?)
}

/// The menu label a filter registers with (issue #871): the effect's own AE
/// category nested under "AEXCompat", or the bare "AEXCompat" without one.
/// Initial value only — AviUtl2 persists a user-editable label per effect on
/// first registration.
fn filter_label(category: Option<&str>) -> String {
    match category {
        Some(category) => format!("AEXCompat\\{category}"),
        None => "AEXCompat".to_owned(),
    }
}
