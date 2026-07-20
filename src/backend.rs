use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// A coding agent that can turn a prompt into edits in a working directory.
/// auto-oss never calls an LLM itself; it delegates patch generation here.
/// `generate` may return a human-readable summary of the change, which ends
/// up in the submission body.
pub trait Backend {
    fn name(&self) -> &str;
    fn generate(&self, workdir: &Path, prompt: &str) -> Result<Option<String>>;
}

/// User-side configuration, `~/.auto-oss/config.yml`:
///
/// ```yaml
/// default_backend: claude-code
/// backends:
///   codex:
///     command: ["codex", "exec", "{prompt}"]
/// ```
///
/// Custom backends run in the clone's working directory with `{prompt}`
/// substituted; they are expected to edit files and exit 0.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_backend: Option<String>,
    pub backends: BTreeMap<String, CustomBackendCfg>,
}

#[derive(Debug, Deserialize)]
pub struct CustomBackendCfg {
    pub command: Vec<String>,
}

pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".auto-oss").join("config.yml"))
}

pub fn load_config() -> Result<Config> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_yaml::from_str(&raw)
            .with_context(|| format!("invalid config at {}", path.display())),
        Err(_) => Ok(Config::default()),
    }
}

/// Resolve the backend: explicit flag > config `default_backend` > claude-code.
pub fn resolve(flag: Option<&str>, config: &Config) -> Result<Box<dyn Backend>> {
    let name = flag
        .map(str::to_string)
        .or_else(|| config.default_backend.clone())
        .unwrap_or_else(|| "claude-code".to_string());
    match name.as_str() {
        "claude-code" => Ok(Box::new(ClaudeCode)),
        "human" => Ok(Box::new(Human)),
        other => {
            if let Some(cfg) = config.backends.get(other) {
                if !cfg.command.iter().any(|a| a.contains("{prompt}")) {
                    bail!(
                        "backend `{other}` in config.yml has no `{{prompt}}` placeholder \
                         in its command"
                    );
                }
                Ok(Box::new(Custom {
                    name: other.to_string(),
                    command: cfg.command.clone(),
                }))
            } else {
                bail!(
                    "unknown backend `{other}` (built-in: claude-code, human; \
                     custom backends are defined in ~/.auto-oss/config.yml)"
                );
            }
        }
    }
}

struct ClaudeCode;

impl Backend for ClaudeCode {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn generate(&self, workdir: &Path, prompt: &str) -> Result<Option<String>> {
        // stream-json makes progress visible while the agent works; plain -p
        // is silent until the very end, which reads as a hang.
        let mut child = Command::new("claude")
            .args([
                "-p",
                prompt,
                "--permission-mode",
                "acceptEdits",
                "--output-format",
                "stream-json",
                "--verbose",
            ])
            .current_dir(workdir)
            // The user's stdin belongs to the confirmation prompts, not to
            // the agent; claude treats piped stdin as prompt input.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .context("running `claude` (is Claude Code installed?)")?;

        let stdout = child.stdout.take().expect("piped stdout");
        let mut errored = None;
        let mut summary = None;
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            report_claude_event(&event, &mut errored, &mut summary);
        }
        let status = child.wait()?;
        if let Some(e) = errored {
            bail!("claude reported an error: {e}");
        }
        if !status.success() {
            bail!("claude exited with {status}");
        }
        Ok(summary)
    }
}

