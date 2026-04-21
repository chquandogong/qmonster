use crate::domain::origin::SourceKind;

/// A single provider-configurable lever — a CLI flag, environment
/// variable, or `settings.json` key — packaged with its citation so
/// the UI can surface authority honestly. Levers inside a
/// `ProviderProfile` typically carry `SourceKind::ProviderOfficial`
/// because the value is copied from the provider's own documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLever {
    pub key: &'static str,
    pub value: &'static str,
    /// Short pointer to where the lever value came from — e.g.
    /// "Claude Code docs — environment variables, file-read budget".
    /// Never empty: a ProviderOfficial claim without a citation is a
    /// Heuristic, not an official lever.
    pub citation: &'static str,
    pub source_kind: SourceKind,
}

/// A named bundle of provider-native levers that can be recommended
/// for a `(provider, role, situation)` triple. The profile NAME is
/// `SourceKind::ProjectCanonical` (our abstraction); individual levers
/// inside the bundle keep their own `source_kind` (usually
/// `ProviderOfficial`). Phase 4 opens `side_effects` as a slot that
/// Gemini G-6 populates on high-compression profiles in a later
/// slice; P4-1 ships the shape with `side_effects: vec![]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProfile {
    pub name: &'static str,
    pub levers: Vec<ProfileLever>,
    pub side_effects: Vec<String>,
    pub source_kind: SourceKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_lever_pairs_key_value_with_provider_official_citation() {
        let lever = ProfileLever {
            key: "BASH_MAX_OUTPUT_LENGTH",
            value: "30000",
            citation: "Claude Code docs — environment variables, bash output cap",
            source_kind: SourceKind::ProviderOfficial,
        };
        assert_eq!(lever.source_kind, SourceKind::ProviderOfficial);
        assert!(
            !lever.citation.is_empty(),
            "every lever must carry a non-empty citation; a ProviderOfficial claim without a citation drifts into Heuristic"
        );
    }

    #[test]
    fn provider_profile_carries_project_canonical_name_with_provider_official_levers() {
        let profile = ProviderProfile {
            name: "claude-default",
            levers: vec![ProfileLever {
                key: "BASH_MAX_OUTPUT_LENGTH",
                value: "30000",
                citation: "Claude Code docs",
                source_kind: SourceKind::ProviderOfficial,
            }],
            side_effects: vec![],
            source_kind: SourceKind::ProjectCanonical,
        };
        assert_eq!(
            profile.source_kind,
            SourceKind::ProjectCanonical,
            "the profile NAME is our abstraction — ProjectCanonical"
        );
        assert_eq!(
            profile.levers[0].source_kind,
            SourceKind::ProviderOfficial,
            "individual levers inside keep their own authority label"
        );
        assert!(
            profile.side_effects.is_empty(),
            "P4-1 ships the shape with empty side_effects; Gemini G-6 populates in a later slice"
        );
    }
}
