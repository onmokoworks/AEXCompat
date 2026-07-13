pub mod scattermap;

use crate::host_core::parameter::PluginProfile;

pub fn find(id: &str) -> Option<&'static PluginProfile> {
    match id {
        scattermap::PROFILE_ID => Some(&scattermap::PROFILE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_explicit_and_unknown_profiles_fail_closed() {
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().id,
            scattermap::PROFILE_ID
        );
        assert!(find("unknown-aex").is_none());
    }
}
