use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::backend::{self, Backend};
use crate::gates::{self, GateResult};
use crate::metadata::{self, Submission};
use crate::policy::{self, Fallback, Policy, PolicyStatus, RepoRef};
use crate::status::{self, NewRun, RunTracker};

pub struct FixArgs {
    pub repo: String,
    pub feedback: String,
    pub scope: String,
    pub repro: Option<String>,
    pub backend: Option<String>,
    pub dry_run: bool,
}

fn discover_policy(repo: &RepoRef) -> Result<Policy> {
    match policy::discover(repo)? {
        PolicyStatus::OptedIn { policy, found_at } => {
            eprintln!("==> policy found at {found_at}");
            Ok(policy)
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
    }
}

pub fn run(args: FixArgs) -> Result<()> {
    let repo = RepoRef::parse(&args.repo)?;
    let policy = discover_policy(&repo)?;

    validate_request(&policy, &args)?;
    if !args.dry_run {
        enforce_limit(&policy, &repo)?;
    }
    let config = backend::load_config()?;
    let backend = backend::resolve(args.backend.as_deref(), &config)?;

    let workdir = make_workdir(&repo)?;
    let mut tracker = RunTracker::start(NewRun {
        repo: &repo.short_name(),
        repo_arg: &args.repo,
        workdir: &workdir,
        feedback: &args.feedback,
        scope: &args.scope,
        repro: args.repro.as_deref(),
        backend: args.backend.as_deref(),
        dry_run: args.dry_run,
    });
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

    // A backend failure is not a hard error: SPEC's fallback promise
    // ("when a patch cannot be produced... submitted as an issue") applies
    // here too, so it's routed through continue_after_generation exactly
    // like a no-op patch, an oversized diff, or a failing gate.
    let (generated, failure_reason) = match backend.generate(&workdir, &prompt) {
        Ok(g) => {
            tracker.set_generated(g.title.as_deref(), g.summary.as_deref());
            (g, None)
        }
        Err(e) => {
            tracker.set("failed");
            (
                backend::Generated::default(),
                Some(format!(
                    "the `{}` backend failed to produce a patch: {e}",
                    backend.name()
                )),
            )
        }
    };

    continue_after_generation(ContinueArgs {
        repo: &repo,
        policy: &policy,
        args: &args,
        backend: backend.as_ref(),
        workdir: &workdir,
        tracker: &mut tracker,
        generated,
        failure_reason,
    })
}

/// Pick a run back up after this process (or a previous `fix`/`resume`) was
/// interrupted before it reached a terminal phase. Everything through patch
/// generation is redone from what is already on disk in `workdir` rather
/// than re-run: gates are re-executed (they're expected to be idempotent,
/// and a prior run may have died mid-gate), but the backend is not called
/// again — its title/summary are restored from the tracked run, and the
/// diff is read fresh from the clone.
pub fn resume(workdir_arg: &str) -> Result<()> {
    let workdir = PathBuf::from(workdir_arg);
    let Some((path, state)) = status::find_run(&workdir)? else {
        bail!(
            "no tracked run found for {}; check `autos status` for the exact workdir",
            workdir.display()
        );
    };
    if status::is_terminal(&state.phase) {
        bail!(
            "the run at {} already finished ({}); nothing to resume",
            workdir.display(),
            state.phase
        );
    }
    if state.repo_arg.is_empty() {
        bail!(
            "the run at {} was tracked by an older version of autos and can't be resumed \
             automatically; the workdir is still there if you want to finish by hand",
            workdir.display()
        );
    }
    if !workdir.exists() {
        bail!(
            "work directory {} no longer exists; nothing to resume",
            workdir.display()
        );
    }

    let args = FixArgs {
        repo: state.repo_arg.clone(),
        feedback: state.feedback.clone(),
        scope: state.scope.clone(),
        repro: state.repro.clone(),
        backend: state.backend.clone(),
        dry_run: state.dry_run,
    };
    let repo = RepoRef::parse(&args.repo)?;
    // Re-validated against whatever the policy says now, not what it said
    // when the run started — a resume days later should see current rules.
    let policy = discover_policy(&repo)?;
    validate_request(&policy, &args)?;
    let config = backend::load_config()?;
    let backend = backend::resolve(args.backend.as_deref(), &config)?;
    let generated = backend::Generated {
        title: state.title.clone(),
        summary: state.summary.clone(),
    };
    let mut tracker = RunTracker::attach(path, state.clone());

    eprintln!(
        "==> resuming {} from phase `{}`",
        repo.short_name(),
        state.phase
    );
    eprintln!("    workdir: {}", workdir.display());

    continue_after_generation(ContinueArgs {
        repo: &repo,
        policy: &policy,
        args: &args,
        backend: backend.as_ref(),
        workdir: &workdir,
        tracker: &mut tracker,
        generated,
        failure_reason: None,
    })
}

struct ContinueArgs<'a> {
    repo: &'a RepoRef,
    policy: &'a Policy,
    args: &'a FixArgs,
    backend: &'a dyn Backend,
    workdir: &'a Path,
    tracker: &'a mut RunTracker,
    generated: backend::Generated,
    /// Some(reason) skips staging/diffing entirely — set when the backend
    /// itself already failed and there is nothing new to look at.
    failure_reason: Option<String>,
}

