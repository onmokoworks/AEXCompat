use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct GuestRegistry {
    keys: BTreeSet<(u64, u32, Vec<u8>)>,
    handles: BTreeMap<u64, (u64, u32, Vec<u8>, u32)>,
    issued: u64,
}
impl GuestRegistry {
    pub(crate) fn resolve(&self, key: u64, view: u32) -> Result<(u64, u32, Vec<u8>), u32> {
        if matches!(
            key,
            0xffff_ffff_8000_0000..=0xffff_ffff_8000_0003 | 0xffff_ffff_8000_0005
        ) {
            return Ok((key, view, Vec::new()));
        }
        self.handles
            .get(&key)
            .map(|(root, stored_view, path, _)| {
                (
                    *root,
                    if view == 0 { *stored_view } else { view },
                    path.clone(),
                )
            })
            .ok_or(6)
    }
    pub(crate) fn open(
        &mut self,
        key: u64,
        name: &[u8],
        access: u32,
        create: bool,
    ) -> Result<(u64, bool), u32> {
        let view = access & 0x300;
        if view == 0x300 {
            return Err(87);
        }
        let (root, view, mut path) = self.resolve(key, view)?;
        if create && self.handles.get(&key).is_some_and(|h| h.3 & 4 == 0) {
            return Err(5);
        }
        if name.len() > 32767 || !name.is_ascii() {
            return Err(87);
        }
        for part in name.split(|b| *b == b'\\').filter(|p| !p.is_empty()) {
            if !path.is_empty() {
                path.push(b'\\');
            }
            path.extend(part.iter().map(u8::to_ascii_lowercase));
        }
        let identity = (root, view, path.clone());
        let existed = path.is_empty() || self.keys.contains(&identity);
        if !create && !existed {
            return Err(2);
        }
        if self.handles.len() >= 256 || self.issued >= 65536 {
            return Err(8);
        }
        let mut additions = Vec::new();
        if create && !existed {
            for end in (0..path.len())
                .filter(|i| path[*i] == b'\\')
                .chain(std::iter::once(path.len()))
            {
                let part = (root, view, path[..end].to_vec());
                if !self.keys.contains(&part) {
                    additions.push(part);
                }
            }
            if self.keys.len() + additions.len() > 1024 {
                return Err(8);
            }
        }
        self.keys.extend(additions);
        let handle = 0x0000_000d_0000_0000 + self.issued * 8;
        self.issued += 1;
        self.handles.insert(handle, (root, view, path, access));
        Ok((handle, !existed))
    }
    pub(crate) fn close(&mut self, key: u64) -> Result<(), u32> {
        self.handles.remove(&key).map(|_| ()).ok_or(6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_survives_close_and_views_are_separate() {
        let mut registry = GuestRegistry::default();
        let root = 0xffff_ffff_8000_0001;
        let (handle, created) = registry
            .open(root, b"Software\\Example", 0x2001f, true)
            .unwrap();
        assert!(created);
        registry.close(handle).unwrap();
        assert!(registry.close(handle).is_err());
        let (_, created) = registry
            .open(root, b"software\\EXAMPLE", 0x20019, false)
            .unwrap();
        assert!(!created);
        assert!(
            registry
                .open(root, b"Software\\Example", 0x20219, false)
                .is_err()
        );
    }
}
