//! Pointer-free scene identity shared with the native #571 scene registry.
//!
//! The C ABI keeps `kind` as an integer so an untrusted caller cannot create
//! an invalid Rust enum discriminant. Registry ownership and mutation remain
//! in C++; this module only validates and compares identity values.

use crate::error::{HostError, HostErrorCode};
use std::mem::{align_of, offset_of, size_of};

pub const HOST_SCENE_IDENTITY_ABI_VERSION: u32 = 1;
pub const HOST_SCENE_IDENTITY_ABI_DESCRIPTOR_MAGIC: u64 = 0x4145_5853_4349_4431;
pub const HOST_SCENE_IDENTITY_CAPABILITY_MATCH_V1: u64 = 1;
pub const HOST_SCENE_OWNER_RELATION_ABI_VERSION: u32 = 1;
pub const HOST_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_MAGIC: u64 = 0x4145_584f_574e_5231;
pub const HOST_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1: u64 = 1;
pub const HOST_SCENE_TOPOLOGY_ABI_VERSION: u32 = 1;
pub const HOST_SCENE_TOPOLOGY_ABI_DESCRIPTOR_MAGIC: u64 = 0x4145_5854_4f50_4f31;
pub const HOST_SCENE_TOPOLOGY_CAPABILITY_SUMMARY_V1: u64 = 1;
pub const HOST_SCENE_TOPOLOGY_CAPACITY: usize = 16;

/// Numeric values frozen by `scene_model::ObjectKind` in Issue #26 / PR #571.
pub mod object_kind {
    pub const NONE: u8 = 0;
    pub const PROJECT: u8 = 1;
    pub const ITEM: u8 = 2;
    pub const COMPOSITION: u8 = 3;
    pub const FOLDER: u8 = 4;
    pub const FOOTAGE: u8 = 5;
    pub const LAYER: u8 = 6;
    pub const EFFECT: u8 = 7;
    pub const STREAM: u8 = 8;
    pub const KEYFRAME: u8 = 9;
    pub const VALUE: u8 = 10;
}

/// Pointer-free identity copied from the current C++ scene owner.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneIdentity {
    pub project_id: u64,
    pub object_id: u64,
    pub generation: u32,
    pub kind: u8,
    pub reserved: [u8; 3],
}

impl HostSceneIdentity {
    pub const fn new(project_id: u64, object_id: u64, generation: u32, kind: u8) -> Self {
        Self {
            project_id,
            object_id,
            generation,
            kind,
            reserved: [0; 3],
        }
    }

    pub fn validate(&self) -> Result<(), HostError> {
        if self.project_id == 0
            || self.object_id == 0
            || self.generation == 0
            || self.reserved != [0; 3]
        {
            return Err(HostError::new(
                HostErrorCode::InvalidArgument,
                "validate_scene_identity",
            ));
        }
        if !matches!(
            self.kind,
            object_kind::PROJECT
                | object_kind::ITEM
                | object_kind::COMPOSITION
                | object_kind::FOLDER
                | object_kind::FOOTAGE
                | object_kind::LAYER
                | object_kind::EFFECT
                | object_kind::STREAM
                | object_kind::KEYFRAME
                | object_kind::VALUE
        ) {
            return Err(HostError::new(
                HostErrorCode::WrongKind,
                "validate_scene_identity_kind",
            ));
        }
        Ok(())
    }

    pub const fn is_zero_sentinel(&self) -> bool {
        self.project_id == 0
            && self.object_id == 0
            && self.generation == 0
            && self.kind == object_kind::NONE
            && self.reserved[0] == 0
            && self.reserved[1] == 0
            && self.reserved[2] == 0
    }

