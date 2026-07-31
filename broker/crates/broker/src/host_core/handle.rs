use crate::host_core::boundary::HostOpaqueHandle;
use crate::host_core::error::{HostError, HostErrorCode};

const KIND_BITS: u32 = 8;
const SLOT_BITS: u32 = 24;
const SLOT_MASK: u64 = (1_u64 << SLOT_BITS) - 1;
const MAX_SLOTS: usize = SLOT_MASK as usize;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandleKind {
    Scene = 1,
    World = 2,
    Parameter = 3,
    Session = 4,
    Report = 5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerId(u64);

impl OwnerId {
    pub fn new(value: u64) -> Result<Self, HostError> {
        if value == 0 {
            return Err(HostError::new(
                HostErrorCode::InvalidArgument,
                "create_owner_id",
            ));
        }
        Ok(Self(value))
    }
}

struct Entry<T> {
    generation: u32,
    owner: OwnerId,
    kind: HandleKind,
    value: Option<T>,
}

pub struct HandleRegistry<T> {
    entries: Vec<Entry<T>>,
    live: usize,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            live: 0,
        }
    }
}

impl<T> HandleRegistry<T> {
    pub fn insert(
        &mut self,
        owner: OwnerId,
        kind: HandleKind,
        value: T,
    ) -> Result<HostOpaqueHandle, HostError> {
        if let Some((slot, entry)) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.value.is_none())
        {
            entry.generation = entry.generation.checked_add(1).ok_or_else(|| {
                HostError::new(HostErrorCode::CapacityExceeded, "reuse_handle_slot")
            })?;
            entry.owner = owner;
            entry.kind = kind;
            entry.value = Some(value);
            self.live += 1;
            return Ok(encode(slot, entry.generation, kind));
        }
        if self.entries.len() >= MAX_SLOTS {
            return Err(HostError::new(
                HostErrorCode::CapacityExceeded,
                "allocate_handle_slot",
            ));
        }
        let slot = self.entries.len();
        self.entries.push(Entry {
            generation: 1,
            owner,
            kind,
            value: Some(value),
        });
        self.live += 1;
        Ok(encode(slot, 1, kind))
    }

    pub fn get(
        &self,
        handle: HostOpaqueHandle,
        owner: OwnerId,
        expected_kind: HandleKind,
    ) -> Result<&T, HostError> {
        let (_, entry) = self.resolve(handle, owner, expected_kind, "resolve_handle")?;
        entry
            .value
            .as_ref()
            .ok_or_else(|| HostError::new(HostErrorCode::StaleHandle, "resolve_handle"))
    }

    pub fn remove(
        &mut self,
        handle: HostOpaqueHandle,
        owner: OwnerId,
        expected_kind: HandleKind,
    ) -> Result<T, HostError> {
        let (slot, _) = self.resolve(handle, owner, expected_kind, "dispose_handle")?;
        let value = self.entries[slot]
            .value
            .take()
            .ok_or_else(|| HostError::new(HostErrorCode::StaleHandle, "dispose_handle"))?;
        self.live -= 1;
        Ok(value)
    }

    pub fn live_count(&self) -> usize {
        self.live
    }

    fn resolve(
        &self,
        handle: HostOpaqueHandle,
        owner: OwnerId,
        expected_kind: HandleKind,
        operation: &'static str,
    ) -> Result<(usize, &Entry<T>), HostError> {
        let (slot, generation, encoded_kind) = decode(handle)
            .ok_or_else(|| HostError::new(HostErrorCode::InvalidHandle, operation))?;
        let entry = self
            .entries
            .get(slot)
            .ok_or_else(|| HostError::new(HostErrorCode::InvalidHandle, operation))?;
        if generation != entry.generation || entry.value.is_none() {
            return Err(HostError::new(HostErrorCode::StaleHandle, operation));
        }
        if encoded_kind != entry.kind || expected_kind != entry.kind {
            return Err(HostError::new(HostErrorCode::WrongKind, operation));
        }
        if owner != entry.owner {
            return Err(HostError::new(HostErrorCode::WrongOwner, operation));
        }
        Ok((slot, entry))
    }
}

fn encode(slot: usize, generation: u32, kind: HandleKind) -> HostOpaqueHandle {
    let slot_word = slot as u64 + 1;
    HostOpaqueHandle(
        ((generation as u64) << (SLOT_BITS + KIND_BITS)) | (slot_word << KIND_BITS) | kind as u64,
    )
}

fn decode(handle: HostOpaqueHandle) -> Option<(usize, u32, HandleKind)> {
    let kind = match (handle.0 & ((1 << KIND_BITS) - 1)) as u8 {
        1 => HandleKind::Scene,
        2 => HandleKind::World,
        3 => HandleKind::Parameter,
        4 => HandleKind::Session,
        5 => HandleKind::Report,
        _ => return None,
    };
    let slot_word = (handle.0 >> KIND_BITS) & SLOT_MASK;
    let generation = (handle.0 >> (SLOT_BITS + KIND_BITS)) as u32;
    if slot_word == 0 || generation == 0 {
        return None;
    }
    Some((slot_word as usize - 1, generation, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_kind_generation_and_disposal_are_fail_closed() {
        let owner = OwnerId::new(11).unwrap();
        let foreign = OwnerId::new(12).unwrap();
        let mut registry = HandleRegistry::default();
        let scene = registry.insert(owner, HandleKind::Scene, "scene").unwrap();

        assert_eq!(
            registry.get(scene, owner, HandleKind::Scene).unwrap(),
            &"scene"
        );
        assert_eq!(
            registry
                .get(scene, foreign, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::WrongOwner
        );
        assert_eq!(
            registry
                .get(scene, owner, HandleKind::World)
                .unwrap_err()
                .code(),
            HostErrorCode::WrongKind
        );
        assert_eq!(
            registry.remove(scene, owner, HandleKind::Scene).unwrap(),
            "scene"
        );
        assert_eq!(
            registry
                .remove(scene, owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::StaleHandle
        );

        let replacement = registry
            .insert(foreign, HandleKind::World, "replacement")
            .unwrap();
        assert_ne!(scene, replacement);
        assert_eq!(
            registry
                .get(scene, owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::StaleHandle
        );
        assert_eq!(
            registry
                .get(replacement, owner, HandleKind::World)
                .unwrap_err()
                .code(),
            HostErrorCode::WrongOwner
        );
        assert_eq!(
            registry
                .get(replacement, foreign, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::WrongKind
        );
        assert_eq!(registry.live_count(), 1);
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let owner = OwnerId::new(1).unwrap();
        let registry = HandleRegistry::<()>::default();
        assert_eq!(
            registry
                .get(HostOpaqueHandle(0), owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidHandle
        );
    }
}
