use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::adapters::process_memory::CliProcessDescriptor;
use crate::domain::identity::Provider;
use crate::domain::origin::SourceKind;
use crate::domain::signal::{RuntimeFact, RuntimeFactKind};

pub(crate) type CliVersionCache = HashMap<CliVersionCacheKey, Option<RuntimeFact>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CliVersionCacheKey {
    provider: Provider,
    pid: u32,
    comm: String,
    argv: Vec<String>,
    exe_path: Option<String>,
}

impl CliVersionCacheKey {
    fn from_descriptor(provider: Provider, desc: &CliProcessDescriptor) -> Self {
        Self {
            provider,
            pid: desc.pid,
            comm: desc.comm.clone(),
            argv: desc.argv.clone(),
            exe_path: desc
                .exe_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        }
    }
}

pub(crate) fn resolve_cli_version_fact(
    provider: Provider,
    tail: &str,
    pane_pid: Option<u32>,
    cache: &mut CliVersionCache,
) -> Option<RuntimeFact> {
    if provider == Provider::Qmonster {
        return None;
    }
    if let Some(fact) = parse_cli_version_from_tail(provider, tail) {
        return Some(fact);
    }

    let desc = pane_pid.and_then(crate::adapters::process_memory::read_descendant_cli_process)?;
    let key = CliVersionCacheKey::from_descriptor(provider, &desc);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let fact = probe_cli_version(&desc).map(|fact| fact.with_provider(provider));
    cache.insert(key, fact.clone());
    fact
}

pub(crate) fn parse_cli_version_from_tail(provider: Provider, tail: &str) -> Option<RuntimeFact> {
    if provider == Provider::Qmonster {
        return None;
    }
    for line in tail.lines() {
        let lower = line.to_ascii_lowercase();
        let anchored = match provider {
            Provider::Claude => lower.contains("claude code") || lower.contains("claude cli"),
            Provider::Codex => lower.contains("openai codex") || lower.contains("codex cli"),
            Provider::Gemini => lower.contains("gemini cli"),
            Provider::Qmonster | Provider::Unknown => false,
        };
        // Require an explicit `v` prefix on the version token. Chat
        // and status lines often mention models like "Opus 4.7" next
        // to "Claude Code", and accepting bare numerics would surface
        // the model version as the CLI version.
        if anchored && let Some(version) = extract_version_token(line, true) {
            return Some(cli_version_fact(version, Some(provider)));
        }
    }
    None
}

pub(crate) fn probe_cli_version(desc: &CliProcessDescriptor) -> Option<RuntimeFact> {
    let (program, args) = version_probe_command(desc)?;
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let first_line = first_non_empty_output_line(&out.stdout)
        .or_else(|| first_non_empty_output_line(&out.stderr))?;
    let version = extract_version_token(&first_line, false)?;
    Some(cli_version_fact(version, None))
}

fn version_probe_command(desc: &CliProcessDescriptor) -> Option<(PathBuf, Vec<String>)> {
    let program = exact_program_path(desc)?;
    match desc.comm.as_str() {
        "claude" | "codex" | "gemini" => Some((program, vec!["--version".into()])),
        "node" | "nodejs" | "python" | "python3" => {
            let script = cli_script_arg(&desc.argv)?;
            Some((program, vec![script, "--version".into()]))
        }
        _ => None,
    }
}

fn exact_program_path(desc: &CliProcessDescriptor) -> Option<PathBuf> {
    if let Some(path) = desc.exe_path.as_ref() {
        return Some(path.clone());
    }
    let argv0 = desc.argv.first()?;
    let path = PathBuf::from(argv0);
    path.is_absolute().then_some(path)
}

fn cli_script_arg(argv: &[String]) -> Option<String> {
    argv.iter()
        .skip(1)
        .find(|arg| path_basename_contains_known_cli(arg))
        .cloned()
}

fn path_basename_contains_known_cli(path: &str) -> bool {
    let basename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    ["claude", "codex", "gemini"]
        .iter()
        .any(|needle| basename.contains(needle))
}

