use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// A coding agent that can turn a prompt into edits in a working directory.
/// auto-oss never calls an LLM itself; it delegates patch generation here.
pub trait Backend {
    fn name(&self) -> &'static str;
    fn generate(&self, workdir: &Path, prompt: &str) -> Result<()>;
}

pub fn by_name(name: &str) -> Result<Box<dyn Backend>> {
    match name {
        "claude-code" => Ok(Box::new(ClaudeCode)),
        other => bail!("unknown backend `{other}` (available: claude-code)"),
    }
}

struct ClaudeCode;

impl Backend for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn generate(&self, workdir: &Path, prompt: &str) -> Result<()> {
        let status = Command::new("claude")
            .args(["-p", prompt, "--permission-mode", "acceptEdits"])
            .current_dir(workdir)
            .status()
            .context("running `claude` (is Claude Code installed?)")?;
        if !status.success() {
            bail!("claude exited with {status}");
        }
        Ok(())
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
         - Match the existing code style of the repository.\n",
    );
    p
}