    /// Matches a caller-held candidate against the current registry identity.
    ///
    /// This deliberately does not decide whether either value is live. The
    /// C++ registry remains the owner and supplies the current value.
    pub fn match_candidate(&self, candidate: &Self) -> Result<(), HostError> {
        self.validate()?;
        candidate.validate()?;
        if self.project_id != candidate.project_id {
            return Err(HostError::new(
                HostErrorCode::WrongOwner,
                "match_scene_identity_project",
            ));
        }
        if self.kind != candidate.kind {
            return Err(HostError::new(
                HostErrorCode::WrongKind,
                "match_scene_identity_kind",
            ));
        }
        if self.object_id != candidate.object_id {
            return Err(HostError::new(
                HostErrorCode::InvalidHandle,
                "match_scene_identity_object",
            ));
        }
        if self.generation != candidate.generation {
            return Err(HostError::new(
                HostErrorCode::StaleHandle,
                "match_scene_identity_generation",
            ));
        }
        Ok(())
    }
}

/// Pointer-free projection of the single ownership edge in a C++ scene
/// snapshot. The C++ registry remains authoritative for both identities.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneOwnerRelation {
    pub object: HostSceneIdentity,
    pub owner: HostSceneIdentity,
}

impl HostSceneOwnerRelation {
    pub const fn new(object: HostSceneIdentity, owner: HostSceneIdentity) -> Self {
        Self { object, owner }
    }

    pub fn validate(&self) -> Result<(), HostError> {
        self.object.validate()?;
        if self.object.kind == object_kind::PROJECT {
            if self.owner.is_zero_sentinel() {
                return Ok(());
            }
            self.owner.validate()?;
            return Err(HostError::new(
                HostErrorCode::WrongOwner,
                "validate_scene_project_owner",
            ));
        }

        self.owner.validate()?;
        if self.object.project_id != self.owner.project_id {
            return Err(HostError::new(
                HostErrorCode::WrongOwner,
                "validate_scene_owner_project",
            ));
        }
        if self.object.object_id == self.owner.object_id && self.object.kind == self.owner.kind {
            return Err(HostError::new(
                HostErrorCode::WrongOwner,
                "validate_scene_self_owner",
            ));
        }
        Ok(())
    }

    /// Matches one caller-held ownership edge against the current C++ snapshot.
    pub fn match_candidate(&self, candidate: &Self) -> Result<(), HostError> {
        self.validate()?;
        candidate.validate()?;
        self.object.match_candidate(&candidate.object)?;
        if self.object.kind == object_kind::PROJECT {
            return Ok(());
        }
        match self.owner.match_candidate(&candidate.owner) {
            Err(error) if error.code() == HostErrorCode::InvalidHandle => Err(HostError::new(
                HostErrorCode::WrongOwner,
                "match_scene_owner_object",
            )),
            result => result,
        }
    }
}

/// One pointer-free entry in a bounded topology snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneTopologyEntry {
    pub relation: HostSceneOwnerRelation,
    pub local_index: i32,
    pub reserved: u32,
}

impl HostSceneTopologyEntry {
    pub const fn new(relation: HostSceneOwnerRelation, local_index: i32) -> Self {
        Self {
            relation,
            local_index,
            reserved: 0,
        }
    }

    pub const fn zeroed() -> Self {
        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        Self::new(HostSceneOwnerRelation::new(zero, zero), 0)
    }

    pub const fn is_zeroed(&self) -> bool {
        self.relation.object.is_zero_sentinel()
            && self.relation.owner.is_zero_sentinel()
            && self.local_index == 0
            && self.reserved == 0
    }
}

/// Fixed-capacity snapshot copied from the current C++ scene registry.
///
/// `entry_count` may be greater than the fixed capacity only to report an
/// explicit capacity error. In that case no entry is inspected or truncated.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneTopologySnapshot {
    pub project_id: u64,
    pub entry_count: u32,
    pub reserved: u32,
    pub entries: [HostSceneTopologyEntry; HOST_SCENE_TOPOLOGY_CAPACITY],
}

impl HostSceneTopologySnapshot {
    pub const fn empty(project_id: u64) -> Self {
        Self {
            project_id,
            entry_count: 0,
            reserved: 0,
            entries: [HostSceneTopologyEntry::zeroed(); HOST_SCENE_TOPOLOGY_CAPACITY],
        }
    }

