pub mod scattermap;

use crate::host_core::parameter::PluginProfile;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterizedRenderAdapter {
    ScatterMap,
}

pub struct RegisteredProfile {
    pub parameters: &'static PluginProfile,
    pub parameterized_render: ParameterizedRenderAdapter,
}

static SCATTERMAP: RegisteredProfile = RegisteredProfile {
    parameters: &scattermap::PROFILE,
    parameterized_render: ParameterizedRenderAdapter::ScatterMap,
};

pub fn find(id: &str) -> Option<&'static RegisteredProfile> {
    match id {
        scattermap::PROFILE_ID => Some(&SCATTERMAP),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_explicit_and_unknown_profiles_fail_closed() {
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().parameters.id,
            scattermap::PROFILE_ID
        );
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().parameterized_render,
            ParameterizedRenderAdapter::ScatterMap
        );
        assert!(find("unknown-aex").is_none());
    }
}
