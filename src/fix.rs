use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::backend;
use crate::gates::{self, GateResult};
use crate::metadata::{self, Submission};
use crate::policy::{self, Fallback, Policy, PolicyStatus, RepoRef};
use crate::status::RunTracker;

pub struct FixArgs {
    pub repo: String,
    pub feedback: String,
    pub scope: String,
    pub repro: Option<String>,
    pub backend: Option<String>,
    pub dry_run: bool,
}

pub fn run(args: FixArgs) -> Result<()> {
    let repo = RepoRef::parse(&args.repo)?;
    let policy = match policy::discover(&repo)? {
        PolicyStatus::OptedIn { policy, found_at } => {
            eprintln!("==> policy found at {found_at}");
            policy
        }
        PolicyStatus::NotOptedIn => bail!(
            "{} has not opted in to auto-oss (no policy file); \
             the protocol forbids submitting agent PRs to it.\n\
             You can still open an ordinary issue by hand.",
            repo.short_name()
        ),
        PolicyStatus::Unusable { found_at, reason } => bail!(
            "{}: policy file {found_at} is unusable ({reason}); treating as not opted in",
            repo.short_name()
        ),
    };

    validate_request(&policy, &args)?;
    if !args.dry_run {
        enforce_limit(&policy, &repo)?;
    }
    let config = backend::load_config()?;
    let backend = backend::resolve(args.backend.as_deref(), &config)?;

    let workdir = make_workdir(&repo)?;
    let mut tracker = RunTracker::start(&repo.short_name(), &workdir);
    eprintln!(
        "==> cloning {} into {}",
        repo.short_name(),
        workdir.display()
    );
    tracker.set("cloning");
    git(&workdir, &["clone", "--quiet", &repo.clone_url(), "."])?;

    eprintln!("==> generating patch with {}", backend.name());
    tracker.set("generating");
    let prompt = backend::build_prompt(
        &args.feedback,
        args.repro.as_deref(),
        &args.scope,
        policy.accepts.max_diff_lines,
        policy.metadata.language.as_deref(),
    );
    let generated = match backend.generate(&workdir, &prompt) {
        Ok(g) => g,
        Err(e) => {
            tracker.set("failed");
            return Err(e);
        }
    };

    git(&workdir, &["add", "-A"])?;
    let diff = git_out(&workdir, &["diff", "--cached"])?;
    if diff.trim().is_empty() {
        tracker.set("failed");
        bail!("backend produced no changes; nothing to submit");
    }
    let changed = diff_lines(&diff);
    eprintln!("\n{diff}");
    eprintln!("==> {changed} changed lines");

    let oversized = policy
        .accepts
        .max_diff_lines
        .is_some_and(|max| changed > max);
    if oversized {
        eprintln!(
            "==> patch exceeds max_diff_lines ({} > {}); downgrading to fallback",
            changed,
            policy.accepts.max_diff_lines.unwrap()
        );
    }

    let gate_results = if oversized {
        policy
            .gates
            .keys()
            .map(|k| (k.clone(), GateResult::Skipped))
            .collect()
    } else {
        tracker.set("gates");
        gates::run_all(&policy.gates, &workdir)?
    };
    let qualified = !oversized && gates::all_pass(&gate_results);

    let title = pr_title(&args, generated.title.as_deref());
    let body = submission_body(
        &args,
        backend.name(),
        backend.model(),
        generated.summary.as_deref(),
        &gate_results,
        qualified,
        &diff,
    );
    let body_path = workdir.join(".auto-oss-body.md");
    std::fs::write(&body_path, &body)?;

    eprintln!("\n----- submission preview -----");
    eprintln!("title: {title}");
    eprintln!("{body}");
    eprintln!("------------------------------");

    if args.dry_run {
        tracker.set("dry-run-done");
        eprintln!("==> dry run: stopping before submission");
        eprintln!("    workdir: {}", workdir.display());
        eprintln!("    body:    {}", body_path.display());
        return Ok(());
    }

    let RepoRef::GitHub { owner, repo: name } = &repo else {
        tracker.set("dry-run-done");
        eprintln!("==> local repository: submission not applicable; review the diff in place");
        eprintln!("    workdir: {}", workdir.display());
        return Ok(());
    };

    if !qualified {
        tracker.set("awaiting-approval");
        if submit_fallback(&policy, owner, name, &title, &body_path)? {
            tracker.set("submitted-issue");
            record_submission(&repo)?;
        } else {
            tracker.set("aborted");
        }
        return Ok(());
    }

    tracker.set("awaiting-approval");
    if !confirm("Review the diff and preview above. Submit this pull request?")? {
        tracker.set("aborted");
        eprintln!(
            "==> aborted; nothing submitted (workdir kept at {})",
            workdir.display()
        );
        return Ok(());
    }
    tracker.set("submitting");
    if let Err(e) = submit_pr(&policy, owner, name, &args, &workdir, &title, &body_path) {
        tracker.set("failed");
        return Err(e);
    }
    tracker.set("submitted-pr");
    record_submission(&repo)
}

