use crate::domain::identity::Provider;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// v1.60.0: pricing.toml is treated as stale once this many seconds
/// have passed since its mtime. 90 days picks up "operator forgot to
/// refresh after a major model rev" without firing on month-to-month
/// inactivity.
pub const PRICING_STALENESS_SECS: u64 = 60 * 60 * 24 * 90;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingRates {
    pub input_per_1m: f64,
    pub output_per_1m: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    // Note: `Io(NotFound)` already covers the missing-file case via
    // `std::io::ErrorKind::NotFound`, so a separate `NotFound` variant
    // would be dead — never constructed by the code.
    #[error("failed to read pricing config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse pricing config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unknown provider in pricing entry: {0}")]
    UnknownProvider(String),
}

#[derive(Debug, Default, Clone)]
pub struct PricingTable {
    entries: HashMap<(Provider, String), PricingRates>,
}

#[derive(Debug, Deserialize)]
struct PricingFile {
    #[serde(default)]
    entries: Vec<PricingEntry>,
}

#[derive(Debug, Deserialize)]
struct PricingEntry {
    provider: String,
    model: String,
    #[serde(default)]
    input_per_1m: f64,
    #[serde(default)]
    output_per_1m: f64,
}

impl PricingTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn load_from_toml(path: &Path) -> Result<Self, PricingError> {
        let text = fs::read_to_string(path)?;
        let file: PricingFile = toml::from_str(&text)?;
        let mut entries = HashMap::new();
        for e in file.entries {
            let provider = parse_provider(&e.provider)?;
            entries.insert(
                (provider, e.model),
                PricingRates {
                    input_per_1m: e.input_per_1m,
                    output_per_1m: e.output_per_1m,
                },
            );
        }
        Ok(Self { entries })
    }

    pub fn load_from_toml_or_empty(path: &Path) -> Self {
        Self::load_from_toml(path).unwrap_or_else(|_| Self::empty())
    }

    pub fn lookup(&self, provider: Provider, model: &str) -> Option<&PricingRates> {
        self.entries
            .get(&(provider, model.to_string()))
            .filter(|r| r.input_per_1m > 0.0 || r.output_per_1m > 0.0)
    }
}

/// v1.60.0: returns `Some(age_days)` when `path` exists and its mtime
/// is older than `threshold`. Returns `None` when the file is missing
/// (operator hasn't curated pricing yet — that's expected, no need to
/// nudge), when the mtime is newer than the threshold, or when fs
/// metadata can't be read for any reason. Pure helper so unit tests
/// can drive the threshold deterministically.
pub fn pricing_staleness_days(path: &Path, now: SystemTime, threshold: Duration) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let age = now.duration_since(mtime).ok()?;
    if age < threshold {
        return None;
    }
    Some(age.as_secs() / (60 * 60 * 24))
}

fn parse_provider(s: &str) -> Result<Provider, PricingError> {
    match s.to_lowercase().as_str() {
        "claude" => Ok(Provider::Claude),
        "codex" => Ok(Provider::Codex),
        "gemini" => Ok(Provider::Gemini),
        "qmonster" => Ok(Provider::Qmonster),
        _ => Err(PricingError::UnknownProvider(s.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(body: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", body).unwrap();
        f
    }

    #[test]
    fn pricing_table_empty_has_no_entries() {
        let t = PricingTable::empty();
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_loads_entries_from_toml() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.25
output_per_1m = 10.00

[[entries]]
provider = "claude"
model = "claude-sonnet-4-6"
input_per_1m = 3.00
output_per_1m = 15.00
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        let r = t.lookup(Provider::Codex, "gpt-5.4").unwrap();
        assert!((r.input_per_1m - 1.25).abs() < f64::EPSILON);
        assert!((r.output_per_1m - 10.00).abs() < f64::EPSILON);
        let r2 = t.lookup(Provider::Claude, "claude-sonnet-4-6").unwrap();
        assert!((r2.output_per_1m - 15.00).abs() < f64::EPSILON);
    }

    #[test]
    fn pricing_table_lookup_returns_none_for_missing_entry() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        assert!(t.lookup(Provider::Codex, "gpt-4o").is_none());
        assert!(t.lookup(Provider::Claude, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_treats_zero_rate_entries_as_unset() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 0.0
output_per_1m = 0.0
"#,
        );
        let t = PricingTable::load_from_toml(f.path()).unwrap();
        // Entry exists in file but zeros count as "operator has not filled in".
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }

    #[test]
    fn pricing_table_load_from_toml_or_empty_falls_back_on_missing() {
        let t = PricingTable::load_from_toml_or_empty(Path::new("/nonexistent/pricing.toml"));
        assert!(t.lookup(Provider::Codex, "gpt-5.4").is_none());
    }

    // v1.60.0 -----------------------------------------------------------

    #[test]
    fn pricing_staleness_days_returns_none_when_file_missing() {
        let path = Path::new("/nonexistent/pricing-stale-test.toml");
        let now = SystemTime::now();
        assert!(
            pricing_staleness_days(path, now, Duration::from_secs(1)).is_none(),
            "missing file is the no-curation case — should not nudge"
        );
    }

    #[test]
    fn pricing_staleness_days_returns_none_when_within_threshold() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#,
        );
        // File was just created → mtime is now; threshold of 90 days
        // means it's never stale at this moment.
        let now = SystemTime::now();
        let threshold = Duration::from_secs(60 * 60 * 24 * 90);
        assert!(pricing_staleness_days(f.path(), now, threshold).is_none());
    }

    #[test]
    fn pricing_staleness_days_returns_age_when_mtime_predates_threshold() {
        let f = write_toml(
            r#"
[[entries]]
provider = "codex"
model = "gpt-5.4"
input_per_1m = 1.0
output_per_1m = 10.0
"#,
        );
        // File was just created; pretend `now` is 100 days in the
        // future relative to the file's mtime.
        let mtime = std::fs::metadata(f.path()).unwrap().modified().unwrap();
        let now = mtime + Duration::from_secs(60 * 60 * 24 * 100);
        let threshold = Duration::from_secs(60 * 60 * 24 * 90);
        let age = pricing_staleness_days(f.path(), now, threshold).unwrap();
        assert_eq!(age, 100, "age in days should match the synthetic offset");
    }

    #[test]
    fn pricing_staleness_secs_constant_is_90_days() {
        assert_eq!(PRICING_STALENESS_SECS, 60 * 60 * 24 * 90);
    }
}
