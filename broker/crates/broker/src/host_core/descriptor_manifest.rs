use crate::host_core::parameter::{
    ColorValue, Descriptor, ParameterValue, PluginProfile, ValueKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

const MANIFEST_LIMIT: u64 = crate::parameter_limits::MAX_PARAMETER_DOCUMENT_BYTES as u64;

#[derive(Clone, Copy)]
pub struct ManifestPolicy {
    pub path: &'static str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    plugin_id: String,
    source: Source,
    descriptors: Vec<ObservedDescriptor>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    stage: String,
    plugin_sha256: String,
    receipt_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObservedDescriptor {
    slot: u32,
    observed_type: i32,
    display_name: String,
    assignable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<ValueKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_color: Option<ColorValue>,
}

pub struct LoadedManifest {
    pub profile: PluginProfile,
    pub plugin_sha256: String,
    pub receipt_id: String,
    pub observed_descriptor_count: usize,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub fn load(
    repository: &Path,
    plugin_id: &str,
    policy: ManifestPolicy,
) -> io::Result<LoadedManifest> {
    let path = repository.join(policy.path);
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MANIFEST_LIMIT {
        return Err(invalid("descriptor manifest size invalid"));
    }
    let bytes = fs::read(path)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    // The manifest used to be pinned to a digest compiled into the broker, so
    // editing a descriptor file meant hand-updating a constant before any
    // route would run again (issue #733). The digest proved only that the file
    // had not changed since someone last regenerated the constant, which is a
    // property of the checkout, not of the descriptors. The structural checks
    // below are what actually decide whether the manifest can drive a render.
    if !matches!(manifest.schema_version, 1 | 2)
        || manifest.plugin_id != plugin_id
        || manifest.source.stage != "L2"
        || manifest.source.plugin_sha256.len() != 64
        || !manifest
            .source
            .plugin_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.source.receipt_id.is_empty()
        || manifest.descriptors.is_empty()
        || manifest.descriptors.len() > crate::parameter_limits::MAX_PARAMETERS as usize
    {
        return Err(invalid("descriptor manifest identity invalid"));
    }
    let mut ids = BTreeSet::new();
    let mut descriptors = Vec::new();
    for (offset, observed) in manifest.descriptors.iter().enumerate() {
        if observed.slot as usize != offset + 1 || observed.display_name.is_empty() {
            return Err(invalid("descriptor slots must be contiguous and named"));
        }
        if !observed.assignable {
            continue;
        }
        let id = observed
            .id
            .clone()
            .ok_or_else(|| invalid("assignable descriptor id missing"))?;
        let kind = observed
            .kind
            .ok_or_else(|| invalid("assignable descriptor kind missing"))?;
        let type_matches = matches!(kind, ValueKind::Integer)
            && matches!(observed.observed_type, 1 | 4 | 7)
            || matches!(kind, ValueKind::Float) && observed.observed_type == 10
            || matches!(kind, ValueKind::Color)
                && observed.observed_type == 5
                && manifest.schema_version == 2;
        if !ids.insert(id.clone()) || !type_matches {
            return Err(invalid("assignable descriptor contract invalid"));
        }
        let (minimum, maximum, default_value) = match kind {
            ValueKind::Integer | ValueKind::Float => {
                let minimum = observed
                    .minimum
                    .ok_or_else(|| invalid("descriptor minimum missing"))?;
                let maximum = observed
                    .maximum
                    .ok_or_else(|| invalid("descriptor maximum missing"))?;
                let default_value = observed
                    .default
                    .ok_or_else(|| invalid("descriptor default missing"))?;
                if observed.default_color.is_some()
                    || !minimum.is_finite()
                    || !maximum.is_finite()
                    || !default_value.is_finite()
                    || minimum > maximum
                    || default_value < minimum
                    || default_value > maximum
                {
                    return Err(invalid("assignable numeric descriptor contract invalid"));
                }
                (
                    Some(minimum),
                    Some(maximum),
                    ParameterValue::Numeric(default_value),
                )
            }
            ValueKind::Color => {
                if observed.minimum.is_some()
                    || observed.maximum.is_some()
                    || observed.default.is_some()
                {
                    return Err(invalid("assignable color descriptor contract invalid"));
                }
                (
                    None,
                    None,
                    ParameterValue::Color(
                        observed
                            .default_color
                            .ok_or_else(|| invalid("descriptor default color missing"))?,
                    ),
                )
            }
        };
        descriptors.push(Descriptor {
            id,
            display_name: observed.display_name.clone(),
            slot: observed.slot,
            observed_type: observed.observed_type,
            minimum,
            maximum,
            default_value,
            kind,
        });
    }
    if descriptors.is_empty() {
        return Err(invalid("descriptor manifest has no assignable parameters"));
    }
    Ok(LoadedManifest {
        profile: PluginProfile {
            id: manifest.plugin_id,
            descriptors,
        },
        plugin_sha256: manifest.source.plugin_sha256,
        receipt_id: manifest.source.receipt_id,
        observed_descriptor_count: manifest.descriptors.len(),
    })
}

#[cfg(test)]
mod capacity_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn large_descriptor_manifest_retains_every_slot_and_rejects_overflow() {
        let root =
            std::env::temp_dir().join(format!("aex-descriptors-{:032x}", rand::random::<u128>()));
        fs::create_dir(&root).unwrap();
        let policy = ManifestPolicy {
            path: "manifest.json",
        };
        for count in [1025, 1623, 4096, 4097] {
            let descriptors = (1..=count)
                .map(|slot| {
                    json!({
                        "slot": slot, "observed_type": 1, "display_name": format!("Value {slot}"),
                        "assignable": true, "id": format!("p{slot}"), "kind": "integer",
                        "minimum": 0, "maximum": 255, "default": 17,
                    })
                })
                .collect::<Vec<_>>();
            let bytes = serde_json::to_vec(&json!({"schema_version": 1, "plugin_id": "large",
                "source": {"stage": "L2", "plugin_sha256": "a".repeat(64), "receipt_id": "record"},
                "descriptors": descriptors}))
            .unwrap();
            assert!(bytes.len() > 64 * 1024);
            fs::write(root.join(policy.path), bytes).unwrap();
            let loaded = load(&root, "large", policy);
            if count <= 4096 {
                let loaded = loaded.unwrap();
                assert_eq!(loaded.observed_descriptor_count, count as usize);
                assert_eq!(loaded.profile.descriptors.last().unwrap().slot, count);
            } else {
                assert!(loaded.is_err());
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