fn validate_request(policy: &Policy, args: &FixArgs) -> Result<()> {
    if args.feedback.trim().is_empty() {
        bail!("feedback must not be empty; it is the provenance of the whole submission");
    }
    if !policy.accepts.scopes.iter().any(|s| s == &args.scope) {
        bail!(
            "scope `{}` is not accepted by this repository (accepted: {})",
            args.scope,
            policy.accepts.scopes.join(", ")
        );
    }
    if policy.require.reproduction && args.scope == "bug-fix" && args.repro.is_none() {
        bail!("this repository requires reproduction steps for bug fixes; pass --repro");
    }
    Ok(())
}

/// SPEC §4: clients SHOULD respect declared limits without server-side
/// enforcement. Submissions are logged locally, one `epoch<TAB>repo` line
/// per submission, and counted over a rolling week.
fn submission_log() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".auto-oss").join("submissions.tsv"))
}

fn enforce_limit(policy: &Policy, repo: &RepoRef) -> Result<()> {
    let (Some(limit), Some(log)) = (policy.limits.per_author_per_week, submission_log()) else {
        return Ok(());
    };
    let week_ago = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() - 7 * 24 * 3600;
    let name = repo.short_name();
    let recent = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(ts, r)| ts.parse::<u64>().is_ok_and(|t| t >= week_ago) && *r == name)
        .count() as u64;
    if recent >= limit {
        bail!(
            "{name} declares a limit of {limit} submission(s) per author per week and you \
             have made {recent} in the last 7 days; try again later"
        );
    }
    Ok(())
}

fn record_submission(repo: &RepoRef) -> Result<()> {
    let Some(log) = submission_log() else {
        return Ok(());
    };
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    // Append rather than read-modify-write: concurrent `fix` runs would
    // otherwise clobber each other's entries. A single short line is written
    // atomically by every platform we target.
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    writeln!(file, "{ts}\t{}", repo.short_name())?;
    Ok(())
}