/// Everything from "the backend has had its turn" through submission.
/// Shared by a fresh `run()` and by `resume()`, which reconstructs the same
/// inputs from a previous run's tracked state instead of re-cloning and
/// re-generating.
fn continue_after_generation(ca: ContinueArgs) -> Result<()> {
    let ContinueArgs {
        repo,
        policy,
        args,
        backend,
        workdir,
        tracker,
        generated,
        mut failure_reason,
    } = ca;

    let mut diff = String::new();
    if failure_reason.is_none() {
        git(workdir, &["add", "-A"])?;
        diff = git_out(workdir, &["diff", "--cached"])?;
        if diff.trim().is_empty() {
            tracker.set("failed");
            failure_reason = Some("the backend made no changes to the repository".to_string());
        } else {
            eprintln!("\n{diff}");
        }
    }
    let changed = diff_lines(&diff);
    if failure_reason.is_none() {
        eprintln!("==> {changed} changed lines");
    }

    let oversized = failure_reason.is_none()
        && policy
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

    let gate_results = if failure_reason.is_some() || oversized {
        policy
            .gates
            .keys()
            .map(|k| (k.clone(), GateResult::Skipped))
            .collect()
    } else {
        if !policy.gates.is_empty() {
            tracker.set("awaiting-gate-approval");
            if !confirm_gate_execution(policy)? {
                tracker.set("aborted");
                eprintln!(
                    "==> aborted before gates; nothing submitted (workdir kept at {})",
                    workdir.display()
                );
                return Ok(());
            }
        }
        tracker.set("gates");
        gates::run_all(&policy.gates, workdir)?
    };
    let qualified = failure_reason.is_none() && !oversized && gates::all_pass(&gate_results);

    let title = pr_title(args, generated.title.as_deref());
    let body = submission_body(BodyInputs {
        args,
        backend_name: backend.name(),
        model: backend.model(),
        summary: generated.summary.as_deref(),
        gate_results: &gate_results,
        qualified,
        diff: &diff,
        failure_reason: failure_reason.as_deref(),
    });
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

    let RepoRef::GitHub { owner, repo: name } = repo else {
        tracker.set("dry-run-done");
        eprintln!("==> local repository: submission not applicable; review the diff in place");
        eprintln!("    workdir: {}", workdir.display());
        return Ok(());
    };

    if !qualified {
        tracker.set("awaiting-approval");
        if submit_fallback(policy, owner, name, &title, &body_path)? {
            tracker.set("submitted-issue");
            record_submission(repo)?;
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
    if let Err(e) = submit_pr(policy, owner, name, args, workdir, &title, &body_path) {
        tracker.set("failed");
        return Err(e);
    }
    tracker.set("submitted-pr");
    record_submission(repo)
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
    if policy.require.reproduction
        && args.scope == "bug-fix"
        && args
            .repro
            .as_deref()
            .is_none_or(|repro| repro.trim().is_empty())
    {
        bail!("this repository requires reproduction steps for bug fixes; pass --repro");
    }
    Ok(())
}

fn confirm_gate_execution(policy: &Policy) -> Result<bool> {
    if policy.gates.is_empty() {
        return Ok(true);
    }

    eprintln!("\n==> repository-controlled gates");
    for (name, command) in &policy.gates {
        eprintln!("    {name}: {command}");
    }
    eprintln!("    Every gate above runs as a shell command on this machine.");
    confirm("Run these gates?")
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

struct BodyInputs<'a> {
    args: &'a FixArgs,
    backend_name: &'a str,
    model: Option<&'a str>,
    summary: Option<&'a str>,
    gate_results: &'a [(String, GateResult)],
    qualified: bool,
    diff: &'a str,
    failure_reason: Option<&'a str>,
}

fn submission_body(inputs: BodyInputs) -> String {
    let BodyInputs {
        args,
        backend_name,
        model,
        summary,
        gate_results,
        qualified,
        diff,
        failure_reason,
    } = inputs;
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
    let client = format!("auto-oss v{}", env!("CARGO_PKG_VERSION"));
    if qualified {
        format!(
            "{summary_section}{feedback_section}This patch was generated from a user's \
             feedback under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss) by {client}, following \
             this repository's `auto-oss.yml` policy. A human reviewed it before \
             submission.\n\n{block}\n"
        )
    } else if let Some(reason) = failure_reason {
        format!(
            "{summary_section}{feedback_section}This report was collected under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss) by {client}. No patch \
             could be submitted as a pull request: {reason}.\n\n{block}\n"
        )
    } else {
        format!(
            "{summary_section}{feedback_section}This report was collected under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss) by {client}. A patch was \
             attempted but did not qualify for a pull request under this repository's policy; \
             the partial diff is attached for reference.\n\n{block}\n\n\
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
        Fallback::Discussion => submit_discussion(owner, name, title, body_path),
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

/// Submit a fallback report as a GitHub Discussion via the GraphQL API (the
/// REST API cannot create discussions). Picks a category by a fixed
/// preference order since the policy does not name one; falls back to the
/// first category the repository has. Ok(false) means nothing was created
/// (Discussions disabled, no categories, or the user declined) — the caller
/// treats that the same as a declined issue fallback.
fn submit_discussion(owner: &str, name: &str, title: &str, body_path: &Path) -> Result<bool> {
    const LOOKUP: &str = "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){id discussionCategories(first:25){nodes{id name}}}}";
    let raw = gh_out(&[
        "api",
        "graphql",
        "-f",
        &format!("query={LOOKUP}"),
        "-f",
        &format!("owner={owner}"),
        "-f",
        &format!("name={name}"),
    ])?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).context("parsing discussion category lookup")?;
    let Some(repo_id) = json["data"]["repository"]["id"].as_str() else {
        eprintln!(
            "==> could not resolve a repository id for {owner}/{name}; \
             falling back to discussions is unavailable"
        );
        return Ok(false);
    };
    let categories = json["data"]["repository"]["discussionCategories"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if categories.is_empty() {
        eprintln!(
            "==> {owner}/{name} has no discussion categories (Discussions may be disabled); \
             cannot file a fallback discussion. The prepared body is at {}",
            body_path.display()
        );
        return Ok(false);
    }
    let preferred = ["ideas", "feedback", "general", "q&a"];
    let category = preferred
        .iter()
        .find_map(|want| {
            categories.iter().find(|c| {
                c["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(want))
            })
        })
        .unwrap_or(&categories[0]);
    let category_id = category["id"].as_str().unwrap_or_default();
    let category_name = category["name"].as_str().unwrap_or("unknown");

    if !confirm(&format!(
        "Submit the report as a discussion in category `{category_name}` instead?"
    ))? {
        eprintln!("==> aborted; nothing submitted");
        return Ok(false);
    }

    const CREATE: &str = "mutation($repoId:ID!,$catId:ID!,$title:String!,$body:String!){createDiscussion(input:{repositoryId:$repoId,categoryId:$catId,title:$title,body:$body}){discussion{url}}}";
    let raw = gh_out(&[
        "api",
        "graphql",
        "-f",
        &format!("query={CREATE}"),
        "-f",
        &format!("repoId={repo_id}"),
        "-f",
        &format!("catId={category_id}"),
        "-f",
        &format!("title={title}"),
        "-F",
        &format!("body=@{}", body_path.display()),
    ])?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).context("parsing discussion creation response")?;
    let Some(url) = json["data"]["createDiscussion"]["discussion"]["url"].as_str() else {
        bail!("failed to create discussion: {raw}");
    };
    eprintln!("{url}");
    Ok(true)
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
    fn request_validation_rejects_empty_feedback_and_reproduction() {
        let policy = policy::parse(
            "version: 0\naccepts:\n  scopes: [bug-fix]\nrequire:\n  reproduction: true\n",
        )
        .unwrap();
        let mut args = FixArgs {
            repo: String::new(),
            feedback: "  ".into(),
            scope: "bug-fix".into(),
            repro: None,
            backend: None,
            dry_run: true,
        };
        assert!(validate_request(&policy, &args).is_err());

        args.feedback = "it fails".into();
        args.repro = Some("  ".into());
        assert!(validate_request(&policy, &args).is_err());
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
