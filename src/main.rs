mod backend;
mod fix;
mod gates;
mod init_cmd;
mod metadata;
mod policy;
mod status;
mod verify;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use policy::{PolicyStatus, RepoRef};

#[derive(Parser)]
#[command(
    name = "autos",
    version,
    about = "Client for the auto-oss protocol: user-side agent contributions to opted-in repositories"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Arguments shared by every scope-specific submission subcommand
/// (`fix`, `feat`, `docs`, `refactor`, `test`, `typo`).
#[derive(Args)]
struct FixCommon {
    /// Local path, owner/repo, or GitHub URL
    repo: String,
    /// The feedback, verbatim
    feedback: String,
    /// Reproduction steps (required by some policies for bug fixes)
    #[arg(long)]
    repro: Option<String>,
    /// Backend that produces the patch: claude-code, `human` (you edit
    /// the workdir yourself), or a custom backend from
    /// ~/.auto-oss/config.yml. Defaults to the config's
    /// `default_backend`, else claude-code.
    #[arg(long)]
    backend: Option<String>,
    /// Stop after generating the patch and running gates; submit nothing
    #[arg(long)]
    dry_run: bool,
}

impl FixCommon {
    fn into_args(self, scope: impl Into<String>) -> fix::FixArgs {
        fix::FixArgs {
            repo: self.repo,
            feedback: self.feedback,
            scope: scope.into(),
            repro: self.repro,
            backend: self.backend,
            dry_run: self.dry_run,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Show a repository's auto-oss acceptance policy
    Policy {
        /// Local path, owner/repo, or GitHub URL
        repo: String,
    },
    /// Generate an auto-oss.yml for this repository (maintainer side)
    Init {
        /// Overwrite an existing auto-oss.yml
        #[arg(long)]
        force: bool,
    },
    /// Fix a bug (scope: bug-fix). For a scope your repository declares
    /// that isn't one of autos's short subcommands, pass --scope directly.
    Fix {
        #[command(flatten)]
        common: FixCommon,
        /// Change category; must be accepted by the repository's policy.
        /// The `fix`/`feat`/`docs`/`refactor`/`test`/`typo` subcommands are
        /// shortcuts for this; use this flag for anything else.
        #[arg(long, default_value = "bug-fix")]
        scope: String,
    },
    /// Propose a feature or enhancement (scope: feature)
    Feat {
        #[command(flatten)]
        common: FixCommon,
    },
    /// Fix or improve documentation (scope: docs)
    Docs {
        #[command(flatten)]
        common: FixCommon,
    },
    /// Propose a refactor (scope: refactor)
    Refactor {
        #[command(flatten)]
        common: FixCommon,
    },
    /// Add or fix a test (scope: test)
    Test {
        #[command(flatten)]
        common: FixCommon,
    },
    /// Fix a typo (scope: typo)
    Typo {
        #[command(flatten)]
        common: FixCommon,
    },
    /// Check a pull request's metadata block against its repository's policy
    Verify {
        /// GitHub pull request URL
        pr: String,
    },
    /// Show recent and in-progress fix runs
    Status,
    /// Pick a `fix` run back up after it was interrupted (Ctrl-C, closed
    /// terminal, ...) before reaching a terminal phase. Find the work
    /// directory with `autos status`.
    Resume {
        /// The interrupted run's work directory, as shown by `autos status`
        workdir: String,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Policy { repo } => show_policy(&repo),
        Cmd::Init { force } => init_cmd::run(force),
        Cmd::Verify { pr } => verify::run(&pr),
        Cmd::Status => status::run(),
        Cmd::Resume { workdir } => fix::resume(&workdir),
        Cmd::Fix { common, scope } => fix::run(common.into_args(scope)),
        Cmd::Feat { common } => fix::run(common.into_args("feature")),
        Cmd::Docs { common } => fix::run(common.into_args("docs")),
        Cmd::Refactor { common } => fix::run(common.into_args("refactor")),
        Cmd::Test { common } => fix::run(common.into_args("test")),
        Cmd::Typo { common } => fix::run(common.into_args("typo")),
    }
}

fn show_policy(repo: &str) -> Result<()> {
    let repo = RepoRef::parse(repo)?;
    match policy::discover(&repo)? {
        PolicyStatus::NotOptedIn => {
            println!(
                "{}: not opted in to auto-oss (no policy file).\n\
                 Agent-generated pull requests must not be submitted to this repository.",
                repo.short_name()
            );
        }
        PolicyStatus::Unusable { found_at, reason } => {
            println!(
                "{}: policy file `{found_at}` exists but is unusable: {reason}\n\
                 Per SPEC this counts as not opted in.",
                repo.short_name()
            );
        }
        PolicyStatus::OptedIn { policy, found_at } => {
            println!(
                "{}: opted in via `{found_at}` (spec v{})",
                repo.short_name(),
                policy.version
            );
            println!("  scopes:        {}", policy.accepts.scopes.join(", "));
            if let Some(max) = policy.accepts.max_diff_lines {
                println!("  max diff:      {max} lines");
            }
            if policy.gates.is_empty() {
                println!("  gates:         none declared");
            } else {
                for (name, cmd) in &policy.gates {
                    println!("  gate {name}: {cmd}");
                }
            }
            println!("  human review:  {}", policy.require.human_review);
            println!("  reproduction:  {}", policy.require.reproduction);
            println!("  fallback:      {}", policy.fallback);
            if let Some(n) = policy.limits.per_author_per_week {
                println!("  limit:         {n} submissions per author per week");
            }
            if let Some(label) = &policy.metadata.label {
                println!("  label:         {label}");
            }
            if let Some(lang) = &policy.metadata.language {
                println!("  language:      {lang} (titles and summaries)");
            }
        }
    }
    Ok(())
}
