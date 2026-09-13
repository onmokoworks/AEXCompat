use std::collections::BTreeMap;

// The initial virtual hive has no host accounts or installation records. Only
// Everyone ACEs can currently be evaluated without inventing a Windows token.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegistrySecurity {
    pub(crate) dacl: Option<Vec<RegistryAce>>,
    pub(crate) protected: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct RegistryAce {
    pub(crate) deny: bool,
    pub(crate) flags: u8,
    pub(crate) mask: u32,
}
fn map_access(mut mask: u32) -> u32 {
    for (generic, specific) in [
        (0x80000000, 0x20019),
        (0x40000000, 0x20006),
        (0x20000000, 0x20019),
        (0x10000000, 0xf003f),
    ] {
        if mask & generic != 0 {
            mask = (mask & !generic) | specific;
        }
    }
    mask & !0x300
}
impl RegistrySecurity {
    fn permits(&self, access: u32) -> bool {
        let mut remaining = map_access(access);
        if remaining & !0xf003f != 0 {
            return false;
        }
        let Some(entries) = &self.dacl else {
            return true;
        };
        for ace in entries {
            if remaining == 0 {
                break;
            }
            if ace.flags & 8 != 0 {
                continue;
            }
            let matched = map_access(ace.mask) & remaining;
            if ace.deny && matched != 0 {
                return false;
            }
            if !ace.deny {
                remaining &= !matched;
            }
        }
        remaining == 0
    }
    fn inherited(&self) -> Self {
        Self {
            protected: false,
            dacl: self.dacl.as_ref().map(|entries| {
                entries
                    .iter()
                    .filter_map(|ace| {
                        let mut inherited = ace.clone();
                        if ace.flags & 2 != 0 {
                            inherited.flags &= !8;
                        } else if ace.flags & 1 != 0 && ace.flags & 4 == 0 {
                            inherited.flags |= 8;
                        } else {
                            return None;
                        }
                        if ace.flags & 4 != 0 {
                            inherited.flags &= !7;
                        }
                        inherited.flags |= 0x10;
                        Some(inherited)
                    })
                    .collect()
            }),
        }
    }
    fn for_child(parent: &Self, explicit: Option<&Self>) -> Self {
        let inherited = parent.inherited();
        let Some(explicit) = explicit else {
            return inherited;
        };
        let mut security = explicit.clone();
        if !security.protected {
            if let (Some(entries), Some(parent_entries)) = (&mut security.dacl, inherited.dacl) {
                entries.extend(parent_entries);
            }
        }
        security
    }
}