    pub fn calculate_summary(&self) -> Result<HostSceneTopologySummary, HostError> {
        if self.project_id == 0 || self.reserved != 0 || self.entry_count == 0 {
            return Err(HostError::new(
                HostErrorCode::InvalidArgument,
                "validate_scene_topology_header",
            ));
        }
        let entry_count = self.entry_count as usize;
        if entry_count > HOST_SCENE_TOPOLOGY_CAPACITY {
            return Err(HostError::new(
                HostErrorCode::CapacityExceeded,
                "validate_scene_topology_capacity",
            ));
        }
        if self.entries[entry_count..]
            .iter()
            .any(|entry| !entry.is_zeroed())
        {
            return Err(HostError::new(
                HostErrorCode::InvalidArgument,
                "validate_scene_topology_unused_entry",
            ));
        }

        let entries = &self.entries[..entry_count];
        let mut root_count = 0_u32;
        for entry in entries {
            if entry.reserved != 0 {
                return Err(HostError::new(
                    HostErrorCode::InvalidArgument,
                    "validate_scene_topology_entry_reserved",
                ));
            }
            entry.relation.validate()?;
            if entry.relation.object.project_id != self.project_id {
                return Err(HostError::new(
                    HostErrorCode::WrongOwner,
                    "validate_scene_topology_project",
                ));
            }
            if entry.relation.object.kind == object_kind::PROJECT {
                if entry.local_index != -1 {
                    return Err(HostError::new(
                        HostErrorCode::InvalidArgument,
                        "validate_scene_topology_root_index",
                    ));
                }
                root_count += 1;
            } else if entry.local_index < 0 {
                return Err(HostError::new(
                    HostErrorCode::InvalidArgument,
                    "validate_scene_topology_local_index",
                ));
            }
        }
        if root_count != 1 {
            return Err(HostError::new(
                HostErrorCode::InvalidState,
                "validate_scene_topology_root_count",
            ));
        }

        for (index, entry) in entries.iter().enumerate() {
            let object = &entry.relation.object;
            if entries[..index].iter().any(|candidate| {
                candidate.relation.object.project_id == object.project_id
                    && candidate.relation.object.object_id == object.object_id
            }) {
                return Err(HostError::new(
                    HostErrorCode::InvalidArgument,
                    "validate_scene_topology_duplicate_identity",
                ));
            }
            if object.kind == object_kind::PROJECT {
                continue;
            }
            let owner = &entry.relation.owner;
            if !entries
                .iter()
                .any(|candidate| candidate.relation.object == *owner)
            {
                return Err(HostError::new(
                    HostErrorCode::WrongOwner,
                    "validate_scene_topology_missing_owner",
                ));
            }
            if entries[..index].iter().any(|candidate| {
                candidate.relation.owner == *owner
                    && candidate.relation.object.kind == object.kind
                    && candidate.local_index == entry.local_index
            }) {
                return Err(HostError::new(
                    HostErrorCode::InvalidArgument,
                    "validate_scene_topology_local_index_collision",
                ));
            }
        }

        for entry in entries {
            if entry.relation.object.kind == object_kind::PROJECT {
                continue;
            }
            let start = entry.relation.object;
            let mut owner = entry.relation.owner;
            let mut hops = 0_usize;
            while owner.kind != object_kind::PROJECT {
                if owner.project_id == start.project_id && owner.object_id == start.object_id {
                    return Err(HostError::new(
                        HostErrorCode::WrongOwner,
                        "validate_scene_topology_cycle",
                    ));
                }
                let Some(owner_entry) = entries
                    .iter()
                    .find(|candidate| candidate.relation.object == owner)
                else {
                    return Err(HostError::new(
                        HostErrorCode::WrongOwner,
                        "validate_scene_topology_missing_owner_chain",
                    ));
                };
                owner = owner_entry.relation.owner;
                hops += 1;
                if hops >= entry_count {
                    return Err(HostError::new(
                        HostErrorCode::WrongOwner,
                        "validate_scene_topology_cycle_depth",
                    ));
                }
            }
        }

        let mut order = [0_usize; HOST_SCENE_TOPOLOGY_CAPACITY];
        for (index, slot) in order[..entry_count].iter_mut().enumerate() {
            *slot = index;
        }
        order[..entry_count].sort_unstable_by_key(|index| {
            let object = self.entries[*index].relation.object;
            (
                object.project_id,
                object.object_id,
                object.generation,
                object.kind,
            )
        });

        let edge_count = self.entry_count - root_count;
        let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
        fingerprint = mix_topology_u64(fingerprint, self.project_id);
        fingerprint = mix_topology_u64(fingerprint, self.entry_count as u64);
        fingerprint = mix_topology_u64(fingerprint, edge_count as u64);
        for index in &order[..entry_count] {
            let entry = &self.entries[*index];
            fingerprint = mix_topology_identity(fingerprint, &entry.relation.object);
            fingerprint = mix_topology_identity(fingerprint, &entry.relation.owner);
            fingerprint = mix_topology_u64(fingerprint, entry.local_index as u32 as u64);
        }

        Ok(HostSceneTopologySummary {
            project_id: self.project_id,
            fingerprint,
            object_count: self.entry_count,
            edge_count,
            root_count,
            reserved: 0,
        })
    }
}

