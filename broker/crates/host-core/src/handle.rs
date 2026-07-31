use crate::boundary::HostOpaqueHandle;
use crate::error::{HostError, HostErrorCode};
use std::sync::atomic::{AtomicU32, Ordering};

// Internal token layout: registry | generation | one-based slot | kind.
// Registry IDs and generations never wrap; exhaustion fails closed instead.
const KIND_BITS: u32 = 4;
const SLOT_BITS: u32 = 20;
const GENERATION_BITS: u32 = 20;
const REGISTRY_BITS: u32 = 20;
const KIND_MASK: u64 = (1_u64 << KIND_BITS) - 1;
const SLOT_MASK: u64 = (1_u64 << SLOT_BITS) - 1;
const GENERATION_MASK: u64 = (1_u64 << GENERATION_BITS) - 1;
const REGISTRY_MASK: u64 = (1_u64 << REGISTRY_BITS) - 1;
const SLOT_SHIFT: u32 = KIND_BITS;
const GENERATION_SHIFT: u32 = SLOT_SHIFT + SLOT_BITS;
const REGISTRY_SHIFT: u32 = GENERATION_SHIFT + GENERATION_BITS;
const MAX_SLOTS: usize = SLOT_MASK as usize;
static NEXT_REGISTRY_ID: AtomicU32 = AtomicU32::new(1);

const _: () = assert!(KIND_BITS + SLOT_BITS + GENERATION_BITS + REGISTRY_BITS == 64);

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
    registry_id: u32,
    entries: Vec<Entry<T>>,
    live: usize,
}

impl<T> Default for HandleRegistry<T> {
    fn default() -> Self {
        Self {
            registry_id: allocate_registry_id(),
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
        if self.registry_id == 0 {
            return Err(HostError::new(
                HostErrorCode::CapacityExceeded,
                "allocate_handle_registry",
            ));
        }
        if let Some((slot, entry)) =
            self.entries.iter_mut().enumerate().find(|(_, entry)| {
                entry.value.is_none() && entry.generation < GENERATION_MASK as u32
            })
        {
            entry.generation += 1;
            entry.owner = owner;
            entry.kind = kind;
            entry.value = Some(value);
            self.live += 1;
            return Ok(encode(self.registry_id, slot, entry.generation, kind));
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
        Ok(encode(self.registry_id, slot, 1, kind))
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

    pub fn get_mut(
        &mut self,
        handle: HostOpaqueHandle,
        owner: OwnerId,
        expected_kind: HandleKind,
    ) -> Result<&mut T, HostError> {
        let slot = {
            let (slot, _) = self.resolve(handle, owner, expected_kind, "resolve_handle_mut")?;
            slot
        };
        self.entries[slot]
            .value
            .as_mut()
            .ok_or_else(|| HostError::new(HostErrorCode::StaleHandle, "resolve_handle_mut"))
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
        let (registry_id, slot, generation, encoded_kind) = decode(handle)
            .ok_or_else(|| HostError::new(HostErrorCode::InvalidHandle, operation))?;
        if registry_id != self.registry_id {
            return Err(HostError::new(HostErrorCode::InvalidHandle, operation));
        }
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

fn allocate_registry_id() -> u32 {
    // Relaxed ordering is sufficient: atomic uniqueness, rather than memory
    // publication, is the invariant. Zero permanently denotes exhaustion.
    NEXT_REGISTRY_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            (next <= REGISTRY_MASK as u32).then_some(next + 1)
        })
        .unwrap_or(0)
}

fn encode(registry_id: u32, slot: usize, generation: u32, kind: HandleKind) -> HostOpaqueHandle {
    let slot_word = slot as u64 + 1;
    HostOpaqueHandle(
        ((registry_id as u64) << REGISTRY_SHIFT)
            | ((generation as u64) << GENERATION_SHIFT)
            | (slot_word << SLOT_SHIFT)
            | kind as u64,
    )
}

fn decode(handle: HostOpaqueHandle) -> Option<(u32, usize, u32, HandleKind)> {
    let kind = match (handle.0 & KIND_MASK) as u8 {
        1 => HandleKind::Scene,
        2 => HandleKind::World,
        3 => HandleKind::Parameter,
        4 => HandleKind::Session,
        5 => HandleKind::Report,
        _ => return None,
    };
    let slot_word = (handle.0 >> SLOT_SHIFT) & SLOT_MASK;
    let generation = ((handle.0 >> GENERATION_SHIFT) & GENERATION_MASK) as u32;
    let registry_id = ((handle.0 >> REGISTRY_SHIFT) & REGISTRY_MASK) as u32;
    if registry_id == 0 || slot_word == 0 || generation == 0 {
        return None;
    }
    Some((registry_id, slot_word as usize - 1, generation, kind))
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

    #[test]
    fn tokens_are_bound_to_the_registry_that_created_them() {
        let owner = OwnerId::new(21).unwrap();
        let mut first = HandleRegistry::default();
        let first_handle = first.insert(owner, HandleKind::Scene, "first").unwrap();
        let mut second = HandleRegistry::default();
        let second_handle = second.insert(owner, HandleKind::Scene, "second").unwrap();

        assert_ne!(first_handle, second_handle);
        assert_eq!(
            second
                .get(first_handle, owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidHandle
        );
        assert_eq!(
            first
                .get(second_handle, owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidHandle
        );

        drop(first);
        let mut replacement = HandleRegistry::default();
        let replacement_handle = replacement
            .insert(owner, HandleKind::Scene, "replacement")
            .unwrap();
        assert_ne!(first_handle, replacement_handle);
        assert_eq!(
            replacement
                .get(first_handle, owner, HandleKind::Scene)
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidHandle
        );
    }

    #[test]
    fn exhausted_slots_are_retired_without_blocking_remaining_capacity() {
        let owner = OwnerId::new(31).unwrap();
        let mut registry = HandleRegistry::default();
        let first = registry.insert(owner, HandleKind::Scene, "first").unwrap();
        let second = registry.insert(owner, HandleKind::Scene, "second").unwrap();
        registry.remove(first, owner, HandleKind::Scene).unwrap();
        registry.remove(second, owner, HandleKind::Scene).unwrap();
        registry.entries[0].generation = GENERATION_MASK as u32;

        let reused = registry.insert(owner, HandleKind::Scene, "reused").unwrap();
        let (_, reused_slot, reused_generation, _) = decode(reused).unwrap();
        assert_eq!(reused_slot, 1);
        assert_eq!(reused_generation, 2);

        let mut fresh_capacity = HandleRegistry::default();
        let exhausted = fresh_capacity
            .insert(owner, HandleKind::Scene, "exhausted")
            .unwrap();
        fresh_capacity
            .remove(exhausted, owner, HandleKind::Scene)
            .unwrap();
        fresh_capacity.entries[0].generation = GENERATION_MASK as u32;

        let newly_allocated = fresh_capacity
            .insert(owner, HandleKind::Scene, "new-slot")
            .unwrap();
        let (_, new_slot, new_generation, _) = decode(newly_allocated).unwrap();
        assert_eq!(new_slot, 1);
        assert_eq!(new_generation, 1);
    }
}
