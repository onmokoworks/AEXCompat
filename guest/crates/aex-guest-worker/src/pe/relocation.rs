//! PE32+ IMAGE_REL_BASED_DIR64 relocation for dependency DLL guest mappings.
use super::PeError;
use std::collections::BTreeSet;

fn invalid(message: &str) -> PeError {
    PeError::Relocation(message.to_string())
}

pub(super) fn apply(
    image: &mut [u8],
    directory: Option<(usize, usize)>,
    old_base: u64,
    new_base: u64,
) -> Result<(), PeError> {
    if old_base == new_base {
        return Ok(());
    }
    let (start, size) = directory.ok_or_else(|| invalid("image has no relocation directory"))?;
    if size == 0 || size > 32 * 1024 * 1024 {
        return Err(invalid("relocation directory size outside bounds"));
    }
    let end = start
        .checked_add(size)
        .ok_or_else(|| invalid("directory overflow"))?;
    let table = image
        .get(start..end)
        .ok_or_else(|| invalid("directory outside image"))?;
    let mut cursor = 0;
    let mut targets = BTreeSet::new();
    // Validate the entire table before mutating bytes, including entries which
    // target the relocation table itself. Overlapping fixups are ambiguous.
    while cursor < table.len() {
        let header = table
            .get(cursor..cursor + 8)
            .ok_or_else(|| invalid("truncated block header"))?;
        let page = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if page % 4096 != 0 || length < 8 || length % 4 != 0 {
            return Err(invalid("invalid relocation block alignment or size"));
        }
        let block_end = cursor
            .checked_add(length)
            .ok_or_else(|| invalid("block overflow"))?;
        let entries = table
            .get(cursor + 8..block_end)
            .ok_or_else(|| invalid("truncated block entries"))?;
        for entry in entries.chunks_exact(2) {
            let encoded = u16::from_le_bytes(entry.try_into().unwrap());
            let kind = encoded >> 12;
            if kind == 0 {
                continue;
            }
            if kind != 10 {
                return Err(invalid("unsupported AMD64 relocation type"));
            }
            let target = page
                .checked_add(usize::from(encoded & 0xfff))
                .ok_or_else(|| invalid("target overflow"))?;
            let target_end = target
                .checked_add(8)
                .ok_or_else(|| invalid("target range overflow"))?;
            if target_end > image.len() {
                return Err(invalid("target outside image"));
            }
            if targets
                .range(target.saturating_sub(7)..target_end)
                .next()
                .is_some()
            {
                return Err(invalid("overlapping relocation targets"));
            }
            targets.insert(target);
            if targets.len() > 1024 * 1024 {
                return Err(invalid("too many relocation targets"));
            }
        }
        cursor = block_end;
    }
    let delta = new_base.wrapping_sub(old_base);
    for target in targets {
        let value = u64::from_le_bytes(image[target..target + 8].try_into().unwrap());
        image[target..target + 8].copy_from_slice(&value.wrapping_add(delta).to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(entries: [u16; 2]) -> Vec<u8> {
        let mut bytes = vec![0; 0x3000];
        bytes[0x100..0x104].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[0x104..0x108].copy_from_slice(&12u32.to_le_bytes());
        bytes[0x108..0x10a].copy_from_slice(&entries[0].to_le_bytes());
        bytes[0x10a..0x10c].copy_from_slice(&entries[1].to_le_bytes());
        bytes[0x1010..0x1018].copy_from_slice(&0x180001234u64.to_le_bytes());
        bytes
    }

    #[test]
    fn relocates_up_and_down_and_ignores_absolute_padding() {
        for base in [0x1000000000, 0x100000000] {
            let mut bytes = fixture([0xa010, 0]);
            let unchanged = bytes[..0x1000].to_vec();
            apply(&mut bytes, Some((0x100, 12)), 0x180000000, base).unwrap();
            assert_eq!(
                u64::from_le_bytes(bytes[0x1010..0x1018].try_into().unwrap()),
                base + 0x1234
            );
            assert_eq!(bytes[..0x1000], unchanged);
        }
    }

    #[test]
    fn malformed_fixups_fail_before_any_image_mutation() {
        for entries in [[0xa010, 0xa010], [0xa010, 0xa014], [0xa010, 0x3018]] {
            let mut bytes = fixture(entries);
            let original = bytes.clone();
            assert!(apply(&mut bytes, Some((0x100, 12)), 0x180000000, 0x200000000).is_err());
            assert_eq!(bytes, original);
        }
        for size in [1, 7, 9, 10, 11, 13, 0x3000] {
            let mut bytes = fixture([0xa010, 0]);
            assert!(apply(&mut bytes, Some((0x100, size)), 1, 2).is_err());
        }
        let mut bytes = fixture([0xafff, 0]);
        bytes[0x100..0x104].copy_from_slice(&0x2000u32.to_le_bytes());
        assert!(apply(&mut bytes, Some((0x100, 12)), 1, 2).is_err());
        assert!(apply(&mut bytes, None, 1, 2).is_err());
        assert!(apply(&mut bytes, None, 1, 1).is_ok());
    }
}
