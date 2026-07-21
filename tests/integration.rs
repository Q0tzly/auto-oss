use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    temp: PathBuf,
}

impl Fixture {
    fn new(policy: Option<&str>) -> Self {
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "auto-oss-integration-{}-{nanos}-{nonce}",
            std::process::id()
        ));
        let home = root.join("home");
        let repo = root.join("target");
        let temp = root.join("tmp");
        fs::create_dir_all(home.join(".auto-oss")).unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&temp).unwrap();

        fs::write(
            home.join(".auto-oss/config.yml"),
            r#"default_backend: fixture
backends:
  fixture:
    command:
      - sh
      - -c
      - "sed -i.bak 's/original/changed by fixture/' README.md && rm README.md.bak"
      - "{prompt}"
"#,
        )
        .unwrap();

        git(&repo, &["init", "--quiet"]);
        git(&repo, &["config", "user.email", "fixture@example.com"]);
        git(&repo, &["config", "user.name", "Integration Fixture"]);
        fs::write(repo.join("README.md"), "original\n").unwrap();
        if let Some(policy) = policy {
            fs::write(repo.join("auto-oss.yml"), policy).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "fixture"]);

        Self {
            root,
            home,
            repo,
            temp,
        }
    }

    fn autos(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_autos"))
            .args(args)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.temp)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn autos_with_stdin(&self, args: &[&str], input: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_autos"))
            .args(args)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.temp)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn repo_arg(&self) -> &str {
        self.repo.to_str().unwrap()
    }

    fn write_submission(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        fs::write(
            self.home.join(".auto-oss/submissions.tsv"),
            format!("{now}\ttarget\n"),
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed:\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed:\nstdout:\n{}\nstderr:\n{}",
        text(&output.stdout),
        text(&output.stderr)
    );
}

fn policy(extra: &str) -> String {
    format!("version: 0\naccepts:\n  scopes: [docs]\n  max_diff_lines: 20\n{extra}")
}

#[test]
fn policy_reports_opted_in() {
    let fixture = Fixture::new(Some(&policy("gates:\n  test: \"true\"\n")));

    let output = fixture.autos(&["policy", fixture.repo_arg()]);

    assert_success(&output);
    let stdout = text(&output.stdout);
    assert!(stdout.contains("opted in via `auto-oss.yml`"), "{stdout}");
    assert!(stdout.contains("gate test: true"), "{stdout}");
}

#[test]
fn policy_reports_not_opted_in() {
    let fixture = Fixture::new(None);

    let output = fixture.autos(&["policy", fixture.repo_arg()]);

    assert_success(&output);
    let stdout = text(&output.stdout);
    assert!(stdout.contains("not opted in to auto-oss"), "{stdout}");
}

#[test]
fn policy_reports_unusable_policy() {
    let fixture = Fixture::new(Some("version: [not-a-number\n"));

    let output = fixture.autos(&["policy", fixture.repo_arg()]);

    assert_success(&output);
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("policy file `auto-oss.yml` exists but is unusable"),
        "{stdout}"
    );
    assert!(stdout.contains("counts as not opted in"), "{stdout}");
}

#[test]
fn fix_dry_run_uses_custom_backend_without_network_or_claude() {
    let fixture = Fixture::new(Some(&policy("gates:\n  test: \"true\"\n")));

    let output = fixture.autos_with_stdin(
        &[
            "fix",
            fixture.repo_arg(),
            "update the fixture",
            "--scope",
            "docs",
            "--dry-run",
        ],
        "y\n",
    );

    assert_success(&output);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("generating patch with fixture"), "{stderr}");
    assert!(stderr.contains("gate `test`: pass"), "{stderr}");
    assert!(stderr.contains("backend: fixture"), "{stderr}");
    assert!(
        stderr.contains("dry run: stopping before submission"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(fixture.repo.join("README.md")).unwrap(),
        "original\n"
    );
}

#[test]
fn fix_rejects_a_scope_outside_the_policy() {
    let fixture = Fixture::new(Some(&policy("")));

    let output = fixture.autos(&[
        "fix",
        fixture.repo_arg(),
        "add a feature",
        "--scope",
        "feature",
        "--dry-run",
    ]);

    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("scope `feature` is not accepted"),
        "{stderr}"
    );
    assert!(!stderr.contains("generating patch"), "{stderr}");
}

