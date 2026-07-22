use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::policy::{self, Policy, PolicyStatus, RepoRef};

/// The SPEC §3 metadata block, as read back from a submission body.
#[derive(Debug, Deserialize)]
struct MetaBlock {
    scope: String,
    feedback: String,
    #[serde(default)]
    reproduction: Option<String>,
    agent: AgentInfo,
    #[serde(default)]
    gates: BTreeMap<String, String>,
    human_reviewed: bool,
}

#[derive(Debug, Deserialize)]
struct AgentInfo {
    backend: String,
}

const BLOCK_START: &str = "<!-- auto-oss:v0";
const BLOCK_END: &str = "-->";

pub fn run(pr: &str) -> Result<()> {
    let (owner, repo, number) = parse_pr_ref(pr)?;
    let body = gh_out(&[
        "api",
        &format!("repos/{owner}/{repo}/pulls/{number}"),
        "-q",
        ".body // \"\"",
    ])?;

    let Some(raw_block) = extract_block(&body)? else {
        println!(
            "{owner}/{repo}#{number}: no auto-oss metadata block; not an auto-oss submission."
        );
        return Ok(());
    };
    let block: MetaBlock =
        serde_yaml::from_str(raw_block).context("metadata block is not valid YAML")?;

    let target = RepoRef::GitHub {
        owner: owner.clone(),
        repo: repo.clone(),
    };
    let policy = match policy::discover(&target)? {
        PolicyStatus::OptedIn { policy, .. } => policy,
        PolicyStatus::NotOptedIn | PolicyStatus::Unusable { .. } => bail!(
            "{owner}/{repo} carries an auto-oss submission but has no usable policy; \
             the submission violates SPEC §1"
        ),
    };
    let changed_lines = pr_changed_lines(&owner, &repo, number)?;

    let failures = check(&policy, &block, changed_lines);
    for f in &failures {
        println!("✗ {f}");
    }
    if failures.is_empty() {
        println!("✓ {owner}/{repo}#{number}: metadata block conforms to the policy");
        Ok(())
    } else {
        bail!("{} conformance failure(s)", failures.len());
    }
}

fn check(policy: &Policy, block: &MetaBlock, changed_lines: u64) -> Vec<String> {
    let mut failures = Vec::new();
    if !policy.accepts.scopes.iter().any(|s| s == &block.scope) {
        failures.push(format!(
            "scope `{}` is not accepted (policy allows: {})",
            block.scope,
            policy.accepts.scopes.join(", ")
        ));
    }
    if block.feedback.trim().is_empty() {
        failures.push("feedback is empty; provenance is mandatory".into());
    }
    if block.agent.backend.trim().is_empty() {
        failures.push("agent.backend is empty; disclosure is mandatory".into());
    }
    if policy.require.human_review && !block.human_reviewed {
        failures.push("policy requires human review but human_reviewed is not true".into());
    }
    if policy.require.reproduction
        && block.scope == "bug-fix"
        && block
            .reproduction
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
    {
        failures.push("policy requires reproduction steps for bug fixes".into());
    }
    if let Some(max) = policy.accepts.max_diff_lines {
        if changed_lines > max {
            failures.push(format!(
                "pull request changes {changed_lines} lines; policy maximum is {max}"
            ));
        }
    }
    for gate in policy.gates.keys() {
        match block.gates.get(gate).map(String::as_str) {
            None => failures.push(format!("declared gate `{gate}` is not reported")),
            Some("pass") => {}
            Some(other) => failures.push(format!(
                "gate `{gate}` reported `{other}`; a pull request requires every gate to pass"
            )),
        }
    }
    failures
}

fn pr_changed_lines(owner: &str, repo: &str, number: u64) -> Result<u64> {
    let raw = gh_out(&[
        "api",
        &format!("repos/{owner}/{repo}/pulls/{number}"),
        "-q",
        ".additions + .deletions",
    ])?;
    raw.trim().parse().with_context(|| {
        format!(
            "GitHub returned an invalid changed-line count `{}`",
            raw.trim()
        )
    })
}

