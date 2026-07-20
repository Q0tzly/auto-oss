mod backend;
mod fix;
mod gates;
mod init_cmd;
mod metadata;
mod policy;
mod status;
mod verify;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Turn user feedback into a policy-gated patch and submit it upstream
    Fix {
        /// Local path, owner/repo, or GitHub URL
        repo: String,
        /// The feedback, verbatim
        feedback: String,
        /// Change category; must be accepted by the repository's policy
        #[arg(long, default_value = "bug-fix")]
        scope: String,
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
    },
    /// Check a pull request's metadata block against its repository's policy
    Verify {
        /// GitHub pull request URL
        pr: String,
    },
    /// Show recent and in-progress fix runs
    Status,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Policy { repo } => show_policy(&repo),
        Cmd::Init { force } => init_cmd::run(force),
        Cmd::Verify { pr } => verify::run(&pr),
        Cmd::Status => status::run(),
        Cmd::Fix {
            repo,
            feedback,
            scope,
            repro,
            backend,
            dry_run,
        } => fix::run(fix::FixArgs {
            repo,
            feedback,
            scope,
            repro,
            backend,
            dry_run,
        }),
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