#[test]
fn fix_rejects_empty_feedback() {
    let fixture = Fixture::new(Some(&policy("")));

    let output = fixture.autos(&[
        "fix",
        fixture.repo_arg(),
        "",
        "--scope",
        "docs",
        "--dry-run",
    ]);

    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(stderr.contains("feedback must not be empty"), "{stderr}");
    assert!(!stderr.contains("generating patch"), "{stderr}");
}

#[test]
fn fix_enforces_weekly_limit_from_fake_home() {
    let fixture = Fixture::new(Some(&policy("limits:\n  per_author_per_week: 1\n")));
    fixture.write_submission();

    let output = fixture.autos(&[
        "fix",
        fixture.repo_arg(),
        "update the fixture",
        "--scope",
        "docs",
    ]);

    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("declares a limit of 1 submission"),
        "{stderr}"
    );
    assert!(
        stderr.contains("you have made 1 in the last 7 days"),
        "{stderr}"
    );
    assert!(!stderr.contains("generating patch"), "{stderr}");
}

#[test]
fn docs_subcommand_sets_scope_without_a_flag() {
    let fixture = Fixture::new(Some(&policy("gates:\n  test: \"true\"\n")));

    let output = fixture.autos_with_stdin(
        &[
            "docs",
            fixture.repo_arg(),
            "update the fixture",
            "--dry-run",
        ],
        "y\n",
    );

    assert_success(&output);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("scope: docs"), "{stderr}");
    assert!(
        stderr.contains("dry run: stopping before submission"),
        "{stderr}"
    );
}

#[test]
fn feat_subcommand_rejects_a_policy_that_only_accepts_docs() {
    let fixture = Fixture::new(Some(&policy("")));

    let output = fixture.autos(&["feat", fixture.repo_arg(), "add a feature", "--dry-run"]);

    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("scope `feature` is not accepted"),
        "{stderr}"
    );
}

#[test]
fn resume_continues_an_interrupted_run_from_saved_state() {
    let fixture = Fixture::new(Some(&policy("gates:\n  test: \"true\"\n")));

    // Simulate a run that already generated and staged a patch, then died
    // (Ctrl-C, crash) before the gate-confirmation prompt was answered —
    // the same state a killed `fix`/`feat`/... leaves behind.
    fs::write(fixture.repo.join("README.md"), "changed by a prior run\n").unwrap();
    git(&fixture.repo, &["add", "-A"]);

    let workdir = fixture.repo.to_str().unwrap();
    let runs_dir = fixture.home.join(".auto-oss/runs");
    fs::create_dir_all(&runs_dir).unwrap();
    fs::write(
        runs_dir.join("interrupted.json"),
        format!(
            r#"{{"repo":"target","workdir":{workdir:?},"phase":"awaiting-gate-approval",
"started":1,"updated":1,"repo_arg":{workdir:?},"feedback":"resumed feedback",
"scope":"docs","repro":null,"backend":null,"dry_run":true,"title":null,"summary":null}}"#
        ),
    )
    .unwrap();

    let output = fixture.autos_with_stdin(&["resume", workdir], "y\n");

    assert_success(&output);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("resuming target from phase"), "{stderr}");
    assert!(stderr.contains("gate `test`: pass"), "{stderr}");
    assert!(
        stderr.contains("dry run: stopping before submission"),
        "{stderr}"
    );

    // Resuming a now-terminal run must refuse rather than redo it.
    let second = fixture.autos(&["resume", workdir]);
    assert!(!second.status.success());
    assert!(
        text(&second.stderr).contains("already finished"),
        "{}",
        text(&second.stderr)
    );
}

#[test]
fn fix_stops_before_submission_when_a_gate_fails_for_a_local_repo() {
    let fixture = Fixture::new(Some(&policy("gates:\n  test: \"false\"\n")));

    let output = fixture.autos_with_stdin(
        &[
            "fix",
            fixture.repo_arg(),
            "update the fixture",
            "--scope",
            "docs",
        ],
        "y\n",
    );

    assert_success(&output);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("gate `test`: fail"), "{stderr}");
    assert!(
        stderr.contains("local repository: submission not applicable"),
        "{stderr}"
    );
    assert!(!stderr.contains("Submit this pull request?"), "{stderr}");
    assert_eq!(
        fs::read_to_string(fixture.repo.join("README.md")).unwrap(),
        "original\n"
    );
}
