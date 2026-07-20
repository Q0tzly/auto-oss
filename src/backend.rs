use std::path::Path;
use std::process::{Command, Stdio};

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
        "human" => Ok(Box::new(Human)),
        other => bail!("unknown backend `{other}` (available: claude-code, human)"),
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
            // The user's stdin belongs to the confirmation prompts, not to
            // the agent; claude treats piped stdin as prompt input.
            .stdin(Stdio::null())
            .status()
            .context("running `claude` (is Claude Code installed?)")?;
        if !status.success() {
            bail!("claude exited with {status}");
        }
        Ok(())
    }
}

/// You are the backend. Make the edits yourself; the rest of the pipeline
/// (gates, metadata, submission) treats you exactly like any agent.
struct Human;

impl Backend for Human {
    fn name(&self) -> &'static str {
        "human"
    }

    fn generate(&self, workdir: &Path, prompt: &str) -> Result<()> {
        eprintln!("==> backend `human`: make your changes now.\n");
        eprintln!("{prompt}");
        eprintln!("\n    workdir: {}", workdir.display());
        eprint!("    Press Enter when your edits are done... ");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).context("reading stdin")?;
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