fn first_non_empty_output_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn extract_version_token(text: &str, require_v_prefix: bool) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        let ch = chars[start];
        let starts_with_v = matches!(ch, 'v' | 'V')
            && chars
                .get(start + 1)
                .map(|next| next.is_ascii_digit())
                .unwrap_or(false);
        if require_v_prefix {
            if !starts_with_v {
                continue;
            }
        } else if !starts_with_v && !ch.is_ascii_digit() {
            continue;
        }

        let token_start = if starts_with_v { start + 1 } else { start };
        let mut end = token_start;
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || matches!(chars[end], '.' | '-' | '+' | '_'))
        {
            end += 1;
        }
        let token: String = chars[token_start..end].iter().collect();
        if token.contains('.') && token.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Some(token);
        }
    }
    None
}

fn cli_version_fact(version: String, provider: Option<Provider>) -> RuntimeFact {
    let fact = RuntimeFact::new(
        RuntimeFactKind::CliVersion,
        version,
        SourceKind::ProviderOfficial,
    )
    .with_confidence(0.98);
    if let Some(provider) = provider {
        fact.with_provider(provider)
    } else {
        fact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::process_memory::CliProcessDescriptor;
    use crate::domain::identity::Provider;
    use crate::domain::origin::SourceKind;
    use crate::domain::signal::RuntimeFactKind;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_executable(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn codex_status_surface_extracts_cli_version() {
        let tail = "│  >_ OpenAI Codex (v0.122.0)  │";

        let fact = parse_cli_version_from_tail(Provider::Codex, tail).unwrap();

        assert_eq!(fact.kind, RuntimeFactKind::CliVersion);
        assert_eq!(fact.value, "0.122.0");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn gemini_welcome_surface_extracts_cli_version() {
        let tail = " ▝▜▄     Gemini CLI v0.39.0-preview.0";

        let fact = parse_cli_version_from_tail(Provider::Gemini, tail).unwrap();

        assert_eq!(fact.kind, RuntimeFactKind::CliVersion);
        assert_eq!(fact.value, "0.39.0-preview.0");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn claude_banner_surface_extracts_cli_version() {
        let tail = "Claude Code v2.1.141";

        let fact = parse_cli_version_from_tail(Provider::Claude, tail).unwrap();

        assert_eq!(fact.kind, RuntimeFactKind::CliVersion);
        assert_eq!(fact.value, "2.1.141");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn claude_model_mention_does_not_leak_into_cli_version() {
        // Chat/status text mentioning a model next to "Claude Code"
        // must not be mistaken for the CLI version.
        let tail = "I'm using Claude Code with Opus 4.7 today.";

        assert!(parse_cli_version_from_tail(Provider::Claude, tail).is_none());
    }

    #[test]
    fn claude_anchor_without_version_token_yields_none() {
        let tail = "✻ Welcome to Claude Code!\nSelect model > Opus 4.7";

        assert!(parse_cli_version_from_tail(Provider::Claude, tail).is_none());
    }

    #[test]
    fn native_cli_probe_uses_exact_proc_exe() {
        let tmp = tempdir().unwrap();
        let exe = tmp.path().join("claude-exact");
        write_executable(&exe, "#!/bin/sh\necho 'Claude Code 1.2.3'\n");
        let desc = CliProcessDescriptor {
            pid: 42,
            comm: "claude".into(),
            argv: vec![exe.to_string_lossy().into_owned()],
            exe_path: Some(exe),
        };

        let fact = probe_cli_version(&desc).unwrap();

        assert_eq!(fact.kind, RuntimeFactKind::CliVersion);
        assert_eq!(fact.value, "1.2.3");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    }

    #[test]
    fn node_cli_probe_uses_exact_interpreter_and_script() {
        let tmp = tempdir().unwrap();
        let node = tmp.path().join("node-exact");
        let script = tmp.path().join("codex.js");
        fs::write(&script, "// fixture\n").unwrap();
        write_executable(
            &node,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"{}\" ] && [ \"$2\" = \"--version\" ]; then echo 'codex-cli 0.122.0'; exit 0; fi\nexit 7\n",
                script.display()
            ),
        );
        let desc = CliProcessDescriptor {
            pid: 77,
            comm: "node".into(),
            argv: vec![
                node.to_string_lossy().into_owned(),
                script.to_string_lossy().into_owned(),
                "--session".into(),
                "abc".into(),
            ],
            exe_path: Some(node),
        };

        let fact = probe_cli_version(&desc).unwrap();

        assert_eq!(fact.kind, RuntimeFactKind::CliVersion);
        assert_eq!(fact.value, "0.122.0");
        assert_eq!(fact.source_kind, SourceKind::ProviderOfficial);
    }
}