/// Print a compact progress line per salient stream event. The final result
/// text — the agent's own account of what it changed — becomes the summary.
fn report_claude_event(
    event: &serde_json::Value,
    errored: &mut Option<String>,
    summary: &mut Option<String>,
) {
    match event["type"].as_str() {
        Some("assistant") => {
            let Some(blocks) = event["message"]["content"].as_array() else {
                return;
            };
            for block in blocks {
                match block["type"].as_str() {
                    Some("text") => {
                        for l in block["text"].as_str().unwrap_or("").lines() {
                            if !l.trim().is_empty() {
                                eprintln!("    [claude] {l}");
                            }
                        }
                    }
                    Some("tool_use") => {
                        let tool = block["name"].as_str().unwrap_or("?");
                        let target = block["input"]["file_path"]
                            .as_str()
                            .or_else(|| block["input"]["command"].as_str())
                            .or_else(|| block["input"]["pattern"].as_str())
                            .unwrap_or("");
                        let mut target = target.replace('\n', " ");
                        if target.len() > 80 {
                            target.truncate(80);
                            target.push('…');
                        }
                        eprintln!("    [claude] {tool} {target}");
                    }
                    _ => {}
                }
            }
        }
        Some("result") => {
            if event["is_error"].as_bool() == Some(true) {
                *errored = Some(
                    event["result"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_string(),
                );
            } else if let Some(text) = event["result"].as_str() {
                if !text.trim().is_empty() {
                    *summary = Some(text.trim().to_string());
                }
            }
        }
        _ => {}
    }
}

/// You are the backend. Make the edits yourself; the rest of the pipeline
/// (gates, metadata, submission) treats you exactly like any agent.
struct Human;

impl Backend for Human {
    fn name(&self) -> &str {
        "human"
    }

    fn generate(&self, workdir: &Path, prompt: &str) -> Result<Option<String>> {
        eprintln!("==> backend `human`: make your changes now.\n");
        eprintln!("{prompt}");
        eprintln!("\n    workdir: {}", workdir.display());
        eprint!("    Press Enter when your edits are done... ");
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .context("reading stdin")?;
        eprint!("    Describe the change for the submission body (empty to skip): ");
        let mut desc = String::new();
        std::io::stdin()
            .read_line(&mut desc)
            .context("reading stdin")?;
        let desc = desc.trim();
        Ok((!desc.is_empty()).then(|| desc.to_string()))
    }
}

/// A backend defined in the user's config: an arbitrary command with the
/// prompt substituted for `{prompt}`.
struct Custom {
    name: String,
    command: Vec<String>,
}

impl Backend for Custom {
    fn name(&self) -> &str {
        &self.name
    }

    fn generate(&self, workdir: &Path, prompt: &str) -> Result<Option<String>> {
        let argv: Vec<String> = self
            .command
            .iter()
            .map(|a| a.replace("{prompt}", prompt))
            .collect();
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(workdir)
            .stdin(Stdio::null())
            .status()
            .with_context(|| format!("running backend `{}` ({})", self.name, argv[0]))?;
        if !status.success() {
            bail!("backend `{}` exited with {status}", self.name);
        }
        Ok(None)
    }
}

pub fn build_prompt(
    feedback: &str,
    reproduction: Option<&str>,
    scope: &str,
    max_diff_lines: Option<u64>,
) -> String {
    let mut p = format!(
        "You are generating a patch for this repository on behalf of one of its users, \
         under the auto-oss protocol.\n\n\
         User feedback (verbatim):\n{feedback}\n"
    );
    if let Some(repro) = reproduction {
        p.push_str(&format!("\nReproduction steps:\n{repro}\n"));
    }
    p.push_str(&format!(
        "\nConstraints:\n\
         - The change must fall within the `{scope}` scope. Do not fix unrelated issues.\n\
         - Keep the change as small as possible"
    ));
    if let Some(max) = max_diff_lines {
        p.push_str(&format!(" (hard limit: {max} changed lines)"));
    }
    p.push_str(
        ".\n- Edit files only. Do not commit, push, or create branches.\n\
         - Match the existing code style of the repository.\n\
         - When you are done, end your reply with a short plain-language \
         summary of what you changed and why; it will be shown to the \
         project's maintainers.\n",
    );
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_flag_over_config_default() {
        let config: Config =
            serde_yaml::from_str("default_backend: human\nbackends: {}\n").unwrap();
        assert_eq!(
            resolve(Some("claude-code"), &config).unwrap().name(),
            "claude-code"
        );
        assert_eq!(resolve(None, &config).unwrap().name(), "human");
        assert_eq!(
            resolve(None, &Config::default()).unwrap().name(),
            "claude-code"
        );
    }

    #[test]
    fn resolves_custom_backend_from_config() {
        let config: Config = serde_yaml::from_str(
            "backends:\n  codex:\n    command: [\"codex\", \"exec\", \"{prompt}\"]\n",
        )
        .unwrap();
        assert_eq!(resolve(Some("codex"), &config).unwrap().name(), "codex");
        assert!(resolve(Some("nonexistent"), &config).is_err());
    }

    #[test]
    fn custom_backend_requires_prompt_placeholder() {
        let config: Config =
            serde_yaml::from_str("backends:\n  bad:\n    command: [\"true\"]\n").unwrap();
        assert!(resolve(Some("bad"), &config).is_err());
    }

    #[test]
    fn reports_error_result_events() {
        let mut errored = None;
        let mut summary = None;
        let event: serde_json::Value =
            serde_json::from_str(r#"{"type":"result","is_error":true,"result":"boom"}"#).unwrap();
        report_claude_event(&event, &mut errored, &mut summary);
        assert_eq!(errored.as_deref(), Some("boom"));
        assert!(summary.is_none());
    }

    #[test]
    fn captures_result_text_as_summary() {
        let mut errored = None;
        let mut summary = None;
        let event: serde_json::Value = serde_json::from_str(
            r#"{"type":"result","is_error":false,"result":"Fixed the typo in README."}"#,
        )
        .unwrap();
        report_claude_event(&event, &mut errored, &mut summary);
        assert!(errored.is_none());
        assert_eq!(summary.as_deref(), Some("Fixed the typo in README."));
    }
}