fn mix_topology_u64(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn mix_topology_identity(mut hash: u64, identity: &HostSceneIdentity) -> u64 {
    hash = mix_topology_u64(hash, identity.project_id);
    hash = mix_topology_u64(hash, identity.object_id);
    hash = mix_topology_u64(hash, u64::from(identity.generation));
    mix_topology_u64(hash, u64::from(identity.kind))
}

/// Canonical Rust-owned calculation result for one topology snapshot.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneTopologySummary {
    pub project_id: u64,
    pub fingerprint: u64,
    pub object_count: u32,
    pub edge_count: u32,
    pub root_count: u32,
    pub reserved: u32,
}

impl HostSceneTopologySummary {
    pub const fn zeroed() -> Self {
        Self {
            project_id: 0,
            fingerprint: 0,
            object_count: 0,
            edge_count: 0,
            root_count: 0,
            reserved: 0,
        }
    }
}

/// Dedicated pre-cast descriptor for the one identity value and matcher.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneIdentityAbiDescriptorV1 {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub identity_size: u32,
    pub identity_alignment: u32,
    pub capabilities: u64,
}

impl HostSceneIdentityAbiDescriptorV1 {
    pub const fn current() -> Self {
        Self {
            magic: HOST_SCENE_IDENTITY_ABI_DESCRIPTOR_MAGIC,
            abi_version: HOST_SCENE_IDENTITY_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            identity_size: size_of::<HostSceneIdentity>() as u32,
            identity_alignment: align_of::<HostSceneIdentity>() as u32,
            capabilities: HOST_SCENE_IDENTITY_CAPABILITY_MATCH_V1,
        }
    }
}

/// Dedicated pre-cast descriptor for the owner-edge value and matcher.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneOwnerRelationAbiDescriptorV1 {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub relation_size: u32,
    pub relation_alignment: u32,
    pub capabilities: u64,
}

impl HostSceneOwnerRelationAbiDescriptorV1 {
    pub const fn current() -> Self {
        Self {
            magic: HOST_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_MAGIC,
            abi_version: HOST_SCENE_OWNER_RELATION_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            relation_size: size_of::<HostSceneOwnerRelation>() as u32,
            relation_alignment: align_of::<HostSceneOwnerRelation>() as u32,
            capabilities: HOST_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1,
        }
    }
}

/// Dedicated pre-cast descriptor for the bounded topology calculation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostSceneTopologyAbiDescriptorV1 {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub entry_size: u32,
    pub entry_alignment: u32,
    pub snapshot_size: u32,
    pub snapshot_alignment: u32,
    pub summary_size: u32,
    pub summary_alignment: u32,
    pub capacity: u32,
    pub reserved: u32,
    pub capabilities: u64,
}