/// Extract the YAML between the block markers. Exactly one block is allowed.
fn extract_block(body: &str) -> Result<Option<&str>> {
    let mut starts = body.match_indices(BLOCK_START);
    let Some((start, _)) = starts.next() else {
        return Ok(None);
    };
    if starts.next().is_some() {
        bail!("multiple auto-oss metadata blocks; exactly one is allowed");
    }
    let inner = &body[start + BLOCK_START.len()..];
    let Some(end) = inner.find(BLOCK_END) else {
        bail!("metadata block is not terminated");
    };
    Ok(Some(&inner[..end]))
}

fn parse_pr_ref(s: &str) -> Result<(String, String, u64)> {
    let rest = s.strip_prefix("https://github.com/").unwrap_or(s);
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    match parts.as_slice() {
        [owner, repo, "pull", number] => Ok((
            owner.to_string(),
            repo.to_string(),
            number
                .parse()
                .with_context(|| format!("invalid PR number `{number}`"))?,
        )),
        _ => bail!("cannot parse `{s}`: expected a GitHub pull request URL"),
    }
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

    fn test_policy() -> Policy {
        policy::parse(
            "version: 0\naccepts:\n  scopes: [bug-fix, docs]\ngates:\n  test: \"cargo test\"\nrequire:\n  reproduction: true\n",
        )
        .unwrap()
    }

    fn block(yaml: &str) -> MetaBlock {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn extracts_single_block() {
        let body = "intro\n<!-- auto-oss:v0\nscope: docs\n-->\noutro";
        assert_eq!(extract_block(body).unwrap(), Some("\nscope: docs\n"));
        assert_eq!(extract_block("no block here").unwrap(), None);
    }

    #[test]
    fn rejects_multiple_blocks() {
        let body = "<!-- auto-oss:v0\na: 1\n-->\n<!-- auto-oss:v0\nb: 2\n-->";
        assert!(extract_block(body).is_err());
    }

    #[test]
    fn conforming_block_passes() {
        let b = block(
            "scope: docs\nfeedback: |\n  text\nagent:\n  backend: claude-code\ngates:\n  test: pass\nhuman_reviewed: true\n",
        );
        assert!(check(&test_policy(), &b, 10).is_empty());
    }

    #[test]
    fn catches_violations() {
        let b = block(
            "scope: feature\nfeedback: \"\"\nagent:\n  backend: \"\"\ngates: {}\nhuman_reviewed: false\n",
        );
        let failures = check(&test_policy(), &b, 10);
        assert_eq!(
            failures.len(),
            5,
            "scope, feedback, backend, human_review, missing gate: {failures:?}"
        );
    }

    #[test]
    fn failing_gate_is_a_violation_on_prs() {
        let b = block(
            "scope: docs\nfeedback: |\n  text\nagent:\n  backend: human\ngates:\n  test: fail\nhuman_reviewed: true\n",
        );
        let failures = check(&test_policy(), &b, 10);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("gate `test`"));
    }

    #[test]
    fn oversized_pull_request_is_a_violation() {
        let policy =
            policy::parse("version: 0\naccepts:\n  scopes: [docs]\n  max_diff_lines: 20\n")
                .unwrap();
        let b =
            block("scope: docs\nfeedback: text\nagent:\n  backend: human\nhuman_reviewed: true\n");
        assert!(check(&policy, &b, 20).is_empty());
        let failures = check(&policy, &b, 21);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("21 lines"));
        assert!(failures[0].contains("maximum is 20"));
    }

    #[test]
    fn catches_empty_reproduction_string() {
        let b = block(
            "scope: bug-fix\nfeedback: text\nagent:\n  backend: human\nhuman_reviewed: true\nreproduction: \"\"\ngates:\n  test: pass\n",
        );
        let failures = check(&test_policy(), &b, 10);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("reproduction"));
    }

    #[test]
    fn catches_whitespace_only_reproduction() {
        let b = block(
            "scope: bug-fix\nfeedback: text\nagent:\n  backend: human\nhuman_reviewed: true\nreproduction: \"   \"\ngates:\n  test: pass\n",
        );
        let failures = check(&test_policy(), &b, 10);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("reproduction"));
    }

    #[test]
    fn parses_pr_urls() {
        let (o, r, n) = parse_pr_ref("https://github.com/q0tzly/auto-oss/pull/1").unwrap();
        assert_eq!((o.as_str(), r.as_str(), n), ("q0tzly", "auto-oss", 1));
        assert!(parse_pr_ref("https://github.com/q0tzly/auto-oss").is_err());
    }
}
