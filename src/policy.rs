use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const SUPPORTED_VERSION: u64 = 0;
pub const POLICY_PATHS: [&str; 2] = ["auto-oss.yml", ".github/auto-oss.yml"];

#[derive(Debug, Deserialize)]
pub struct Policy {
    pub version: u64,
    pub accepts: Accepts,
    #[serde(default)]
    pub gates: BTreeMap<String, String>,
    #[serde(default)]
    pub require: Require,
    #[serde(default)]
    pub fallback: Fallback,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub metadata: MetadataCfg,
}

#[derive(Debug, Deserialize)]
pub struct Accepts {
    pub scopes: Vec<String>,
    pub max_diff_lines: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Require {
    pub human_review: bool,
    pub reproduction: bool,
}

impl Default for Require {
    fn default() -> Self {
        Self {
            human_review: true,
            reproduction: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fallback {
    #[default]
    Issue,
    Discussion,
    None,
}

impl fmt::Display for Fallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fallback::Issue => write!(f, "issue"),
            Fallback::Discussion => write!(f, "discussion"),
            Fallback::None => write!(f, "none"),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub per_author_per_week: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MetadataCfg {
    pub label: Option<String>,
    /// Language the client SHOULD use for submission titles and summaries
    /// (e.g. "en"). The feedback itself always stays verbatim.
    pub language: Option<String>,
}

/// Where a target repository lives.
#[derive(Debug, Clone)]
pub enum RepoRef {
    Local(PathBuf),
    GitHub { owner: String, repo: String },
}

impl RepoRef {
    pub fn parse(s: &str) -> Result<Self> {
        let path = Path::new(s);
        if path.exists() {
            return Ok(RepoRef::Local(path.canonicalize()?));
        }
        let rest = s
            .strip_prefix("https://github.com/")
            .or_else(|| s.strip_prefix("git@github.com:"))
            .unwrap_or(s);
        let rest = rest.trim_end_matches(".git").trim_end_matches('/');
        let parts: Vec<&str> = rest.split('/').collect();
        match parts.as_slice() {
            [owner, repo] if !owner.is_empty() && !repo.is_empty() => Ok(RepoRef::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
            }),
            _ => bail!(
                "cannot resolve `{s}`: not an existing local path, and not owner/repo or a GitHub URL"
            ),
        }
    }

    pub fn clone_url(&self) -> String {
        match self {
            RepoRef::Local(p) => p.display().to_string(),
            RepoRef::GitHub { owner, repo } => format!("https://github.com/{owner}/{repo}.git"),
        }
    }

    pub fn short_name(&self) -> String {
        match self {
            RepoRef::Local(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "local".into()),
            RepoRef::GitHub { owner, repo } => format!("{owner}/{repo}"),
        }
    }
}

/// Outcome of policy discovery, per SPEC §1 and §2.
pub enum PolicyStatus {
    /// Policy found and parsed; found_at is the path that matched.
    OptedIn { policy: Policy, found_at: String },
    /// No policy file at any discovery path.
    NotOptedIn,
    /// A policy file exists but is unusable (parse error or unsupported
    /// version). Per SPEC this is equivalent to no opt-in, but the reason is
    /// worth surfacing to the user.
    Unusable { found_at: String, reason: String },
}

pub fn discover(repo: &RepoRef) -> Result<PolicyStatus> {
    for rel in POLICY_PATHS {
        let raw = match repo {
            RepoRef::Local(dir) => {
                let p = dir.join(rel);
                if !p.exists() {
                    continue;
                }
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?
            }
            RepoRef::GitHub { owner, repo } => {
                let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/{rel}");
                let fetched = fetch(&url).with_context(|| {
                    format!(
                        "could not reach GitHub to read {owner}/{repo}'s auto-oss policy; \
                         check your network connection and that the repository name is correct"
                    )
                })?;
                match fetched {
                    Some(body) => body,
                    None => continue,
                }
            }
        };
        return Ok(match parse(&raw) {
            Ok(policy) => PolicyStatus::OptedIn {
                policy,
                found_at: rel.to_string(),
            },
            Err(e) => PolicyStatus::Unusable {
                found_at: rel.to_string(),
                reason: e.to_string(),
            },
        });
    }
    Ok(PolicyStatus::NotOptedIn)
}

pub fn parse(raw: &str) -> Result<Policy> {
    let policy: Policy = serde_yaml::from_str(raw).context("invalid YAML")?;
    if policy.version > SUPPORTED_VERSION {
        bail!(
            "policy declares spec version {} but this client supports up to {}",
            policy.version,
            SUPPORTED_VERSION
        );
    }
    if policy.accepts.scopes.is_empty() {
        bail!("accepts.scopes must list at least one scope");
    }
    Ok(policy)
}

/// Fetch a URL. Ok(None) means the server answered "no such file" (curl -f
/// exits 22 on HTTP errors); network failures are Err — an unreachable
/// repository must not be mistaken for one that has not opted in.
fn fetch(url: &str) -> Result<Option<String>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "15", url])
        .output()
        .context("running curl (is it installed?)")?;
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    } else if out.status.code() == Some(22) {
        Ok(None)
    } else {
        bail!(
            "fetching {url} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_policy_with_defaults() {
        let p = parse("version: 0\naccepts:\n  scopes: [bug-fix]\n").unwrap();
        assert_eq!(p.version, 0);
        assert_eq!(p.accepts.scopes, vec!["bug-fix"]);
        assert!(p.require.human_review, "human_review defaults to true");
        assert!(!p.require.reproduction);
        assert_eq!(p.fallback, Fallback::Issue);
        assert!(p.gates.is_empty());
    }

    #[test]
    fn parses_full_policy() {
        let raw = r#"
version: 0
accepts:
  scopes: [bug-fix, docs, typo]
  max_diff_lines: 300
gates:
  build: "cargo build"
  test: "cargo test"
require:
  human_review: true
  reproduction: true
fallback: none
limits:
  per_author_per_week: 3
metadata:
  label: "auto-oss"
  language: "en"
"#;
        let p = parse(raw).unwrap();
        assert_eq!(p.accepts.max_diff_lines, Some(300));
        assert_eq!(p.gates.len(), 2);
        assert!(p.require.reproduction);
        assert_eq!(p.fallback, Fallback::None);
        assert_eq!(p.limits.per_author_per_week, Some(3));
        assert_eq!(p.metadata.label.as_deref(), Some("auto-oss"));
    }

    #[test]
    fn ignores_unknown_fields() {
        let p = parse("version: 0\nfuture_field: 1\naccepts:\n  scopes: [docs]\n");
        assert!(
            p.is_ok(),
            "unknown fields must be ignored for forward compatibility"
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        assert!(parse("version: 99\naccepts:\n  scopes: [docs]\n").is_err());
    }

    #[test]
    fn rejects_empty_scopes() {
        assert!(parse("version: 0\naccepts:\n  scopes: []\n").is_err());
    }

    #[test]
    fn parses_repo_refs() {
        for s in [
            "owner/name",
            "https://github.com/owner/name",
            "git@github.com:owner/name.git",
        ] {
            match RepoRef::parse(s).unwrap() {
                RepoRef::GitHub { owner, repo } => {
                    assert_eq!(owner, "owner");
                    assert_eq!(repo, "name");
                }
                other => panic!("expected GitHub ref for {s}, got {other:?}"),
            }
        }
        assert!(RepoRef::parse("not-a-repo").is_err());
    }
}
