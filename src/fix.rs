use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::backend;
use crate::gates::{self, GateResult};
use crate::metadata::{self, Submission};
use crate::policy::{self, Fallback, Policy, PolicyStatus, RepoRef};

pub struct FixArgs {
    pub repo: String,
    pub feedback: String,
    pub scope: String,
    pub repro: Option<String>,
    pub backend: String,
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
    let backend = backend::by_name(&args.backend)?;

    let workdir = make_workdir(&repo)?;
    eprintln!("==> cloning {} into {}", repo.short_name(), workdir.display());
    git(&workdir, &["clone", "--quiet", &repo.clone_url(), "."])?;

    eprintln!("==> generating patch with {}", backend.name());
    let prompt = backend::build_prompt(
        &args.feedback,
        args.repro.as_deref(),
        &args.scope,
        policy.accepts.max_diff_lines,
    );
    backend.generate(&workdir, &prompt)?;

    git(&workdir, &["add", "-A"])?;
    let diff = git_out(&workdir, &["diff", "--cached"])?;
    if diff.trim().is_empty() {
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
        policy.gates.keys().map(|k| (k.clone(), GateResult::Skipped)).collect()
    } else {
        gates::run_all(&policy.gates, &workdir)?
    };
    let qualified = !oversized && gates::all_pass(&gate_results);

    let title = pr_title(&args);
    let body = submission_body(&args, &gate_results, qualified, &diff);
    let body_path = workdir.join(".auto-oss-body.md");
    std::fs::write(&body_path, &body)?;

    eprintln!("\n----- submission preview -----");
    eprintln!("title: {title}");
    eprintln!("{body}");
    eprintln!("------------------------------");

    if args.dry_run {
        eprintln!("==> dry run: stopping before submission");
        eprintln!("    workdir: {}", workdir.display());
        eprintln!("    body:    {}", body_path.display());
        return Ok(());
    }

    let RepoRef::GitHub { owner, repo: name } = &repo else {
        eprintln!("==> local repository: submission not applicable; review the diff in place");
        eprintln!("    workdir: {}", workdir.display());
        return Ok(());
    };

    if !qualified {
        return submit_fallback(&policy, owner, name, &title, &body_path);
    }

    if !confirm("Review the diff and preview above. Submit this pull request?")? {
        eprintln!("==> aborted; nothing submitted (workdir kept at {})", workdir.display());
        return Ok(());
    }
    submit_pr(&policy, owner, name, &args, &workdir, &title, &body_path)
}

fn validate_request(policy: &Policy, args: &FixArgs) -> Result<()> {
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

fn make_workdir(repo: &RepoRef) -> Result<PathBuf> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let slug = repo.short_name().replace('/', "-");
    let dir = std::env::temp_dir().join(format!("auto-oss-{slug}-{ts}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn pr_title(args: &FixArgs) -> String {
    let mut summary = args.feedback.lines().next().unwrap_or("").trim().to_string();
    if summary.len() > 60 {
        let cut = summary
            .char_indices()
            .take_while(|(i, _)| *i <= 57)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        summary.truncate(cut);
        summary.push('…');
    }
    format!("{}: {}", args.scope, summary)
}

fn submission_body(
    args: &FixArgs,
    gate_results: &[(String, GateResult)],
    qualified: bool,
    diff: &str,
) -> String {
    let block = metadata::render_block(&Submission {
        scope: &args.scope,
        feedback: &args.feedback,
        reproduction: args.repro.as_deref(),
        backend: &args.backend,
        gates: gate_results,
        human_reviewed: true,
    });
    if qualified {
        format!(
            "This patch was generated from a user's feedback under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss), following this \
             repository's `auto-oss.yml` policy. A human reviewed it before submission.\n\n{block}\n"
        )
    } else {
        format!(
            "This report was collected under the \
             [auto-oss protocol](https://github.com/q0tzly/auto-oss). A patch was attempted \
             but did not qualify for a pull request under this repository's policy; the \
             partial diff is attached for reference.\n\n{block}\n\n\
             <details><summary>Partial diff</summary>\n\n```diff\n{diff}\n```\n</details>\n"
        )
    }
}

fn submit_fallback(
    policy: &Policy,
    owner: &str,
    name: &str,
    title: &str,
    body_path: &Path,
) -> Result<()> {
    match policy.fallback {
        Fallback::None => {
            eprintln!("==> submission did not qualify and fallback is `none`; stopping");
            Ok(())
        }
        Fallback::Discussion => {
            eprintln!(
                "==> fallback `discussion` is not supported by this client yet; \
                 the prepared body is at {}",
                body_path.display()
            );
            Ok(())
        }
        Fallback::Issue => {
            if !confirm("Patch did not qualify for a PR. Submit the report as an issue instead?")? {
                eprintln!("==> aborted; nothing submitted");
                return Ok(());
            }
            let mut cmd_args = vec![
                "issue".to_string(),
                "create".to_string(),
                "--repo".to_string(),
                format!("{owner}/{name}"),
                "--title".to_string(),
                title.to_string(),
                "--body-file".to_string(),
                body_path.display().to_string(),
            ];
            if let Some(label) = &policy.metadata.label {
                cmd_args.extend(["--label".to_string(), label.clone()]);
            }
            gh(&cmd_args.iter().map(String::as_str).collect::<Vec<_>>())
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
    let branch = format!("auto-oss/{}-{ts}", args.scope);

    eprintln!("==> forking {owner}/{name} (no-op if the fork exists)");
    gh(&["repo", "fork", &format!("{owner}/{name}"), "--clone=false"])?;

    git(workdir, &["checkout", "-b", &branch])?;
    git(workdir, &["commit", "--quiet", "-m", title])?;
    let push_url = format!("https://github.com/{login}/{name}.git");
    eprintln!("==> pushing {branch} to {push_url}");
    git(workdir, &["push", &push_url, &format!("HEAD:refs/heads/{branch}")])
        .context("push failed; if authentication failed, run `gh auth setup-git` once")?;

    gh(&[
        "pr",
        "create",
        "--repo",
        &format!("{owner}/{name}"),
        "--head",
        &format!("{login}:{branch}"),
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
            &format!("{login}:{branch}"),
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
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn gh(args: &[&str]) -> Result<()> {
    let status = Command::new("gh").args(args).status().context("running gh")?;
    if !status.success() {
        bail!("gh {} failed", args.join(" "));
    }
    Ok(())
}

fn gh_out(args: &[&str]) -> Result<String> {
    let out = Command::new("gh").args(args).output().context("running gh")?;
    if !out.status.success() {
        bail!("gh {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
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
            backend: "claude-code".into(),
            dry_run: true,
        };
        let title = pr_title(&args);
        assert!(title.starts_with("docs: あ"));
        assert!(title.ends_with('…'));
    }
}
