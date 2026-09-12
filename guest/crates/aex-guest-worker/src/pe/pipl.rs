//! Bounded Windows PiPL resource discovery. The resource declares the Effect
//! ABI and export; arbitrary export names are never guessed as Effect entries.
use super::PeError;

fn invalid(message: &str) -> PeError {
    PeError::InvalidPipl(message.to_string())
}

fn bytes(data: &[u8], offset: usize, size: usize) -> Result<&[u8], PeError> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| invalid("range overflow"))?;
    data.get(offset..end)
        .ok_or_else(|| invalid("truncated resource"))
}

fn u16_at(data: &[u8], offset: usize) -> Result<u16, PeError> {
    Ok(u16::from_le_bytes(
        bytes(data, offset, 2)?.try_into().unwrap(),
    ))
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, PeError> {
    Ok(u32::from_le_bytes(
        bytes(data, offset, 4)?.try_into().unwrap(),
    ))
}

fn directory(data: &[u8], offset: usize) -> Result<Vec<(u32, u32)>, PeError> {
    let count = usize::from(u16_at(data, offset + 12)?) + usize::from(u16_at(data, offset + 14)?);
    if count > 4096 {
        return Err(invalid("resource directory has too many entries"));
    }
    let entries = bytes(data, offset + 16, count * 8)?;
    entries
        .chunks_exact(8)
        .map(|entry| Ok((u32_at(entry, 0)?, u32_at(entry, 4)?)))
        .collect()
}

fn subdirectory(target: u32) -> Result<usize, PeError> {
    if target & 0x8000_0000 == 0 {
        return Err(invalid("expected resource subdirectory"));
    }
    Ok((target & 0x7fff_ffff) as usize)
}

pub(super) fn effect_entry(
    mapped: &[u8],
    start: usize,
    size: usize,
) -> Result<Option<String>, PeError> {
    if start == 0 && size == 0 {
        return Ok(None);
    }
    let resources = bytes(mapped, start, size)?;
    let mut result = None;
    let mut payload_count = 0;
    for (name, target) in directory(resources, 0)? {
        if name & 0x8000_0000 == 0 {
            continue;
        }
        let offset = (name & 0x7fff_ffff) as usize;
        let length = usize::from(u16_at(resources, offset)?);
        let name = bytes(resources, offset + 2, length * 2)?;
        if !name.eq_ignore_ascii_case(b"P\0i\0P\0L\0") {
            continue;
        }
        for (_, target) in directory(resources, subdirectory(target)?)? {
            for (_, target) in directory(resources, subdirectory(target)?)? {
                payload_count += 1;
                if payload_count > 64 || target & 0x8000_0000 != 0 {
                    return Err(invalid("invalid PiPL resource depth or count"));
                }
                let leaf = bytes(resources, target as usize, 16)?;
                let rva = u32_at(leaf, 0)? as usize;
                let length = u32_at(leaf, 4)? as usize;
                if length > 1024 * 1024 {
                    return Err(invalid("PiPL exceeds size limit"));
                }
                // Payloads must be inside the declared resource directory span.
                let relative = rva
                    .checked_sub(start)
                    .ok_or_else(|| invalid("PiPL outside resources"))?;
                let entry = parse(bytes(resources, relative, length)?)?;
                if result.as_ref().is_some_and(|previous| previous != &entry) {
                    return Err(invalid("ambiguous Effect exports"));
                }
                result = Some(entry);
            }
        }
        if payload_count == 0 {
            return Err(invalid("empty PiPL resource tree"));
        }
    }
    Ok(result)
}

fn parse(data: &[u8]) -> Result<String, PeError> {
    if data.len() < 10 || u32_at(data, 0)? > 1 || u16_at(data, 4)? != 0 || u16_at(data, 8)? != 0 {
        return Err(invalid("invalid PiPL header"));
    }
    let count = u16_at(data, 6)?;
    if count == 0 || count > 256 {
        return Err(invalid("invalid property count"));
    }
    let mut offset = 10;
    let mut kind = None;
    let mut symbol = None;
    for _ in 0..count {
        let header = bytes(data, offset, 16)?;
        let length = u32_at(header, 12)? as usize;
        offset += 16;
        let padded = length
            .checked_add(3)
            .ok_or_else(|| invalid("property size overflow"))?
            & !3;
        let payload = bytes(data, offset, padded)?;
        if payload[length..].iter().any(|byte| *byte != 0) {
            return Err(invalid("nonzero property padding"));
        }
        if &header[..4] == b"MIB8" {
            match &header[4..8] {
                b"dnik" => {
                    if kind.is_some() || length != 4 || u32_at(header, 8)? != 0 {
                        return Err(invalid("invalid or duplicate Kind property"));
                    }
                    kind = Some(payload[..length].to_vec());
                }
                b"4668" => {
                    if symbol.is_some() || length == 0 || length > 256 || u32_at(header, 8)? != 0 {
                        return Err(invalid("invalid or duplicate CodeWin64X86 property"));
                    }
                    let value = &payload[..length];
                    let end = value
                        .iter()
                        .position(|byte| *byte == 0)
                        .ok_or_else(|| invalid("unterminated export"))?;
                    if end == 0
                        || end > 127
                        || value[end..].iter().any(|byte| *byte != 0)
                        || !(value[0].is_ascii_alphabetic() || value[0] == b'_')
                        || !value[..end]
                            .iter()
                            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                    {
                        return Err(invalid("invalid export name"));
                    }
                    symbol = Some(
                        String::from_utf8(value[..end].to_vec())
                            .map_err(|_| invalid("invalid export encoding"))?,
                    );
                }
                _ => {}
            }
        }
        offset += padded;
    }
    if offset != data.len() {
        return Err(invalid("trailing PiPL bytes"));
    }
    if kind.as_deref() != Some(b"TKFe") {
        return Err(invalid("PiPL does not declare an Effect ABI"));
    }
    symbol.ok_or_else(|| invalid("missing CodeWin64X86 property"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipl(kind: &[u8; 4], name: &[u8]) -> Vec<u8> {
        let mut data = vec![1, 0, 0, 0, 0, 0, 2, 0, 0, 0];
        for (key, value) in [(b"dnik", kind.as_slice()), (b"4668", name)] {
            data.extend_from_slice(b"MIB8");
            data.extend_from_slice(key);
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&(value.len() as u32).to_le_bytes());
            data.extend_from_slice(value);
            data.resize(10 + (data.len() - 10 + 3) / 4 * 4, 0);
        }
        data
    }

    #[test]
    fn declares_arbitrary_effect_export_without_guessing_its_name() {
        assert_eq!(
            parse(&pipl(b"TKFe", b"CustomEffect_42\0")).unwrap(),
            "CustomEffect_42"
        );
        assert!(parse(&pipl(b"xgEA", b"CustomEffect_42\0")).is_err());
        assert!(parse(&pipl(b"TKFe", b"unterminated")).is_err());
        assert!(parse(&pipl(b"TKFe", b"a\0hidden")).is_err());
    }

    #[test]
    fn rejects_truncation_trailing_bytes_and_duplicate_kind() {
        let valid = pipl(b"TKFe", b"entry\0");
        for end in 0..valid.len() {
            assert!(parse(&valid[..end]).is_err(), "truncation {end}");
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(parse(&trailing).is_err());
        let mut duplicate = valid;
        duplicate[6] = 3;
        duplicate.extend_from_within(10..30);
        assert!(parse(&duplicate).is_err());
    }

    fn resource(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[16..20].copy_from_slice(&0x8000_0060u32.to_le_bytes());
        data[20..24].copy_from_slice(&0x8000_0018u32.to_le_bytes());
        data[38..40].copy_from_slice(&1u16.to_le_bytes());
        data[44..48].copy_from_slice(&0x8000_0030u32.to_le_bytes());
        data[62..64].copy_from_slice(&1u16.to_le_bytes());
        data[68..72].copy_from_slice(&72u32.to_le_bytes());
        data[72..76].copy_from_slice(&128u32.to_le_bytes());
        data[76..80].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        data[96..98].copy_from_slice(&4u16.to_le_bytes());
        data[98..106].copy_from_slice(b"P\0i\0P\0L\0");
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn follows_resource_tree_and_bounds_every_indirection() {
        let valid = resource(&pipl(b"TKFe", b"chosenEntry\0"));
        assert_eq!(
            effect_entry(&valid, 0, valid.len()).unwrap().as_deref(),
            Some("chosenEntry")
        );
        let mut uppercase = valid.clone();
        uppercase[100] = b'I';
        assert_eq!(
            effect_entry(&uppercase, 0, uppercase.len()).unwrap(),
            Some("chosenEntry".to_string())
        );
        for (offset, value) in [
            (20, 0xffff_ffff),
            (44, 0x8000_0000),
            (68, 0x8000_0030),
            (72, 0xffff_ffff),
            (76, 0xffff_ffff),
        ] {
            let mut bad = valid.clone();
            bad[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
            assert!(effect_entry(&bad, 0, bad.len()).is_err(), "offset {offset}");
        }
        for size in 1..valid.len() {
            assert!(
                effect_entry(&valid, 0, size).is_err(),
                "resource size {size}"
            );
        }
    }

    fn pe_fixture(export: &str, declared: &[u8]) -> Vec<u8> {
        let mut resources = resource(&pipl(b"TKFe", declared));
        resources[72..76].copy_from_slice(&0x2080u32.to_le_bytes());
        let mut file = vec![0u8; 0x600];
        let put16 = |file: &mut [u8], offset, value: u16| {
            file[offset..offset + 2].copy_from_slice(&value.to_le_bytes())
        };
        let put32 = |file: &mut [u8], offset, value: u32| {
            file[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
        };
        file[..2].copy_from_slice(b"MZ");
        put32(&mut file, 60, 0x80);
        file[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut file, 0x84, 0x8664);
        put16(&mut file, 0x86, 2);
        put16(&mut file, 0x94, 240);
        put16(&mut file, 0x96, 0x2022);
        let optional = 0x98;
        put16(&mut file, optional, 0x20b);
        file[optional + 24..optional + 32].copy_from_slice(&0x180000000u64.to_le_bytes());
        for (offset, value) in [
            (32, 0x1000),
            (36, 0x200),
            (56, 0x3000),
            (60, 0x200),
            (108, 16),
            (112, 0x1100),
            (116, 0x80),
            (128, 0x2000),
            (132, resources.len() as u32),
        ] {
            put32(&mut file, optional + offset, value);
        }
        for (index, name, va, raw, flags) in [
            (0, b".text", 0x1000, 0x200, 0x60000020),
            (1, b".rsrc", 0x2000, 0x400, 0x40000040),
        ] {
            let section = optional + 240 + index * 40;
            file[section..section + 5].copy_from_slice(name);
            for (offset, value) in [(8, 0x200), (12, va), (16, 0x200), (20, raw), (36, flags)] {
                put32(&mut file, section + offset, value);
            }
        }
        // One named executable export outside the export directory range.
        file[0x200] = 0xc3;
        for (offset, value) in [
            (12, 0x1180),
            (16, 1),
            (20, 1),
            (24, 1),
            (28, 0x1128),
            (32, 0x112c),
            (36, 0x1130),
            (40, 0x1000),
            (44, 0x1160),
        ] {
            put32(&mut file, 0x300 + offset, value);
        }
        file[0x360..0x360 + export.len()].copy_from_slice(export.as_bytes());
        file[0x380..0x386].copy_from_slice(b"probe\0");
        file[0x400..0x400 + resources.len()].copy_from_slice(&resources);
        file
    }

    #[test]
    fn mapped_pe_uses_declared_export_and_preserves_existing_discovery() {
        use super::super::{PeError, PeImage};
        let file = pe_fixture("customEntry", b"customEntry\0");
        let image = PeImage::parse_and_map(&file).unwrap();
        assert_eq!(image.entry_address(), Some(0x180001000));
        let missing = pe_fixture("customEntry", b"missing\0");
        assert!(matches!(
            PeImage::parse_and_map(&missing),
            Err(PeError::InvalidPipl(_))
        ));
        let mut non_executable = file;
        non_executable[0x328..0x32c].copy_from_slice(&0x2000u32.to_le_bytes());
        assert!(matches!(
            PeImage::parse_and_map(&non_executable),
            Err(PeError::NonExecutableExport(_))
        ));
        // Existing entry paths retain their prior behavior with unusable PiPL.
        for export in [
            "EffectMain",
            "entryPointFunc",
            "entry_point",
            "PluginDataEntryFunction2",
            "PluginDataEntryFunction",
        ] {
            let file = pe_fixture(export, b"unterminated");
            assert!(PeImage::parse_and_map(&file).is_ok(), "{export}");
        }
    }
}
