use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{bail, Result};

use crate::policy::{self, Accepts, Fallback, Limits, MetadataCfg, Policy, Require};

/// Interactively generate an auto-oss.yml in the current directory.
pub fn run(force: bool) -> Result<()> {
    let target = Path::new("auto-oss.yml");
    if target.exists() && !force {
        bail!("auto-oss.yml already exists; pass --force to overwrite");
    }

    eprintln!("Generating an auto-oss policy for this repository.\n");
    let scopes = ask("Accepted scopes (comma-separated)", "bug-fix, docs, typo")?;
    let scopes: Vec<String> = scopes
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if scopes.is_empty() {
        bail!("at least one scope is required");
    }
    let max_diff = ask("Max changed lines per patch (empty for no limit)", "300")?;
    let build = ask("Build gate command (empty to skip)", "")?;
    let test = ask("Test gate command (empty to skip)", "")?;
    let lint = ask("Lint gate command (empty to skip)", "")?;
    let repro = ask("Require reproduction steps for bug fixes? (y/n)", "y")?;
    let fallback = ask(
        "Fallback when a patch does not qualify (issue/none)",
        "issue",
    )?;
    let label = ask("Label for submissions (empty to skip)", "auto-oss")?;

    let max_diff_lines = if max_diff.is_empty() {
        None
    } else {
        let n: u64 = max_diff
            .parse()
            .map_err(|_| anyhow::anyhow!("max changed lines must be a number, got `{max_diff}`"))?;
        Some(n)
    };

    let mut gates = BTreeMap::new();
    if !build.is_empty() {
        gates.insert("build".to_string(), build);
    }
    if !test.is_empty() {
        gates.insert("test".to_string(), test);
    }
    if !lint.is_empty() {
        gates.insert("lint".to_string(), lint);
    }

    let policy = Policy {
        version: 0,
        accepts: Accepts {
            scopes,
            max_diff_lines,
        },
        gates,
        require: Require {
            human_review: true,
            reproduction: matches!(repro.as_str(), "y" | "Y" | "yes"),
        },
        fallback: match fallback.as_str() {
            "none" => Fallback::None,
            "discussion" => Fallback::Discussion,
            _ => Fallback::Issue,
        },
        limits: Limits::default(),
        metadata: if label.is_empty() {
            MetadataCfg::default()
        } else {
            MetadataCfg {
                label: Some(label),
                language: None,
            }
        },
    };

    let out = serde_yaml::to_string(&policy)?;
    // Round-trip through the parser so we never write an invalid policy.
    policy::parse(&out)?;
    std::fs::write(target, &out)?;
    eprintln!("\nWrote auto-oss.yml:\n\n{out}");
    Ok(())
}

fn ask(question: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        eprint!("{question}: ");
    } else {
        eprint!("{question} [{default}]: ");
    }
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
    })
}
