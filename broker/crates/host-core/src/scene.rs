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
}
