use crate::host_core::parameter::{
    ColorValue, Descriptor, ParameterValue, PluginProfile, ValueKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

const MANIFEST_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Copy)]
pub struct ManifestPolicy {
    pub path: &'static str,
    pub sha256: &'static str,
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
    let canonical = serde_json::to_vec(&manifest).map_err(|error| invalid(error.to_string()))?;
    let actual = format!("{:X}", Sha256::digest(canonical));
    if !actual.eq_ignore_ascii_case(policy.sha256) {
        return Err(invalid("descriptor manifest digest mismatch"));
    }
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
        || manifest.descriptors.len() > 1024
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
