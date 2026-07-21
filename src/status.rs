use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub repo: String,
    pub workdir: String,
    pub phase: String,
    pub started: u64,
    pub updated: u64,
    // Everything below is needed to resume a run that got interrupted
    // (Ctrl-C, terminal closed, ...) partway through. Old run files written
    // before these fields existed still parse: each defaults to empty/None,
    // and `resume` treats a missing `repo_arg` as unresumable.
    #[serde(default)]
    pub repo_arg: String,
    #[serde(default)]
    pub feedback: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub repro: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

/// Fields needed to start tracking a new `fix` run, kept separate from
/// `fix::FixArgs` so this module doesn't depend on it.
pub struct NewRun<'a> {
    pub repo: &'a str,
    pub repo_arg: &'a str,
    pub workdir: &'a Path,
    pub feedback: &'a str,
    pub scope: &'a str,
    pub repro: Option<&'a str>,
    pub backend: Option<&'a str>,
    pub dry_run: bool,
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

/// Tracks one `fix` run so `autos status` can show it from another terminal,
/// and so an interrupted run can be picked back up with `autos resume`.
/// All writes are best-effort: a failing status file must never break a run.
pub struct RunTracker {
    path: Option<PathBuf>,
    state: RunState,
}

impl RunTracker {
    pub fn start(new: NewRun) -> Self {
        let ts = now();
        let state = RunState {
            repo: new.repo.to_string(),
            workdir: new.workdir.display().to_string(),
            phase: "starting".into(),
            started: ts,
            updated: ts,
            repo_arg: new.repo_arg.to_string(),
            feedback: new.feedback.to_string(),
            scope: new.scope.to_string(),
            repro: new.repro.map(str::to_string),
            backend: new.backend.map(str::to_string),
            dry_run: new.dry_run,
            title: None,
            summary: None,
        };
        // pid in the filename: two concurrent runs against the same
        // repository in the same second must not share a status file.
        let path = runs_dir().map(|d| {
            d.join(format!(
                "{ts}-{}-{}.json",
                new.repo.replace('/', "-"),
                std::process::id()
            ))
        });
        let tracker = Self { path, state };
        tracker.write();
        tracker
    }

    /// Continue writing to an existing run's status file, for `resume`.
    pub fn attach(path: PathBuf, state: RunState) -> Self {
        Self {
            path: Some(path),
            state,
        }
    }

    pub fn set(&mut self, phase: &str) {
        self.state.phase = phase.to_string();
        self.state.updated = now();
        self.write();
    }

    /// Record what the backend produced, once known, so a later `resume`
    /// (after this process dies) doesn't have to re-run the backend to
    /// recover the title and change summary.
    pub fn set_generated(&mut self, title: Option<&str>, summary: Option<&str>) {
        self.state.title = title.map(str::to_string);
        self.state.summary = summary.map(str::to_string);
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

pub fn is_terminal(phase: &str) -> bool {
    TERMINAL_PHASES.contains(&phase)
}

/// Find the most recently updated tracked run for a work directory, if any.
/// Paths are compared after canonicalizing both sides so `.` vs an absolute
/// path (or a trailing slash) doesn't cause a false miss.
pub fn find_run(workdir: &Path) -> Result<Option<(PathBuf, RunState)>> {
    let Some(dir) = runs_dir() else {
        return Ok(None);
    };
    let target = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    let mut matches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<RunState>(&raw) else {
                continue;
            };
            let state_wd = PathBuf::from(&state.workdir);
            let state_wd = state_wd.canonicalize().unwrap_or(state_wd);
            if state_wd == target {
                matches.push((path, state));
            }
        }
    }
    matches.sort_by_key(|(_, s)| std::cmp::Reverse(s.updated));
    Ok(matches.into_iter().next())
}

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
    println!("{:<10} {:<28} {:<22} WORKDIR", "UPDATED", "REPO", "PHASE");
    for r in &runs {
        let resumable = !is_terminal(&r.phase) && !r.repo_arg.is_empty();
        let phase = if is_terminal(&r.phase) {
            r.phase.clone()
        } else if Path::new(&r.workdir).exists() {
            format!("{} (running?)", r.phase)
        } else {
            format!("{} (stale)", r.phase)
        };
        println!(
            "{:<10} {:<28} {:<22} {}",
            ago(r.updated),
            r.repo,
            phase,
            r.workdir
        );
        if resumable {
            println!("           resume with: autos resume {}", r.workdir);
        }
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
            repo_arg: "a/b".into(),
            feedback: "it's broken".into(),
            scope: "bug-fix".into(),
            repro: Some("steps".into()),
            backend: None,
            dry_run: false,
            title: Some("Fix it".into()),
            summary: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repo, "a/b");
        assert_eq!(back.phase, "gates");
        assert_eq!(back.feedback, "it's broken");
        assert_eq!(back.title.as_deref(), Some("Fix it"));
    }

    #[test]
    fn old_run_files_without_resume_fields_still_parse() {
        let old = r#"{"repo":"a/b","workdir":"/tmp/x","phase":"gates","started":1,"updated":2}"#;
        let state: RunState = serde_json::from_str(old).unwrap();
        assert_eq!(state.repo_arg, "");
        assert!(state.repro.is_none());
        assert!(!state.dry_run);
    }

    #[test]
    fn terminal_phases_are_not_resumable() {
        assert!(is_terminal("submitted-pr"));
        assert!(is_terminal("aborted"));
        assert!(!is_terminal("awaiting-gate-approval"));
        assert!(!is_terminal("gates"));
    }
}
