use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    ProviderOfficial,
    ProjectCanonical,
    Heuristic,
    Estimated,
}

impl SourceKind {
    pub fn badge(self) -> &'static str {
        match self {
            SourceKind::ProviderOfficial => "PO",
            SourceKind::ProjectCanonical => "PC",
            SourceKind::Heuristic => "HE",
            SourceKind::Estimated => "ES",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SourceKind::ProviderOfficial => "ProviderOfficial",
            SourceKind::ProjectCanonical => "ProjectCanonical",
            SourceKind::Heuristic => "Heuristic",
            SourceKind::Estimated => "Estimated",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_is_two_letter_per_variant() {
        assert_eq!(SourceKind::ProviderOfficial.badge(), "PO");
        assert_eq!(SourceKind::ProjectCanonical.badge(), "PC");
        assert_eq!(SourceKind::Heuristic.badge(), "HE");
        assert_eq!(SourceKind::Estimated.badge(), "ES");
    }

    #[test]
    fn display_is_long_form() {
        assert_eq!(SourceKind::ProviderOfficial.to_string(), "ProviderOfficial");
        assert_eq!(SourceKind::Heuristic.to_string(), "Heuristic");
    }
}