#[derive(Default)]
pub(crate) struct GuestRegistry {
    keys: BTreeMap<(u64, u32, Vec<u8>), RegistrySecurity>,
    inheritable: BTreeMap<u64, bool>,
    handles: BTreeMap<u64, (u64, u32, Vec<u8>, u32)>,
    issued: u64,
    values: BTreeMap<(u64, u32, Vec<u8>, Vec<u8>), (u32, Vec<u8>)>,
    value_bytes: usize,
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
        self.open_with_security(key, name, access, create, None, false)
    }
    pub(crate) fn open_with_security(
        &mut self,
        key: u64,
        name: &[u8],
        access: u32,
        create: bool,
        security: Option<RegistrySecurity>,
        inherit_handle: bool,
    ) -> Result<(u64, bool), u32> {
        let view = access & 0x300;
        if view == 0x300 {
            return Err(87);
        }
        if map_access(access) & !0xf003f != 0 {
            return Err(5);
        }
        let (root, view, mut path) = self.resolve(key, view)?;
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
        let existed = path.is_empty() || self.keys.contains_key(&identity);
        if !create && !existed {
            return Err(2);
        }
        if self.handles.len() >= 256 || self.issued >= 65536 {
            return Err(8);
        }
        let mut additions = Vec::new();
        let default_security = RegistrySecurity::default();
        if existed
            && !self
                .keys
                .get(&identity)
                .unwrap_or(&default_security)
                .permits(access)
        {
            return Err(5);
        }
        if create && !existed {
            let mut parent_security = default_security.clone();
            for end in (0..path.len())
                .filter(|i| path[*i] == b'\\')
                .chain(std::iter::once(path.len()))
            {
                let part = (root, view, path[..end].to_vec());
                if let Some(existing) = self.keys.get(&part) {
                    parent_security = existing.clone();
                } else {
                    if !parent_security.permits(4) {
                        return Err(5);
                    }
                    let child_security = RegistrySecurity::for_child(
                        &parent_security,
                        if end == path.len() {
                            security.as_ref()
                        } else {
                            None
                        },
                    );
                    parent_security = child_security.clone();
                    additions.push((part, child_security));
                }
            }
            if self.keys.len() + additions.len() > 1024 {
                return Err(8);
            }
        }
        self.keys.extend(additions);
        let handle = 0x0000_000d_0000_0000 + self.issued * 8;
        self.issued += 1;
        self.handles
            .insert(handle, (root, view, path, map_access(access)));
        self.inheritable.insert(handle, inherit_handle);
        Ok((handle, !existed))
    }

    fn value_identity(
        &self,
        key: u64,
        name: &[u8],
        access: u32,
    ) -> Result<(u64, u32, Vec<u8>, Vec<u8>), u32> {
        let (root, view, path) = self.resolve(key, 0)?;
        if self
            .handles
            .get(&key)
            .is_some_and(|h| h.3 & access != access)
        {
            return Err(5);
        }
        if name.len() > 16383 || !name.is_ascii() {
            return Err(87);
        }
        Ok((
            root,
            view,
            path,
            name.iter().map(u8::to_ascii_lowercase).collect(),
        ))
    }
    pub(crate) fn set_value(
        &mut self,
        key: u64,
        name: &[u8],
        kind: u32,
        data: Vec<u8>,
    ) -> Result<(), u32> {
        let identity = self.value_identity(key, name, 2)?;
        let previous = self
            .values
            .get(&identity)
            .map_or(0, |(_, bytes)| bytes.len());
        let total = self.value_bytes - previous + data.len();
        if data.len() > 2 * 1024 * 1024
            || total > 16 * 1024 * 1024
            || (!self.values.contains_key(&identity) && self.values.len() >= 4096)
        {
            return Err(8);
        }
        self.values.insert(identity, (kind, data));
        self.value_bytes = total;
        Ok(())
    }
    pub(crate) fn query_value(&self, key: u64, name: &[u8]) -> Result<&(u32, Vec<u8>), u32> {
        let identity = self.value_identity(key, name, 1)?;
        self.values.get(&identity).ok_or(2)
    }
    pub(crate) fn close(&mut self, key: u64) -> Result<(), u32> {
        self.handles.remove(&key).ok_or(6u32)?;
        self.inheritable.remove(&key);
        Ok(())
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
    #[test]
    fn security_persists_and_creation_checks_parent_descriptor() {
        let mut registry = GuestRegistry::default();
        let root = 0xffff_ffff_8000_0001;
        let policy = RegistrySecurity {
            protected: true,
            dacl: Some(vec![
                RegistryAce {
                    deny: true,
                    flags: 2,
                    mask: 2,
                },
                RegistryAce {
                    deny: false,
                    flags: 2,
                    mask: 0x10000000,
                },
            ]),
        };
        let (handle, _) = registry
            .open_with_security(root, b"Parent", 0x20019, true, Some(policy), true)
            .unwrap();
        assert_eq!(registry.inheritable.get(&handle), Some(&true));
        // A read handle can create a child if the parent's DACL allows it.
        let (child, _) = registry.open(handle, b"Child", 0x20019, true).unwrap();
        registry.close(child).unwrap();
        assert_eq!(registry.open(root, b"Parent\\Child", 2, false), Err(5));
        assert!(
            registry
                .open(root, b"Parent\\Child", 0x80000000, false)
                .is_ok()
        );
        // Re-creation never replaces the existing descriptor with a null DACL.
        assert_eq!(
            registry.open_with_security(
                root,
                b"Parent",
                2,
                true,
                Some(RegistrySecurity::default()),
                false
            ),
            Err(5)
        );
        registry.close(handle).unwrap();
        assert!(!registry.inheritable.contains_key(&handle));
        let blocked = RegistrySecurity {
            protected: true,
            dacl: Some(vec![]),
        };
        let (handle, _) = registry
            .open_with_security(root, b"Blocked", 0, true, Some(blocked), false)
            .unwrap();
        assert_eq!(registry.open(handle, b"Child", 0, true), Err(5));
        assert_eq!(registry.open(root, b"Blocked", 1, false), Err(5));
        assert_eq!(registry.open(root, b"Blocked\\Child", 0, false), Err(2));
    }
    #[test]
    fn acl_order_null_empty_and_inheritance_flags() {
        let allow = RegistryAce {
            deny: false,
            flags: 0,
            mask: 1,
        };
        let deny = RegistryAce {
            deny: true,
            flags: 0,
            mask: 1,
        };
        assert!(RegistrySecurity::default().permits(1));
        let mut policy = RegistrySecurity {
            protected: false,
            dacl: Some(vec![]),
        };
        assert!(!policy.permits(1));
        policy.dacl = Some(vec![allow.clone(), deny.clone()]);
        assert!(policy.permits(1));
        policy.dacl = Some(vec![deny, allow.clone()]);
        assert!(!policy.permits(1));
        policy.dacl = Some(vec![RegistryAce {
            flags: 2 | 4 | 8,
            ..allow.clone()
        }]);
        assert!(!policy.permits(1));
        let child = policy.inherited();
        assert!(child.permits(1));
        assert!(!child.inherited().permits(1));
        policy.dacl = Some(vec![RegistryAce { flags: 1, ..allow }]);
        assert!(!policy.inherited().permits(1));
    }
}
