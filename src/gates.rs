use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateResult {
    Pass,
    Fail,
    Skipped,
}

impl fmt::Display for GateResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GateResult::Pass => write!(f, "pass"),
            GateResult::Fail => write!(f, "fail"),
            GateResult::Skipped => write!(f, "skipped"),
        }
    }
}

/// Run every declared gate in the work directory. Output streams to the
/// user's terminal; per SPEC §4 all gates must pass for a PR submission.
pub fn run_all(gates: &BTreeMap<String, String>, dir: &Path) -> Result<Vec<(String, GateResult)>> {
    let mut results = Vec::new();
    for (name, cmd) in gates {
        eprintln!("==> gate `{name}`: {cmd}");
        let status = Command::new("sh")
            .args(["-c", cmd])
            .current_dir(dir)
            .status()
            .with_context(|| format!("running gate `{name}`"))?;
        let result = if status.success() {
            GateResult::Pass
        } else {
            GateResult::Fail
        };
        eprintln!("==> gate `{name}`: {result}");
        results.push((name.clone(), result));
    }
    Ok(results)
}

pub fn all_pass(results: &[(String, GateResult)]) -> bool {
    results.iter().all(|(_, r)| *r == GateResult::Pass)
}