/// The pid keeps concurrent runs against the same repository in the same
/// second from colliding — in the work directory and in the branch name.
fn make_workdir(repo: &RepoRef) -> Result<PathBuf> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let slug = repo.short_name().replace('/', "-");
    let dir = std::env::temp_dir().join(format!("auto-oss-{slug}-{ts}-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Prefer the backend's title; fall back to truncated feedback. The scope
/// prefix stays either way — it is how auto-oss submissions read at a
/// glance in a PR list. The user's raw feedback always travels in the body.
fn pr_title(args: &FixArgs, backend_title: Option<&str>) -> String {
    let summary = match backend_title {
        Some(t) => truncate_chars(t.trim(), 80),
        None => truncate_chars(args.feedback.lines().next().unwrap_or("").trim(), 60),
    };
    format!("{}: {}", args.scope, summary)
}

fn truncate_chars(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let cut = s
        .char_indices()
        .take_while(|(i, _)| *i <= max_bytes - 3)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let mut out = s[..cut].to_string();
    out.push('…');
    out
}

fn submission_body(
    args: &FixArgs,
    backend_name: &str,
    model: Option<&str>,
    summary: Option<&str>,
    gate_results: &[(String, GateResult)],
    qualified: bool,
    diff: &str,
) -> String {
    let block = metadata::render_block(&Submission {
        scope: &args.scope,
        feedback: &args.feedback,
        reproduction: args.repro.as_deref(),
        backend: backend_name,
        model,
        gates: gate_results,
        human_reviewed: true,
    });
    let summary_section = summary
        .map(|s| format!("## What changed\n\n{s}\n\n"))
        .unwrap_or_default();
    let feedback_section = format!(
        "## Original feedback\n\n{}\n\n",
        args.feedback
            .lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if qualified {
        format!(
            "{summary_section}{feedback_section}This patch was generated from a user's \
             feedback under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss), following this \
             repository's `auto-oss.yml` policy. A human reviewed it before submission.\n\n{block}\n"
        )
    } else {
        format!(
            "{summary_section}{feedback_section}This report was collected under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss). A patch was attempted \
             but did not qualify for a pull request under this repository's policy; the \
             partial diff is attached for reference.\n\n{block}\n\n\
             <details><summary>Partial diff</summary>\n\n```diff\n{diff}\n```\n</details>\n"
        )
    }
}

/// Returns whether something was actually submitted.
fn submit_fallback(
    policy: &Policy,
    owner: &str,
    name: &str,
    title: &str,
    body_path: &Path,
) -> Result<bool> {
    match policy.fallback {
        Fallback::None => {
            eprintln!("==> submission did not qualify and fallback is `none`; stopping");
            Ok(false)
        }
        Fallback::Discussion => {
            eprintln!(
                "==> fallback `discussion` is not supported by this client yet; \
                 the prepared body is at {}",
                body_path.display()
            );
            Ok(false)
        }
        Fallback::Issue => {
            if !confirm("Patch did not qualify for a PR. Submit the report as an issue instead?")? {
                eprintln!("==> aborted; nothing submitted");
                return Ok(false);
            }
            let url = gh_out(&[
                "issue",
                "create",
                "--repo",
                &format!("{owner}/{name}"),
                "--title",
                title,
                "--body-file",
                &body_path.display().to_string(),
            ])?;
            let url = url.trim();
            eprintln!("{url}");
            if let Some(label) = &policy.metadata.label {
                // Best effort: the label may not exist in the target repository.
                let _ = gh(&["issue", "edit", url, "--add-label", label]);
            }
            Ok(true)
        }
    }
}

fn submit_pr(
    policy: &Policy,
    owner: &str,
    name: &str,
    args: &FixArgs,
    workdir: &Path,
    title: &str,
    body_path: &Path,
) -> Result<()> {
    let login = gh_out(&["api", "user", "-q", ".login"])?.trim().to_string();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let branch = format!("auto-oss/{}-{ts}-{}", args.scope, std::process::id());

    // With push access (own repo, collaborator) the branch goes straight to
    // upstream; a fork is only the outsider's route to a hosted branch.
    let can_push = gh_out(&[
        "api",
        &format!("repos/{owner}/{name}"),
        "-q",
        ".permissions.push",
    ])
    .map(|s| s.trim() == "true")
    .unwrap_or(false);
    let (push_repo, head) = if can_push {
        eprintln!("==> push access to {owner}/{name}: branching directly, no fork");
        (format!("{owner}/{name}"), branch.clone())
    } else {
        eprintln!("==> forking {owner}/{name} (no-op if the fork exists)");
        gh(&["repo", "fork", &format!("{owner}/{name}"), "--clone=false"])?;
        (format!("{login}/{name}"), format!("{login}:{branch}"))
    };

    git(workdir, &["checkout", "-b", &branch])?;
    git(workdir, &["commit", "--quiet", "-m", title])?;
    let push_url = format!("https://github.com/{push_repo}.git");
    eprintln!("==> pushing {branch} to {push_url}");
    git(
        workdir,
        &["push", &push_url, &format!("HEAD:refs/heads/{branch}")],
    )
    .context("push failed; if authentication failed, run `gh auth setup-git` once")?;

    gh(&[
        "pr",
        "create",
        "--repo",
        &format!("{owner}/{name}"),
        "--head",
        &head,
        "--title",
        title,
        "--body-file",
        &body_path.display().to_string(),
    ])?;

    if let Some(label) = &policy.metadata.label {
        // Best effort: the label may not exist in the target repository.
        let _ = gh(&[
            "pr",
            "edit",
            "--repo",
            &format!("{owner}/{name}"),
            &head,
            "--add-label",
            label,
        ]);
    }
    Ok(())
}

fn confirm(question: &str) -> Result<bool> {
    eprint!("{question} [y/N] ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}

fn diff_lines(diff: &str) -> u64 {
    diff.lines()
        .filter(|l| {
            (l.starts_with('+') && !l.starts_with("+++"))
                || (l.starts_with('-') && !l.starts_with("---"))
        })
        .count() as u64
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git").args(args).current_dir(dir).status()?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn git_out(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output()?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn gh(args: &[&str]) -> Result<()> {
    let status = Command::new("gh")
        .args(args)
        .status()
        .context("running gh")?;
    if !status.success() {
        bail!("gh {} failed", args.join(" "));
    }
    Ok(())
}

fn gh_out(args: &[&str]) -> Result<String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .context("running gh")?;
    if !out.status.success() {
        bail!(
            "gh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_diff_lines_excluding_headers() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n-old\n+new\n+added\n";
        assert_eq!(diff_lines(diff), 3);
    }

    #[test]
    fn truncates_multibyte_titles_safely() {
        let args = FixArgs {
            repo: String::new(),
            feedback: "あ".repeat(40),
            scope: "docs".into(),
            repro: None,
            backend: None,
            dry_run: true,
        };
        let title = pr_title(&args, None);
        assert!(title.starts_with("docs: あ"));
        assert!(title.ends_with('…'));
    }

    #[test]
    fn prefers_backend_title_over_feedback() {
        let args = FixArgs {
            repo: String::new(),
            feedback: "raw user words that make a poor title".into(),
            scope: "bug-fix".into(),
            repro: None,
            backend: None,
            dry_run: true,
        };
        assert_eq!(
            pr_title(&args, Some("Handle empty config without panicking")),
            "bug-fix: Handle empty config without panicking"
        );
    }
}