impl HostSceneTopologyAbiDescriptorV1 {
    pub const fn current() -> Self {
        Self {
            magic: HOST_SCENE_TOPOLOGY_ABI_DESCRIPTOR_MAGIC,
            abi_version: HOST_SCENE_TOPOLOGY_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            entry_size: size_of::<HostSceneTopologyEntry>() as u32,
            entry_alignment: align_of::<HostSceneTopologyEntry>() as u32,
            snapshot_size: size_of::<HostSceneTopologySnapshot>() as u32,
            snapshot_alignment: align_of::<HostSceneTopologySnapshot>() as u32,
            summary_size: size_of::<HostSceneTopologySummary>() as u32,
            summary_alignment: align_of::<HostSceneTopologySummary>() as u32,
            capacity: HOST_SCENE_TOPOLOGY_CAPACITY as u32,
            reserved: 0,
            capabilities: HOST_SCENE_TOPOLOGY_CAPABILITY_SUMMARY_V1,
        }
    }
}

const _: () = {
    assert!(size_of::<HostSceneIdentity>() == 24);
    assert!(align_of::<HostSceneIdentity>() == 8);
    assert!(offset_of!(HostSceneIdentity, project_id) == 0);
    assert!(offset_of!(HostSceneIdentity, object_id) == 8);
    assert!(offset_of!(HostSceneIdentity, generation) == 16);
    assert!(offset_of!(HostSceneIdentity, kind) == 20);
    assert!(offset_of!(HostSceneIdentity, reserved) == 21);

    assert!(size_of::<HostSceneIdentityAbiDescriptorV1>() == 32);
    assert!(align_of::<HostSceneIdentityAbiDescriptorV1>() == 8);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, magic) == 0);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, abi_version) == 8);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, struct_size) == 12);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, identity_size) == 16);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, identity_alignment) == 20);
    assert!(offset_of!(HostSceneIdentityAbiDescriptorV1, capabilities) == 24);

    assert!(size_of::<HostSceneOwnerRelation>() == 48);
    assert!(align_of::<HostSceneOwnerRelation>() == 8);
    assert!(offset_of!(HostSceneOwnerRelation, object) == 0);
    assert!(offset_of!(HostSceneOwnerRelation, owner) == 24);

    assert!(size_of::<HostSceneOwnerRelationAbiDescriptorV1>() == 32);
    assert!(align_of::<HostSceneOwnerRelationAbiDescriptorV1>() == 8);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, magic) == 0);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, abi_version) == 8);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, struct_size) == 12);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, relation_size) == 16);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, relation_alignment) == 20);
    assert!(offset_of!(HostSceneOwnerRelationAbiDescriptorV1, capabilities) == 24);

    assert!(size_of::<HostSceneTopologyEntry>() == 56);
    assert!(align_of::<HostSceneTopologyEntry>() == 8);
    assert!(offset_of!(HostSceneTopologyEntry, relation) == 0);
    assert!(offset_of!(HostSceneTopologyEntry, local_index) == 48);
    assert!(offset_of!(HostSceneTopologyEntry, reserved) == 52);

    assert!(size_of::<HostSceneTopologySnapshot>() == 912);
    assert!(align_of::<HostSceneTopologySnapshot>() == 8);
    assert!(offset_of!(HostSceneTopologySnapshot, project_id) == 0);
    assert!(offset_of!(HostSceneTopologySnapshot, entry_count) == 8);
    assert!(offset_of!(HostSceneTopologySnapshot, reserved) == 12);
    assert!(offset_of!(HostSceneTopologySnapshot, entries) == 16);

    assert!(size_of::<HostSceneTopologySummary>() == 32);
    assert!(align_of::<HostSceneTopologySummary>() == 8);
    assert!(offset_of!(HostSceneTopologySummary, project_id) == 0);
    assert!(offset_of!(HostSceneTopologySummary, fingerprint) == 8);
    assert!(offset_of!(HostSceneTopologySummary, object_count) == 16);
    assert!(offset_of!(HostSceneTopologySummary, edge_count) == 20);
    assert!(offset_of!(HostSceneTopologySummary, root_count) == 24);
    assert!(offset_of!(HostSceneTopologySummary, reserved) == 28);

    assert!(size_of::<HostSceneTopologyAbiDescriptorV1>() == 56);
    assert!(align_of::<HostSceneTopologyAbiDescriptorV1>() == 8);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, magic) == 0);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, abi_version) == 8);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, struct_size) == 12);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, entry_size) == 16);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, entry_alignment) == 20);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, snapshot_size) == 24);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, snapshot_alignment) == 28);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, summary_size) == 32);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, summary_alignment) == 36);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, capacity) == 40);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, reserved) == 44);
    assert!(offset_of!(HostSceneTopologyAbiDescriptorV1, capabilities) == 48);

    assert!(object_kind::PROJECT == 1);
    assert!(object_kind::ITEM == 2);
    assert!(object_kind::COMPOSITION == 3);
    assert!(object_kind::FOLDER == 4);
    assert!(object_kind::FOOTAGE == 5);
    assert!(object_kind::LAYER == 6);
    assert!(object_kind::EFFECT == 7);
    assert!(object_kind::STREAM == 8);
    assert!(object_kind::KEYFRAME == 9);
    assert!(object_kind::VALUE == 10);
    assert!(object_kind::NONE == 0);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> HostSceneIdentity {
        HostSceneIdentity::new(7, 101, 3, object_kind::LAYER)
    }

    #[test]
    fn layout_and_descriptor_match_the_cpp_identity_contract() {
        assert_eq!(size_of::<HostSceneIdentity>(), 24);
        assert_eq!(align_of::<HostSceneIdentity>(), 8);
        assert_eq!(offset_of!(HostSceneIdentity, generation), 16);
        assert_eq!(offset_of!(HostSceneIdentity, kind), 20);
        assert_eq!(offset_of!(HostSceneIdentity, reserved), 21);

        let descriptor = HostSceneIdentityAbiDescriptorV1::current();
        assert_eq!(size_of::<HostSceneIdentityAbiDescriptorV1>(), 32);
        assert_eq!(align_of::<HostSceneIdentityAbiDescriptorV1>(), 8);
        assert_eq!(
            offset_of!(HostSceneIdentityAbiDescriptorV1, capabilities),
            24
        );
        assert_eq!(descriptor.magic, HOST_SCENE_IDENTITY_ABI_DESCRIPTOR_MAGIC);
        assert_eq!(descriptor.abi_version, HOST_SCENE_IDENTITY_ABI_VERSION);
        assert_eq!(descriptor.struct_size, 32);
        assert_eq!(descriptor.identity_size, 24);
        assert_eq!(descriptor.identity_alignment, 8);
        assert_eq!(
            descriptor.capabilities,
            HOST_SCENE_IDENTITY_CAPABILITY_MATCH_V1
        );
    }

    #[test]
    fn matcher_classifies_identity_and_generation_failures() {
        let current = current();
        assert!(current.match_candidate(&current).is_ok());

        let mut candidate = current;
        candidate.project_id += 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongOwner
        );
        candidate = current;
        candidate.kind = object_kind::EFFECT;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongKind
        );
        candidate = current;
        candidate.object_id += 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::InvalidHandle
        );
        candidate = current;
        candidate.generation -= 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::StaleHandle
        );
    }

    #[test]
    fn malformed_values_fail_closed_before_comparison() {
        let current = current();
        for malformed in [
            HostSceneIdentity {
                project_id: 0,
                ..current
            },
            HostSceneIdentity {
                object_id: 0,
                ..current
            },
            HostSceneIdentity {
                generation: 0,
                ..current
            },
            HostSceneIdentity {
                reserved: [1, 0, 0],
                ..current
            },
        ] {
            assert_eq!(
                current.match_candidate(&malformed).unwrap_err().code(),
                HostErrorCode::InvalidArgument
            );
        }
        let unknown_kind = HostSceneIdentity {
            kind: u8::MAX,
            ..current
        };
        assert_eq!(
            current.match_candidate(&unknown_kind).unwrap_err().code(),
            HostErrorCode::WrongKind
        );
        let none_kind = HostSceneIdentity {
            kind: object_kind::NONE,
            ..current
        };
        assert_eq!(
            current.match_candidate(&none_kind).unwrap_err().code(),
            HostErrorCode::WrongKind
        );
    }

    #[test]
    fn owner_relation_layout_and_descriptor_are_stable() {
        assert_eq!(size_of::<HostSceneOwnerRelation>(), 48);
        assert_eq!(align_of::<HostSceneOwnerRelation>(), 8);
        assert_eq!(offset_of!(HostSceneOwnerRelation, object), 0);
        assert_eq!(offset_of!(HostSceneOwnerRelation, owner), 24);

        let descriptor = HostSceneOwnerRelationAbiDescriptorV1::current();
        assert_eq!(
            descriptor.magic,
            HOST_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_MAGIC
        );
        assert_eq!(
            descriptor.abi_version,
            HOST_SCENE_OWNER_RELATION_ABI_VERSION
        );
        assert_eq!(descriptor.struct_size, 32);
        assert_eq!(descriptor.relation_size, 48);
        assert_eq!(descriptor.relation_alignment, 8);
        assert_eq!(
            descriptor.capabilities,
            HOST_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1
        );
    }

    #[test]
    fn owner_relation_matches_root_and_child_edges() {
        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        let project = HostSceneIdentity::new(7, 7, 1, object_kind::PROJECT);
        let root = HostSceneOwnerRelation::new(project, zero);
        assert!(root.match_candidate(&root).is_ok());

        let composition = HostSceneIdentity::new(7, 301, 4, object_kind::COMPOSITION);
        let layer = HostSceneIdentity::new(7, 401, 2, object_kind::LAYER);
        let current = HostSceneOwnerRelation::new(layer, composition);
        assert!(current.match_candidate(&current).is_ok());

        let mut candidate = current;
        candidate.owner.generation -= 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::StaleHandle
        );
        candidate = current;
        candidate.owner.object_id += 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongOwner
        );
        candidate = current;
        candidate.owner.project_id += 1;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongOwner
        );
        candidate = current;
        candidate.owner.kind = object_kind::LAYER;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongKind
        );
    }

    #[test]
    fn malformed_owner_relations_fail_closed() {
        let composition = HostSceneIdentity::new(7, 301, 4, object_kind::COMPOSITION);
        let layer = HostSceneIdentity::new(7, 401, 2, object_kind::LAYER);
        let current = HostSceneOwnerRelation::new(layer, composition);

        let mut candidate = current;
        candidate.owner = candidate.object;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongOwner
        );
        candidate = current;
        candidate.owner.kind = u8::MAX;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::WrongKind
        );
        candidate = current;
        candidate.owner.project_id = 0;
        assert_eq!(
            current.match_candidate(&candidate).unwrap_err().code(),
            HostErrorCode::InvalidArgument
        );

        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        let project = HostSceneIdentity::new(7, 7, 1, object_kind::PROJECT);
        let invalid_root = HostSceneOwnerRelation::new(project, composition);
        assert_eq!(
            HostSceneOwnerRelation::new(project, zero)
                .match_candidate(&invalid_root)
                .unwrap_err()
                .code(),
            HostErrorCode::WrongOwner
        );
    }

    fn topology_fixture() -> HostSceneTopologySnapshot {
        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        let project = HostSceneIdentity::new(7, 7, 1, object_kind::PROJECT);
        let folder = HostSceneIdentity::new(7, 100, 1, object_kind::FOLDER);
        let item = HostSceneIdentity::new(7, 1001, 1, object_kind::ITEM);
        let composition = HostSceneIdentity::new(7, 5001, 1, object_kind::COMPOSITION);
        let layer = HostSceneIdentity::new(7, 2001, 1, object_kind::LAYER);
        let mut snapshot = HostSceneTopologySnapshot::empty(7);
        snapshot.entry_count = 5;
        snapshot.entries[0] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(project, zero), -1);
        snapshot.entries[1] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(folder, project), 0);
        snapshot.entries[2] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(item, folder), 0);
        snapshot.entries[3] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(composition, item), 0);
        snapshot.entries[4] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(layer, composition), 0);
        snapshot
    }

    #[test]
    fn topology_layout_descriptor_and_summary_are_stable() {
        assert_eq!(size_of::<HostSceneTopologyEntry>(), 56);
        assert_eq!(size_of::<HostSceneTopologySnapshot>(), 912);
        assert_eq!(size_of::<HostSceneTopologySummary>(), 32);
        assert_eq!(size_of::<HostSceneTopologyAbiDescriptorV1>(), 56);
        let descriptor = HostSceneTopologyAbiDescriptorV1::current();
        assert_eq!(descriptor.magic, HOST_SCENE_TOPOLOGY_ABI_DESCRIPTOR_MAGIC);
        assert_eq!(descriptor.entry_size, 56);
        assert_eq!(descriptor.snapshot_size, 912);
        assert_eq!(descriptor.summary_size, 32);
        assert_eq!(descriptor.capacity, 16);
        assert_eq!(
            descriptor.capabilities,
            HOST_SCENE_TOPOLOGY_CAPABILITY_SUMMARY_V1
        );

        let summary = topology_fixture().calculate_summary().unwrap();
        assert_eq!(summary.project_id, 7);
        assert_eq!(summary.object_count, 5);
        assert_eq!(summary.edge_count, 4);
        assert_eq!(summary.root_count, 1);
        assert_ne!(summary.fingerprint, 0);
    }

    #[test]
    fn topology_summary_is_enumeration_order_independent_and_state_sensitive() {
        let snapshot = topology_fixture();
        let expected = snapshot.calculate_summary().unwrap();

        let mut reordered = HostSceneTopologySnapshot::empty(7);
        reordered.entry_count = snapshot.entry_count;
        reordered.entries[0] = snapshot.entries[4];
        reordered.entries[1] = snapshot.entries[2];
        reordered.entries[2] = snapshot.entries[0];
        reordered.entries[3] = snapshot.entries[3];
        reordered.entries[4] = snapshot.entries[1];
        assert_eq!(reordered.calculate_summary().unwrap(), expected);

        let mut generation_changed = snapshot;
        generation_changed.entries[3].relation.object.generation += 1;
        generation_changed.entries[4].relation.owner =
            generation_changed.entries[3].relation.object;
        assert_ne!(
            generation_changed.calculate_summary().unwrap().fingerprint,
            expected.fingerprint
        );

        let mut local_index_changed = snapshot;
        local_index_changed.entries[4].local_index = 3;
        assert_ne!(
            local_index_changed.calculate_summary().unwrap().fingerprint,
            expected.fingerprint
        );
    }

    #[test]
    fn topology_structural_failures_are_classified_without_truncation() {
        let snapshot = topology_fixture();

        let mut overflow = snapshot;
        overflow.entry_count = HOST_SCENE_TOPOLOGY_CAPACITY as u32 + 1;
        assert_eq!(
            overflow.calculate_summary().unwrap_err().code(),
            HostErrorCode::CapacityExceeded
        );

        let mut duplicate = snapshot;
        duplicate.entry_count = 6;
        duplicate.entries[5] = duplicate.entries[3];
        duplicate.entries[5].local_index = 1;
        assert_eq!(
            duplicate.calculate_summary().unwrap_err().code(),
            HostErrorCode::InvalidArgument
        );

        let mut missing_owner = snapshot;
        missing_owner.entries[4].relation.owner.object_id = 9999;
        assert_eq!(
            missing_owner.calculate_summary().unwrap_err().code(),
            HostErrorCode::WrongOwner
        );

        let mut cycle = snapshot;
        cycle.entries[1].relation.owner = cycle.entries[2].relation.object;
        cycle.entries[2].relation.owner = cycle.entries[1].relation.object;
        assert_eq!(
            cycle.calculate_summary().unwrap_err().code(),
            HostErrorCode::WrongOwner
        );

        let mut cross_project = snapshot;
        cross_project.entries[4].relation.owner.project_id = 8;
        assert_eq!(
            cross_project.calculate_summary().unwrap_err().code(),
            HostErrorCode::WrongOwner
        );

        let mut collision = snapshot;
        collision.entry_count = 6;
        collision.entries[5] = collision.entries[4];
        collision.entries[5].relation.object.object_id = 2002;
        assert_eq!(
            collision.calculate_summary().unwrap_err().code(),
            HostErrorCode::InvalidArgument
        );

        let mut hidden_unused = snapshot;
        hidden_unused.entries[15].local_index = 1;
        assert_eq!(
            hidden_unused.calculate_summary().unwrap_err().code(),
            HostErrorCode::InvalidArgument
        );
    }
}
