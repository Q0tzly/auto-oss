use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RunState {
    pub repo: String,
    pub workdir: String,
    pub phase: String,
    pub started: u64,
    pub updated: u64,
}

fn runs_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".auto-oss").join("runs"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Tracks one `fix` run so `autos status` can show it from another terminal.
/// All writes are best-effort: a failing status file must never break a run.
pub struct RunTracker {
    path: Option<PathBuf>,
    state: RunState,
}

impl RunTracker {
    pub fn start(repo: &str, workdir: &Path) -> Self {
        let ts = now();
        let state = RunState {
            repo: repo.to_string(),
            workdir: workdir.display().to_string(),
            phase: "starting".into(),
            started: ts,
            updated: ts,
        };
        // pid in the filename: two concurrent runs against the same
        // repository in the same second must not share a status file.
        let path = runs_dir().map(|d| {
            d.join(format!(
                "{ts}-{}-{}.json",
                repo.replace('/', "-"),
                std::process::id()
            ))
        });
        let tracker = Self { path, state };
        tracker.write();
        tracker
    }

    pub fn set(&mut self, phase: &str) {
        self.state.phase = phase.to_string();
        self.state.updated = now();
        self.write();
    }

    fn write(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.state) {
            let _ = std::fs::write(path, json);
        }
    }
}

const TERMINAL_PHASES: [&str; 5] = [
    "submitted-pr",
    "submitted-issue",
    "aborted",
    "failed",
    "dry-run-done",
];

/// The `autos status` command: list recent runs, newest first. Files older
/// than seven days are pruned.
pub fn run() -> Result<()> {
    let Some(dir) = runs_dir() else {
        println!("no runs recorded");
        return Ok(());
    };
    let week_ago = now().saturating_sub(7 * 24 * 3600);
    let mut runs: Vec<RunState> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<RunState>(&raw) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            if state.updated < week_ago {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            runs.push(state);
        }
    }
    if runs.is_empty() {
        println!("no runs in the last 7 days");
        return Ok(());
    }
    runs.sort_by_key(|r| std::cmp::Reverse(r.started));
    println!("{:<10} {:<28} {:<18} WORKDIR", "UPDATED", "REPO", "PHASE");
    for r in &runs {
        let phase = if TERMINAL_PHASES.contains(&r.phase.as_str()) {
            r.phase.clone()
        } else if Path::new(&r.workdir).exists() {
            format!("{} (running?)", r.phase)
        } else {
            format!("{} (stale)", r.phase)
        };
        println!(
            "{:<10} {:<28} {:<18} {}",
            ago(r.updated),
            r.repo,
            phase,
            r.workdir
        );
    }
    Ok(())
}

fn ago(ts: u64) -> String {
    let secs = now().saturating_sub(ts);
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ages() {
        let t = now();
        assert!(ago(t).ends_with("s ago"));
        assert!(ago(t - 120).ends_with("m ago"));
        assert!(ago(t - 7200).ends_with("h ago"));
    }

    #[test]
    fn run_state_round_trips() {
        let s = RunState {
            repo: "a/b".into(),
            workdir: "/tmp/x".into(),
            phase: "gates".into(),
            started: 1,
            updated: 2,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repo, "a/b");
        assert_eq!(back.phase, "gates");
    }
}
