use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn smoke_command_runs_fake_agent_and_reports_completed_signal() {
    let repo = TempRepo::new();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "smoke",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        output.status.success(),
        "ralphterm smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Smoke:"), "{stdout}");
    assert!(stdout.contains("Signal: COMPLETED"), "{stdout}");
    assert!(stdout.contains("COMPLETED"), "{stdout}");
}

#[test]
fn smoke_command_rejects_one_shot_print_mode() {
    let repo = TempRepo::new();
    let command = format!(
        "{} --print",
        fixture_path("fake-agent.sh")
            .to_str()
            .expect("utf8 fixture path")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args(["smoke", "--agent-command", &command])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly accepted one-shot mode\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("one-shot prompt mode"),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("interactive PTY"), "{diagnostics}");
}

#[test]
fn smoke_command_missing_completed_reports_agent_transcript_for_diagnostics() {
    let repo = TempRepo::new();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "smoke",
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = format!("{stdout}\n{stderr}");
    assert!(diagnostics.contains("Smoke:"), "{diagnostics}");
    assert!(diagnostics.contains("NOPE"), "{diagnostics}");
    assert!(diagnostics.contains("Signal: NONE"), "{diagnostics}");
}

#[test]
fn smoke_command_hanging_agent_exits_nonzero_with_bounded_timeout() {
    let repo = TempRepo::new();
    let start = Instant::now();

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_SMOKE_TIMEOUT_MS", "250")
        .args([
            "smoke",
            "--agent-command",
            fixture_path("hanging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm smoke");

    let elapsed = start.elapsed();
    assert!(
        !output.status.success(),
        "ralphterm smoke unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "smoke timeout was not bounded: elapsed={elapsed:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = format!("{stdout}\n{stderr}");
    assert!(diagnostics.contains("timed out"), "{diagnostics}");
    assert!(
        diagnostics.contains("still waiting for external input"),
        "{diagnostics}"
    );
}

#[test]
fn run_command_records_complete_transcript_for_successful_large_output_agent() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let mut command = Command::new(env!("CARGO_BIN_EXE_ralphterm"));
    command
        .current_dir(&repo.path)
        .env("RALPHTERM_AGENT_TIMEOUT_MS", "30000")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("large-output-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null());
    let output = run_with_test_timeout(command, Duration::from_secs(15));

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let transcript =
        fs::read_to_string(repo.path.join(".ralphterm/progress/plan-task-1.transcript"))
            .expect("read transcript");
    assert!(
        transcript.contains("large-output-line-200000"),
        "transcript should include final large-output line; len={} tail={}",
        transcript.len(),
        transcript
            .chars()
            .rev()
            .take(400)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
    assert!(
        transcript.contains("LARGE_OUTPUT_SENTINEL_COMPLETED"),
        "transcript should include sentinel; len={}",
        transcript.len()
    );
}

#[test]
fn run_command_hanging_agent_exits_nonzero_with_bounded_timeout() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let start = Instant::now();

    let mut command = Command::new(env!("CARGO_BIN_EXE_ralphterm"));
    command
        .current_dir(&repo.path)
        .env("RALPHTERM_AGENT_TIMEOUT_MS", "250")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("hanging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    let output = run_with_test_timeout(command, Duration::from_secs(5));

    let elapsed = start.elapsed();
    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "run timeout was not bounded: elapsed={elapsed:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = format!("{stdout}\n{stderr}");
    assert!(diagnostics.contains("timed out"), "{diagnostics}");
    assert!(
        diagnostics.contains("still waiting for external input"),
        "{diagnostics}"
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );
    let transcript_path = ".ralphterm/progress/plan-task-1.transcript";
    let transcript = fs::read_to_string(repo.path.join(transcript_path)).expect("read transcript");
    assert!(
        transcript.contains("still waiting for external input"),
        "{transcript}"
    );
    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: agent execution"), "{summary}");
    assert!(summary.contains("timed out"), "{summary}");
    assert!(summary.contains(transcript_path), "{summary}");
}

#[test]
fn run_command_with_workspace_id_runs_inside_managed_workspace() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    let original_plan = r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
- [ ] Verify first.txt exists
"#;
    fs::write(&plan_path, original_plan).expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            "plan.md",
            "--workspace-id",
            "isolated",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let workspace_path = repo.path.join(".ralphterm/workspaces/isolated");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("Workspace: {}", workspace_path.display())),
        "{stdout}"
    );

    let source_plan = fs::read_to_string(&plan_path).expect("read source plan");
    assert_eq!(source_plan, original_plan);
    assert!(
        !repo.path.join("first.txt").exists(),
        "source repo must not receive generated file"
    );
    assert_eq!(repo.git_output(["status", "--short"]), "");

    let workspace_plan =
        fs::read_to_string(workspace_path.join("plan.md")).expect("read workspace plan");
    assert!(
        workspace_plan.contains("- [x] Write first.txt"),
        "{workspace_plan}"
    );
    assert!(
        workspace_plan.contains("- [x] Verify first.txt exists"),
        "{workspace_plan}"
    );
    let generated =
        fs::read_to_string(workspace_path.join("first.txt")).expect("read generated file");
    assert_eq!(generated, "created by fake agent\n");
    let workspace_status = Command::new("git")
        .current_dir(&workspace_path)
        .args(["status", "--short"])
        .output()
        .expect("workspace git status");
    assert!(
        workspace_status.status.success(),
        "workspace git status failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&workspace_status.stdout),
        String::from_utf8_lossy(&workspace_status.stderr)
    );
    let workspace_status = String::from_utf8(workspace_status.stdout).expect("git status utf8");
    assert!(
        workspace_status.contains(" M plan.md\n"),
        "{workspace_status}"
    );
    assert!(
        workspace_status.contains("?? first.txt\n"),
        "{workspace_status}"
    );
}

#[test]
fn run_command_dry_run_with_workspace_id_does_not_create_workspace() {
    let repo = TempRepo::new();
    repo.init_git();
    fs::write(
        repo.path.join("plan.md"),
        r#"# Example plan

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            "plan.md",
            "--workspace-id",
            "isolated",
            "--dry-run",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let workspace_path = repo.path.join(".ralphterm/workspaces/isolated");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "Workspace: {} (dry run)",
            workspace_path.display()
        )),
        "{stdout}"
    );
    assert!(stdout.contains("Dry run: plan.md"), "{stdout}");
    assert!(
        !workspace_path.exists(),
        "dry run must not create a managed workspace"
    );
    assert_eq!(repo.git_output(["status", "--short"]), "");
}

#[test]
fn run_command_with_workspace_id_preserves_nested_cwd_relative_plan_path() {
    let repo = TempRepo::new();
    repo.init_git();
    fs::create_dir(repo.path.join("docs")).expect("create docs");
    fs::write(
        repo.path.join("docs/plan.md"),
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write nested plan");
    repo.git(["add", "docs/plan.md"]);
    repo.git(["commit", "-m", "docs: add nested test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path.join("docs"))
        .args([
            "run",
            "plan.md",
            "--workspace-id",
            "isolated",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let workspace_path = repo.path.join(".ralphterm/workspaces/isolated");
    let workspace_plan = fs::read_to_string(workspace_path.join("docs/plan.md"))
        .expect("read workspace nested plan");
    assert!(
        workspace_plan.contains("- [x] Write first.txt"),
        "{workspace_plan}"
    );
    let generated = fs::read_to_string(workspace_path.join("docs/first.txt"))
        .expect("read nested generated file");
    assert_eq!(generated, "created by fake agent\n");
    assert!(
        !workspace_path.join("first.txt").exists(),
        "nested run must keep cwd-relative generated files under docs"
    );
    assert_eq!(repo.git_output(["status", "--short"]), "");
}

#[test]
fn run_command_with_workspace_id_rejects_relative_plan_path_that_escapes_repo_root() {
    let repo = TempRepo::new();
    repo.init_git();
    fs::create_dir(repo.path.join("docs")).expect("create docs");
    fs::write(repo.path.join("docs/keep.md"), "# Keep\n").expect("write tracked file");
    repo.git(["add", "docs/keep.md"]);
    repo.git(["commit", "-m", "docs: add tracked file"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(repo.path.join("docs"))
        .args([
            "run",
            "../../outside.md",
            "--workspace-id",
            "isolated",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("plan path must stay inside the repository"),
        "{diagnostics}"
    );
    assert!(
        !repo.path.join(".ralphterm/workspaces/isolated").exists(),
        "escape rejection must not create a workspace"
    );
    assert_eq!(repo.git_output(["status", "--short"]), "");
}

#[test]
fn run_command_with_workspace_id_rejects_absolute_plan_path() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--workspace-id",
            "isolated",
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("--workspace-id requires a relative plan path"),
        "{diagnostics}"
    );
    assert!(
        !repo.path.join(".ralphterm/workspaces/isolated").exists(),
        "absolute-path rejection must not create a workspace"
    );
    assert_eq!(repo.git_output(["status", "--short"]), "");
}

#[test]
fn run_command_marks_completed_tasks_and_commits() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
- [ ] Verify first.txt exists

Literal example: `- [ ] do not mark`
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("unrelated.txt"), "original\n").expect("write unrelated file");
    fs::write(repo.path.join("staged.txt"), "original\n").expect("write staged file");
    repo.git(["add", "plan.md", "unrelated.txt", "staged.txt"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::write(repo.path.join("unrelated.txt"), "do not commit\n").expect("dirty unrelated file");
    fs::write(repo.path.join("staged.txt"), "do not commit\n").expect("dirty staged file");
    repo.git(["add", "staged.txt"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read updated plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");
    assert!(plan.contains("- [x] Verify first.txt exists"), "{plan}");
    assert!(
        plan.contains("Literal example: `- [ ] do not mark`"),
        "{plan}"
    );

    let log = repo.git_output(["log", "--oneline", "-1"]);
    assert!(
        log.contains("task: Create first file"),
        "latest commit should be task commit, got {log}"
    );
    let committed_files = repo.git_output(["show", "--name-only", "--format=", "HEAD"]);
    assert!(committed_files.contains("plan.md"), "{committed_files}");
    assert!(committed_files.contains("first.txt"), "{committed_files}");
    assert!(
        !committed_files.contains("unrelated.txt"),
        "unrelated dirty file should not be committed: {committed_files}"
    );
    assert!(
        !committed_files.contains("staged.txt"),
        "unrelated staged file should not be committed: {committed_files}"
    );
    assert_eq!(
        repo.git_output(["status", "--short"]),
        "M  staged.txt\n M unrelated.txt\n"
    );

    let progress_log_path = repo.path.join(".ralphterm/progress/plan.log");
    let progress_log = fs::read_to_string(&progress_log_path).expect("read progress log");
    assert!(
        progress_log.contains("task_start number=1 title=Create first file"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("validation result=passed"),
        "{progress_log}"
    );
    let commit_hash = repo.git_output(["rev-parse", "--short", "HEAD"]);
    assert!(
        progress_log.contains(&format!("commit hash={}", commit_hash.trim())),
        "{progress_log}"
    );
    assert!(progress_log.contains("signal=COMPLETED"), "{progress_log}");
    assert!(
        progress_log.contains("task_end number=1 result=passed"),
        "{progress_log}"
    );

    let transcript_line = progress_log
        .lines()
        .find(|line| line.contains("transcript path="))
        .expect("transcript path logged");
    let transcript_path = transcript_line
        .split("transcript path=")
        .nth(1)
        .expect("transcript path value")
        .trim();
    let transcript = fs::read_to_string(repo.path.join(transcript_path)).expect("read transcript");
    assert!(transcript.contains("COMPLETED"), "{transcript}");

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read run summary");
    assert!(summary.contains("# Run Summary: plan.md"), "{summary}");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(
        summary.contains("- Task 1: Create first file — passed"),
        "{summary}"
    );
    assert!(summary.contains(transcript_path), "{summary}");

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/first.txt b/first.txt"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("+created by fake agent"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("diff --git a/plan.md b/plan.md"),
        "{diff_patch}"
    );
    assert!(
        !diff_patch.contains("unrelated.txt"),
        "unrelated dirty file should not be included in run patch: {diff_patch}"
    );
    assert!(
        !diff_patch.contains("staged.txt"),
        "unrelated staged file should not be included in run patch: {diff_patch}"
    );
}

#[test]
fn run_command_requires_completed_signal_before_validation_review_completion_or_commit() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    let initial_head = repo.git_output(["rev-parse", "HEAD"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded without COMPLETED\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("COMPLETED"), "{diagnostics}");
    assert!(diagnostics.contains("signal"), "{diagnostics}");

    let plan = fs::read_to_string(&plan_path).expect("read plan after failed run");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");
    assert!(!plan.contains("- [x] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(progress_log.contains("signal=NONE"), "{progress_log}");
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );
    assert!(
        !progress_log.contains("validation result="),
        "validation must not run before COMPLETED:\n{progress_log}"
    );
    assert!(
        !progress_log.contains("review result="),
        "review must not run before COMPLETED:\n{progress_log}"
    );
    assert!(
        !progress_log.contains("commit hash="),
        "commit must not happen before COMPLETED:\n{progress_log}"
    );
    assert!(
        !progress_log.contains("task_end number=1 result=passed"),
        "task must not pass before COMPLETED:\n{progress_log}"
    );
    assert!(
        !repo
            .path
            .join(".ralphterm/progress/plan-task-1-validation.txt")
            .exists(),
        "validation artifact must not be written before COMPLETED"
    );
    assert!(
        !repo
            .path
            .join(".ralphterm/progress/plan-task-1-review.transcript")
            .exists(),
        "review transcript must not be written before COMPLETED"
    );
    assert!(
        !repo.path.join("review-prompt.txt").exists(),
        "review command must not run before COMPLETED"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("agent completion"), "{summary}");
    assert!(summary.contains("COMPLETED"), "{summary}");
    assert!(!summary.contains("Review transcript:"), "{summary}");
    assert!(!summary.contains("REVIEW_PASS"), "{summary}");

    let current_head = repo.git_output(["rev-parse", "HEAD"]);
    assert_eq!(
        current_head, initial_head,
        "must not commit before COMPLETED"
    );
}

#[test]
fn run_command_missing_completed_does_not_link_stale_validation_artifact() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    let validation_path = ".ralphterm/progress/plan-task-1-validation.txt";
    fs::write(repo.path.join(validation_path), "stale validation output\n")
        .expect("write stale validation artifact");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded without COMPLETED\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("agent completion"), "{summary}");
    assert!(
        !summary.contains("Validation:"),
        "failed summary must not link stale validation output when validation did not run:\n{summary}"
    );
    assert!(
        !summary.contains(validation_path),
        "failed summary must not present stale validation path as current validation:\n{summary}"
    );
    assert!(
        !summary.contains("stale validation output"),
        "failed summary must not contain stale validation text:\n{summary}"
    );
    assert!(
        repo.path.join(validation_path).exists(),
        "stale validation artifact should be preserved for resume diagnostics"
    );
}

#[test]
fn run_command_missing_completed_does_not_link_stale_review_transcript() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    let review_path = ".ralphterm/progress/plan-task-1-review.transcript";
    fs::write(repo.path.join(review_path), "stale review transcript\n")
        .expect("write stale review transcript");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded without COMPLETED\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("agent completion"), "{summary}");
    assert!(
        !summary.contains("Review transcript:"),
        "failed summary must not link stale review transcript when review did not run:\n{summary}"
    );
    assert!(
        !summary.contains(review_path),
        "failed summary must not present stale review transcript path as current review:\n{summary}"
    );
    assert!(
        repo.path.join(review_path).exists(),
        "stale review transcript should be preserved for resume diagnostics"
    );
}

#[test]
fn run_command_review_spawn_failure_does_not_link_stale_review_transcript() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    let review_path = ".ralphterm/progress/plan-task-1-review.transcript";
    fs::write(repo.path.join(review_path), "stale review transcript\n")
        .expect("write stale review transcript");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            "ralphterm-review-command-does-not-exist-for-test",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded with missing review command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("review"), "{diagnostics}");
    assert!(
        diagnostics.contains("run review for task 1"),
        "{diagnostics}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Phase: review"), "{summary}");
    assert!(
        !summary.contains("Review transcript:"),
        "failed summary must not link stale review transcript when current review did not write one:\n{summary}"
    );
    assert!(
        !summary.contains(review_path),
        "failed summary must not present stale review transcript path as current review:\n{summary}"
    );
    assert!(
        repo.path.join(review_path).exists(),
        "stale review transcript should be preserved for resume diagnostics"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join(review_path)).expect("read stale review transcript"),
        "stale review transcript\n",
        "spawn failure must not overwrite stale review transcript"
    );
}

#[test]
fn run_command_validation_failure_overwrites_artifact_and_links_failed_summary() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `printf 'current stdout\n'; printf 'current stderr\n' >&2; test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    let validation_path = ".ralphterm/progress/plan-task-1-validation.txt";
    fs::write(repo.path.join(validation_path), "stale validation output\n")
        .expect("write stale validation artifact");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let validation = fs::read_to_string(repo.path.join(validation_path))
        .expect("read validation output artifact from failed validation");
    assert!(
        validation.contains("Validation: printf 'current stdout\\n'; printf 'current stderr\\n' >&2; test -f missing.txt"),
        "{validation}"
    );
    assert!(validation.contains("current stdout"), "{validation}");
    assert!(validation.contains("current stderr"), "{validation}");
    assert!(
        !validation.contains("stale validation output"),
        "failed validation should overwrite stale validation artifact:\n{validation}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(
        summary.contains(&format!("Validation: {validation_path}")),
        "failed run summary should link validation output artifact:\n{summary}"
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read failed run diff patch");
    assert!(
        diff_patch.contains("diff --git a/first.txt b/first.txt"),
        "failed no-commit run should preserve agent-created file diff:\n{diff_patch}"
    );
    assert!(
        diff_patch.contains("+created by fake agent"),
        "{diff_patch}"
    );
}

#[test]
fn run_command_writes_validation_artifact_and_links_summary() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `printf 'validation stdout\n'; printf 'validation stderr\n' >&2; test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let validation_path = ".ralphterm/progress/plan-task-1-validation.txt";
    let validation = fs::read_to_string(repo.path.join(validation_path))
        .expect("read validation output artifact");
    assert!(
        validation.contains("Validation: printf 'validation stdout\\n'; printf 'validation stderr\\n' >&2; test -f first.txt"),
        "{validation}"
    );
    assert!(validation.contains("validation stdout"), "{validation}");
    assert!(validation.contains("validation stderr"), "{validation}");
    assert!(validation.contains("Validation passed"), "{validation}");

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read run summary");
    assert!(
        summary.contains(&format!("Validation: {validation_path}")),
        "run summary should link validation output artifact:\n{summary}"
    );
}

#[test]
fn run_command_no_commit_writes_working_tree_diff_patch() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/first.txt b/first.txt"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("+created by fake agent"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("diff --git a/plan.md b/plan.md"),
        "{diff_patch}"
    );
}

#[test]
fn run_command_no_commit_diff_patch_includes_recreated_tracked_file_matching_head() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f tracked.txt`

### Task 1: Recreate tracked file
- [ ] Recreate tracked.txt with base content
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("tracked.txt"), "base\n").expect("write tracked file");
    repo.git(["add", "plan.md", "tracked.txt"]);
    repo.git(["commit", "-m", "docs: add tracked file"]);
    fs::remove_file(repo.path.join("tracked.txt")).expect("delete tracked file before run");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(repo.git_output(["status", "--short", "tracked.txt"]), "");

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/tracked.txt b/tracked.txt"),
        "{diff_patch}"
    );
    assert!(diff_patch.contains("+base"), "{diff_patch}");
}

#[test]
fn run_command_no_commit_diff_patch_includes_file_created_in_new_untracked_directory() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f nested/generated.txt`

### Task 1: Create nested generated file
- [ ] Write nested/generated.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repo.path.join("nested/generated.txt").exists(),
        "fake agent should create nested/generated.txt"
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/nested/generated.txt b/nested/generated.txt"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("+nested content from fake agent"),
        "{diff_patch}"
    );
}

#[test]
fn run_command_no_commit_diff_patch_includes_run_file_in_preexisting_untracked_directory() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f nested/generated.txt`

### Task 1: Create nested generated file
- [ ] Write nested/generated.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::create_dir(repo.path.join("nested")).expect("create preexisting untracked dir");
    fs::write(repo.path.join("nested/old.txt"), "preexisting\n")
        .expect("write preexisting untracked file");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/nested/generated.txt b/nested/generated.txt"),
        "run-created file in preexisting untracked dir should be included: {diff_patch}"
    );
    assert!(
        diff_patch.contains("+nested content from fake agent"),
        "{diff_patch}"
    );
    assert!(
        !diff_patch.contains("nested/old.txt"),
        "preexisting untracked file in same dir should not be included: {diff_patch}"
    );
}

#[test]
fn run_command_no_commit_propagates_git_status_baseline_errors_inside_worktree() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `true`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let fake_bin = repo.path.join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin");
    let fake_git = fake_bin.join("git");
    fs::write(
        &fake_git,
        "#!/usr/bin/env sh\nif [ \"$1 $2\" = \"rev-parse --is-inside-work-tree\" ]; then\n  printf 'true\\n'\n  exit 0\nfi\nif [ \"$1\" = status ]; then\n  printf 'injected status failure\\n' >&2\n  exit 42\nfi\nprintf 'unexpected fake git invocation: %s\\n' \"$*\" >&2\nexit 43\n",
    )
    .expect("write fake git");
    let mut permissions = fs::metadata(&fake_git)
        .expect("fake git metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_git, permissions).expect("chmod fake git");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").expect("PATH set")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("PATH", path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("snapshot run baseline git status"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("injected status failure"),
        "{diagnostics}"
    );
    assert!(
        !repo.path.join("first.txt").exists(),
        "agent should not run after baseline status failure"
    );
}

#[test]
fn run_command_no_commit_diff_patch_excludes_preexisting_dirty_paths() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("unrelated.txt"), "original\n").expect("write unrelated file");
    fs::write(repo.path.join("staged.txt"), "original\n").expect("write staged file");
    fs::write(repo.path.join("untracked-before.txt"), "do not include\n")
        .expect("write preexisting untracked file");
    repo.git(["add", "plan.md", "unrelated.txt", "staged.txt"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::write(repo.path.join("unrelated.txt"), "do not include\n").expect("dirty unrelated file");
    fs::write(repo.path.join("staged.txt"), "do not include\n").expect("dirty staged file");
    repo.git(["add", "staged.txt"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/first.txt b/first.txt"),
        "{diff_patch}"
    );
    assert!(
        diff_patch.contains("diff --git a/plan.md b/plan.md"),
        "{diff_patch}"
    );
    assert!(
        !diff_patch.contains("unrelated.txt"),
        "preexisting dirty tracked file should not be included: {diff_patch}"
    );
    assert!(
        !diff_patch.contains("staged.txt"),
        "preexisting staged file should not be included: {diff_patch}"
    );
    assert!(
        !diff_patch.contains("untracked-before.txt"),
        "preexisting untracked file should not be included: {diff_patch}"
    );
}

#[test]
fn run_command_no_commit_diff_patch_includes_run_delta_for_preexisting_dirty_tracked_file() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `grep -q run-change tracked.txt`

### Task 1: Change tracked file
- [ ] Change tracked.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("tracked.txt"), "base\n").expect("write tracked file");
    repo.git(["add", "plan.md", "tracked.txt"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::write(repo.path.join("tracked.txt"), "preexisting-dirty\n")
        .expect("dirty tracked file before run");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/tracked.txt b/tracked.txt"),
        "{diff_patch}"
    );
    assert!(diff_patch.contains("-preexisting-dirty"), "{diff_patch}");
    assert!(diff_patch.contains("+run-change"), "{diff_patch}");
    assert!(
        !diff_patch.contains("+preexisting-dirty"),
        "pre-run dirty content should not be represented as final added content: {diff_patch}"
    );
}

#[test]
fn run_command_no_commit_diff_patch_includes_recreated_dirty_tracked_deletion() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `grep -q run-change tracked.txt`

### Task 1: Change tracked file
- [ ] Change tracked.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("tracked.txt"), "base\n").expect("write tracked file");
    repo.git(["add", "plan.md", "tracked.txt"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    fs::remove_file(repo.path.join("tracked.txt")).expect("delete tracked file before run");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/tracked.txt b/tracked.txt"),
        "{diff_patch}"
    );
    assert!(diff_patch.contains("+run-change"), "{diff_patch}");
}

#[test]
fn run_command_no_commit_diff_patch_includes_run_staged_changes() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `git diff --cached --name-only -- generated.txt | grep -q generated.txt`

### Task 1: Stage generated file
- [ ] Stage generated.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("staging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert!(
        diff_patch.contains("diff --git a/generated.txt b/generated.txt"),
        "{diff_patch}"
    );
    assert!(diff_patch.contains("+staged by fake agent"), "{diff_patch}");
    assert!(
        diff_patch.contains("diff --git a/plan.md b/plan.md"),
        "{diff_patch}"
    );
}

#[test]
fn run_command_with_no_pending_tasks_writes_empty_diff_patch() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Already done
- [x] Nothing pending
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add completed plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_patch = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-diff.patch"))
        .expect("read run diff patch");
    assert_eq!(diff_patch, "");
}

#[test]
fn run_command_prints_pending_tasks_in_order() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt

The agent should create first.txt.

### Task 2: Already finished
- [x] Nothing left here

### Task 3: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Executing plan.md"), "{stdout}");
    assert!(stdout.contains("Task 1: Create first file"), "{stdout}");
    assert!(stdout.contains("Task 3: Create second file"), "{stdout}");
    assert!(!stdout.contains("Already finished"), "{stdout}");
    assert!(stdout.contains("COMPLETED"), "{stdout}");
    assert!(stdout.contains("Validation: test -f first.txt"), "{stdout}");
    assert!(stdout.contains("Validation passed"), "{stdout}");

    assert_eq!(
        fs::read_to_string(repo.path.join("first.txt")).expect("first file created"),
        "created by fake agent\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("second.txt")).expect("second file created"),
        "created by fake agent\n"
    );
}

#[test]
fn run_command_writes_passed_summary_with_transcripts_after_success() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt || test -f second.txt`

### Task 1: Create first file
- [ ] Write first.txt

### Task 2: Already done
- [x] Nothing left here

### Task 3: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    let transcript_paths: Vec<&str> = progress_log
        .lines()
        .filter_map(|line| line.split("transcript path=").nth(1))
        .map(str::trim)
        .collect();
    assert_eq!(transcript_paths.len(), 2, "{progress_log}");

    let summary_path = repo.path.join(".ralphterm/progress/plan-summary.md");
    let summary = fs::read_to_string(&summary_path).expect("read run summary");
    assert!(summary.contains("# Run Summary: plan.md"), "{summary}");
    assert!(summary.contains("Result: passed"), "{summary}");
    assert!(
        summary.contains("- Task 1: Create first file — passed"),
        "{summary}"
    );
    assert!(
        summary.contains("- Task 3: Create second file — passed"),
        "{summary}"
    );
    assert!(!summary.contains("Already done"), "{summary}");
    for transcript_path in transcript_paths {
        assert!(summary.contains(transcript_path), "{summary}");
    }
}

#[test]
fn dry_run_lists_work_without_starting_agent_or_editing_files() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt

### Task 2: Done already
- [x] Nothing left here

### Task 3: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");
    let original_plan = fs::read_to_string(&plan_path).expect("read original plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--dry-run",
            "--agent-command",
            "definitely-not-a-real-agent-command",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm dry run");

    assert!(
        output.status.success(),
        "ralphterm dry run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry run: plan.md"), "{stdout}");
    assert!(stdout.contains("Review: skipped"), "{stdout}");
    assert!(stdout.contains("Validation: test -f first.txt"), "{stdout}");
    assert!(stdout.contains("Task 1: Create first file"), "{stdout}");
    assert!(stdout.contains("Task 3: Create second file"), "{stdout}");
    assert!(!stdout.contains("Done already"), "{stdout}");
    assert!(
        !repo.path.join(".ralphterm").exists(),
        "dry run should not write progress logs"
    );
    assert_eq!(
        fs::read_to_string(&plan_path).expect("read plan after dry run"),
        original_plan
    );
}

#[test]
fn dry_run_with_required_review_refuses_missing_review_config() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let original_plan = fs::read_to_string(&plan_path).expect("read original plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--dry-run",
            "--require-review",
            "--agent-command",
            "definitely-not-a-real-agent-command",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm dry run");

    assert!(
        !output.status.success(),
        "dry run should reject required review with no reviewer\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !repo.path.join(".ralphterm").exists(),
        "failed dry run should not write progress logs"
    );
    assert_eq!(
        fs::read_to_string(&plan_path).expect("read plan after dry run"),
        original_plan
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("--require-review needs --review-command or --review-agent"),
        "{diagnostics}"
    );
}

#[test]
fn dry_run_with_required_review_rejects_same_implementation_and_review_command() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--dry-run",
            "--require-review",
            "--agent",
            "claude",
            "--review-agent",
            "claude",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm dry run");

    assert!(
        !output.status.success(),
        "dry run should reject non-independent review config\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !repo.path.join(".ralphterm").exists(),
        "failed dry run should not write progress logs"
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("independent review command must differ from agent command"),
        "{diagnostics}"
    );
}

#[test]
fn dry_run_enforces_review_gate_even_when_no_tasks_are_pending() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

### Task 1: Already done
- [x] Nothing left
"#,
    )
    .expect("write plan");

    let missing_reviewer = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--dry-run",
            "--require-review",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm dry run");
    assert!(
        !missing_reviewer.status.success(),
        "no-pending dry run should still reject missing reviewer\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&missing_reviewer.stdout),
        String::from_utf8_lossy(&missing_reviewer.stderr)
    );

    let same_command = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--dry-run",
            "--require-review",
            "--agent",
            "claude",
            "--review-agent",
            "claude",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm dry run");
    assert!(
        !same_command.status.success(),
        "no-pending dry run should still reject non-independent review config\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&same_command.stdout),
        String::from_utf8_lossy(&same_command.stderr)
    );
    assert!(
        !repo.path.join(".ralphterm").exists(),
        "failed dry run should not write progress logs"
    );
}

#[test]
fn agent_shortcut_codex_uses_interactive_codex_from_path() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let bin_dir = repo.path.join("bin");
    fs::create_dir(&bin_dir).expect("create bin dir");
    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/usr/bin/env sh
set -eu
printf '%s\n' "$#" > codex-argc.txt
printf '%s\n' "$*" > codex-argv.txt
prompt=$(cat)
printf '%s\n' "$prompt" > codex-prompt.txt
if printf '%s\n' "$prompt" | grep -q 'Write first.txt'; then
  printf 'created by fake codex\n' > first.txt
fi
printf 'COMPLETED\n'
"#,
    )
    .expect("write fake codex");
    fs::set_permissions(&codex_path, fs::Permissions::from_mode(0o755)).expect("chmod fake codex");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").expect("PATH is set")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("PATH", path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent",
            "codex",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read_to_string(repo.path.join("codex-argc.txt")).expect("read argc"),
        "0\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("codex-argv.txt")).expect("read argv"),
        "\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("first.txt")).expect("first file created"),
        "created by fake codex\n"
    );
    let prompt = fs::read_to_string(repo.path.join("codex-prompt.txt")).expect("read prompt");
    assert!(prompt.contains("Task 1: Create first file"), "{prompt}");
}

#[test]
fn progress_signal_ignores_prompt_echo() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-no-completed.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded without COMPLETED\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(progress_log.contains("signal=NONE"), "{progress_log}");
    assert!(!progress_log.contains("signal=COMPLETED"), "{progress_log}");
    assert!(
        !progress_log.contains("validation result="),
        "validation must not run when only the prompt echo contains COMPLETED:\n{progress_log}"
    );
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );
}

#[test]
fn commit_excludes_ralphterm_artifacts_even_when_logs_are_ignored() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join(".gitignore"), "*.log\n").expect("write gitignore");
    repo.git(["add", "plan.md", ".gitignore"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let committed_files = repo.git_output(["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        !committed_files.contains(".ralphterm/"),
        "RalphTerm artifacts must not be committed: {committed_files}"
    );
}

#[test]
fn validation_failure_is_logged_and_does_not_complete_task() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("validation result=failed"),
        "{progress_log}"
    );
    assert!(progress_log.contains("task_end number=1"), "{progress_log}");
    assert!(progress_log.contains("result=failed"), "{progress_log}");
    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: validation"), "{summary}");
    assert!(
        summary.contains(".ralphterm/progress/plan-task-1.transcript"),
        "{summary}"
    );
}

#[test]
fn agent_command_failure_writes_transcript_and_failed_task_end() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("failing-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );
    let transcript_line = progress_log
        .lines()
        .find(|line| line.contains("transcript path="))
        .expect("transcript path logged");
    let transcript_path = transcript_line
        .split("transcript path=")
        .nth(1)
        .expect("transcript path value")
        .trim();
    let transcript = fs::read_to_string(repo.path.join(transcript_path)).expect("read transcript");
    assert!(
        transcript.contains("agent failure output before exit"),
        "{transcript}"
    );
    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: agent execution"), "{summary}");
    assert!(summary.contains(transcript_path), "{summary}");
}

#[test]
fn missing_agent_command_writes_failed_summary() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            "definitely-not-a-real-agent-command-for-failed-summary",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: agent execution"), "{summary}");
    assert!(
        summary.contains("spawn agent command")
            || summary.contains("No such file")
            || summary.contains("not found"),
        "{summary}"
    );
}

#[test]
fn failed_summary_includes_prior_passed_tasks_in_same_run() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt && test ! -f second.txt`

### Task 1: Create first file
- [ ] Write first.txt

### Task 2: Create second file
- [ ] Write second.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(
        summary.contains("- Task 1: Create first file — passed"),
        "{summary}"
    );
    assert!(
        summary.contains(".ralphterm/progress/plan-task-1.transcript"),
        "{summary}"
    );
    assert!(
        summary.contains("- Task 2: Create second file — failed"),
        "{summary}"
    );
    assert!(summary.contains("Phase: validation"), "{summary}");
    assert!(
        summary.contains(".ralphterm/progress/plan-task-2.transcript"),
        "{summary}"
    );
}

#[test]
fn review_command_pass_allows_task_acceptance_after_validation() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Review: pass"), "{stdout}");
    assert!(stdout.contains("Review passed"), "{stdout}");

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    let validation_index = progress_log
        .find("validation result=passed")
        .expect("validation pass logged");
    let review_index = progress_log
        .find("review result=passed")
        .expect("review pass logged");
    assert!(
        validation_index < review_index,
        "review must run after validation so it can verify the accepted task evidence:\n{progress_log}"
    );
    assert!(
        progress_log
            .contains("review transcript path=.ralphterm/progress/plan-task-1-review.transcript"),
        "{progress_log}"
    );
    let review_transcript = fs::read_to_string(
        repo.path
            .join(".ralphterm/progress/plan-task-1-review.transcript"),
    )
    .expect("read review transcript");
    assert!(
        review_transcript.contains("REVIEW_PASS"),
        "{review_transcript}"
    );
    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    assert!(
        review_prompt.contains("Validation output:\nValidation: test -f first.txt"),
        "review prompt should include completed validation output:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("Validation passed"),
        "review prompt should include validation pass evidence:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("\nfirst.txt\n"),
        "review prompt should expose newly-created files for review:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("diff --git a/first.txt b/first.txt"),
        "review prompt should include a patch for untracked files, not only their names:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("+created by fake agent"),
        "review prompt should expose untracked file contents for independent verification:\n{review_prompt}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read run summary");
    assert!(
        summary.contains(".ralphterm/progress/plan-task-1-review.transcript"),
        "passed run summary should link the independent review transcript:\n{summary}"
    );
}

#[test]
fn review_prompt_omits_large_untracked_file_contents_with_marker() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    let large_secret = "DO_NOT_SEND_TO_REVIEW".repeat(4_000);
    fs::write(repo.path.join("large-secret.txt"), &large_secret)
        .expect("write large untracked file");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    assert!(
        review_prompt.contains("large-secret.txt patch omitted: file exceeds review prompt limit"),
        "large untracked files should be named but omitted with a clear marker"
    );
    assert!(
        !review_prompt.contains("DO_NOT_SEND_TO_REVIEW"),
        "large untracked file contents must not be sent to the review command"
    );
    assert!(
        review_prompt.contains("diff --git a/first.txt b/first.txt"),
        "small task-created files should still include patches"
    );
}

#[test]
fn review_prompt_applies_aggregate_untracked_patch_budget() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    for index in 0..10 {
        fs::write(
            repo.path.join(format!("small-untracked-{index:02}.txt")),
            format!("AGGREGATE_SECRET_{index}\n{}", "x".repeat(10_000)),
        )
        .expect("write small untracked file");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    assert!(
        review_prompt
            .contains("small-untracked-09.txt patch omitted: untracked patch budget exhausted"),
        "aggregate untracked file patch budget should omit later files with a clear marker"
    );
    assert!(
        !review_prompt.contains("AGGREGATE_SECRET_9"),
        "files omitted by aggregate budget must not leak their contents"
    );
    assert!(
        review_prompt.contains("diff --git a/first.txt b/first.txt"),
        "task-created file patch should still be available for review"
    );
}

#[test]
fn review_command_hanging_agent_exits_nonzero_with_bounded_timeout() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    let start = Instant::now();

    let mut command = Command::new(env!("CARGO_BIN_EXE_ralphterm"));
    command
        .current_dir(&repo.path)
        .env("RALPHTERM_AGENT_TIMEOUT_MS", "250")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("hanging-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--require-review",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    let output = run_with_test_timeout(command, Duration::from_secs(5));

    let elapsed = start.elapsed();
    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "review timeout was not bounded: elapsed={elapsed:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = format!("{stdout}\n{stderr}");
    assert!(
        diagnostics.contains("review command timed out"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("still waiting for external input"),
        "{diagnostics}"
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );
    let review_transcript_path = ".ralphterm/progress/plan-task-1-review.transcript";
    assert!(
        progress_log.contains(&format!("review transcript path={review_transcript_path}")),
        "{progress_log}"
    );
    let review_transcript =
        fs::read_to_string(repo.path.join(review_transcript_path)).expect("read review transcript");
    assert!(
        review_transcript.contains("still waiting for external input"),
        "{review_transcript}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: review"), "{summary}");
    assert!(summary.contains("review command timed out"), "{summary}");
    assert!(summary.contains(review_transcript_path), "{summary}");
}

#[test]
fn require_review_rejects_same_implementation_and_review_command_before_agent_runs() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let fake_agent = fixture_path("fake-agent.sh");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fake_agent.to_str().expect("utf8 fixture path"),
            "--review-command",
            fake_agent.to_str().expect("utf8 fixture path"),
            "--require-review",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("independent review command must differ from agent command"),
        "{diagnostics}"
    );
    assert!(
        !repo.path.join("first.txt").exists(),
        "RalphTerm should reject non-independent review configuration before running the agent"
    );
}

#[test]
fn require_review_rejects_quoted_equivalent_implementation_and_review_command_before_agent_runs() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let fake_agent = fixture_path("fake-agent.sh");
    let fake_agent = fake_agent.to_str().expect("utf8 fixture path");
    let quoted_same_command = format!("'{fake_agent}'");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fake_agent,
            "--review-command",
            quoted_same_command.as_str(),
            "--require-review",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("independent review command must differ from agent command"),
        "{diagnostics}"
    );
    assert!(
        !repo.path.join("first.txt").exists(),
        "RalphTerm should reject equivalent non-independent review configuration before running the agent"
    );
}

#[test]
fn review_agent_codex_uses_codex_from_path_and_satisfies_required_review() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let bin_dir = repo.path.join("bin");
    fs::create_dir(&bin_dir).expect("create bin dir");
    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/usr/bin/env sh
set -eu
printf '%s\n' "$#" > review-codex-argc.txt
printf '%s\n' "$*" > review-codex-argv.txt
prompt=$(cat)
printf '%s\n' "$prompt" > review-codex-prompt.txt
printf 'REVIEW_PASS\n'
"#,
    )
    .expect("write fake codex");
    fs::set_permissions(&codex_path, fs::Permissions::from_mode(0o755)).expect("chmod fake codex");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").expect("PATH is set")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("PATH", path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-agent",
            "codex",
            "--require-review",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read_to_string(repo.path.join("review-codex-argc.txt")).expect("read argc"),
        "0\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("review-codex-argv.txt")).expect("read argv"),
        "\n"
    );
    let review_prompt =
        fs::read_to_string(repo.path.join("review-codex-prompt.txt")).expect("read review prompt");
    assert!(
        review_prompt.contains("Task 1: Create first file"),
        "{review_prompt}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Review passed"), "{stdout}");
    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=passed"),
        "{progress_log}"
    );
}

#[test]
fn validation_commands_run_before_review_command() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "validation should run before review so the reviewer sees acceptance evidence\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    let validation_index = progress_log
        .find("validation result=passed")
        .expect("validation pass logged");
    let review_index = progress_log
        .find("review result=passed")
        .expect("review pass logged");
    assert!(
        validation_index < review_index,
        "validation must be logged before the review gate:\n{progress_log}"
    );

    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    assert!(
        review_prompt.contains("Validation output:\nValidation: test -f first.txt"),
        "post-validation review prompt should include validation output:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("Validation passed"),
        "post-validation reviewer should see validation pass evidence:\n{review_prompt}"
    );
    assert!(
        !review_prompt.contains("Validation commands have not run yet"),
        "review prompt should not claim validation is pending:\n{review_prompt}"
    );
}

#[test]
fn validation_commands_that_change_worktree_are_visible_to_review() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `printf validation-side-effect > sneaky.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "validation side effects should be visible to the reviewer before acceptance\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    let git_state = review_prompt
        .split("Current git diff:\n")
        .nth(1)
        .expect("review prompt has git state");
    assert!(
        git_state.contains("Untracked files:\n") && git_state.contains("\nsneaky.txt\n"),
        "review prompt should include validation-created worktree artifacts in git state:\n{review_prompt}"
    );

    let validation_path = ".ralphterm/progress/plan-task-1-validation.txt";
    let validation = fs::read_to_string(repo.path.join(validation_path))
        .expect("read validation output artifact");
    assert!(
        validation.contains("Validation: printf validation-side-effect > sneaky.txt"),
        "{validation}"
    );
    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read passed run summary");
    assert!(
        summary.contains(&format!("Validation: {validation_path}")),
        "passed summary should link validation output artifact:\n{summary}"
    );
}

#[test]
fn validation_commands_that_change_untracked_files_are_visible_to_review() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `printf validation-side-effect > first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "validation changes to untracked files should be visible to the reviewer before acceptance\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let review_prompt = fs::read_to_string(repo.path.join("review-prompt.txt"))
        .expect("review fixture should capture prompt");
    let git_state = review_prompt
        .split("Current git diff:\n")
        .nth(1)
        .expect("review prompt has git state");
    assert!(
        git_state.contains("Untracked files:\n") && git_state.contains("\nfirst.txt\n"),
        "review prompt should include validation-mutated untracked file in git state:\n{review_prompt}"
    );
    assert!(
        review_prompt.contains("Validation: printf validation-side-effect > first.txt"),
        "review prompt should include the validation command output:\n{review_prompt}"
    );
}

#[test]
fn require_review_without_review_command_or_agent_refuses_to_run_agent() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--require-review",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !repo.path.join("first.txt").exists(),
        "agent should not run when review is required without a review command or agent"
    );
    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("--require-review needs --review-command or --review-agent"),
        "{diagnostics}"
    );
}

#[test]
fn review_agent_conflicts_with_review_command() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `true`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-agent",
            "codex",
            "--review-command",
            fixture_path("review-pass.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("--review-agent"), "{diagnostics}");
    assert!(diagnostics.contains("--review-command"), "{diagnostics}");
}

#[test]
fn review_command_ignores_agent_transcript_review_pass_noise() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent-review-pass-noise.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-silent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "silent reviewer must not pass from echoed agent transcript noise\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
}

#[test]
fn review_prompt_echo_without_explicit_decision_does_not_trigger_retry() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-echo-prompt.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "prompt-echoing reviewer without an explicit decision must fail without retry\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
    assert!(
        !progress_log.contains("agent_retry"),
        "prompt REVIEW_FAIL instructions must not make the run retry:\n{progress_log}"
    );
    assert!(
        !repo
            .path
            .join(".ralphterm/progress/plan-task-1-attempt-2.transcript")
            .exists(),
        "no retry transcript should be created for a reviewer with no explicit decision"
    );
}

#[test]
fn max_review_retries_zero_blocks_after_first_review_fail() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-after-review-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--max-review-retries",
            "0",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "review failure should block immediately when review retry budget is zero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");
    assert_eq!(
        fs::read_to_string(repo.path.join("agent-count.txt")).expect("read agent count"),
        "1\n"
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
    assert!(
        !progress_log.contains("agent_retry"),
        "zero review retry budget must not start a second implementation attempt:\n{progress_log}"
    );
    assert!(
        !repo
            .path
            .join(".ralphterm/progress/plan-task-1-attempt-2.transcript")
            .exists(),
        "attempt 2 transcript should not exist when review retry budget is zero"
    );
    let validation_path = ".ralphterm/progress/plan-task-1-validation.txt";
    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(
        summary.contains(&format!("Validation: {validation_path}")),
        "post-validation review failure summary should link validation output artifact:\n{summary}"
    );
}

#[test]
fn review_failure_triggers_agent_retry_and_rereview_before_acceptance() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-after-review-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should retry implementation after one failed review\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read_to_string(repo.path.join("first.txt")).expect("read fixed file"),
        "fixed after review\n"
    );
    assert!(
        !repo.path.join("rejected.txt").exists(),
        "accepted retry must not retain artifacts created only by the rejected implementation attempt"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("agent-count.txt")).expect("read agent count"),
        "2\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("review-count.txt")).expect("read review count"),
        "2\n"
    );
    let retry_prompt =
        fs::read_to_string(repo.path.join("agent-prompt-2.txt")).expect("read retry prompt");
    assert!(
        retry_prompt.contains("Previous review failed"),
        "retry prompt should include review failure feedback:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains("REVIEW_FAIL"),
        "retry prompt should include the reviewer transcript:\n{retry_prompt}"
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("review result=passed"),
        "{progress_log}"
    );
    let cleanup_index = progress_log
        .find("review_retry_cleanup result=passed")
        .expect("progress log should record cleanup before retry");
    let retry_index = progress_log
        .find("agent_retry attempt=2 reason=review_failed")
        .expect("progress log should record retry after review failure");
    assert!(
        cleanup_index < retry_index,
        "cleanup should be logged before retry starts:\n{progress_log}"
    );

    let attempt_1_transcript = repo
        .path
        .join(".ralphterm/progress/plan-task-1-attempt-1.transcript");
    let attempt_1_review_transcript = repo
        .path
        .join(".ralphterm/progress/plan-task-1-attempt-1-review.transcript");
    let attempt_2_transcript = repo
        .path
        .join(".ralphterm/progress/plan-task-1-attempt-2.transcript");
    let attempt_2_review_transcript = repo
        .path
        .join(".ralphterm/progress/plan-task-1-attempt-2-review.transcript");
    for artifact in [
        &attempt_1_transcript,
        &attempt_1_review_transcript,
        &attempt_2_transcript,
        &attempt_2_review_transcript,
    ] {
        assert!(
            artifact.exists(),
            "expected per-attempt artifact to exist: {}",
            artifact.display()
        );
    }

    assert!(
        fs::read_to_string(&attempt_1_review_transcript)
            .expect("read attempt 1 review transcript")
            .contains("REVIEW_FAIL"),
        "first review attempt should be preserved as failed"
    );
    assert!(
        fs::read_to_string(&attempt_2_review_transcript)
            .expect("read attempt 2 review transcript")
            .contains("REVIEW_PASS"),
        "second review attempt should be preserved as passed"
    );
    assert!(
        progress_log
            .contains("transcript path=.ralphterm/progress/plan-task-1-attempt-2.transcript"),
        "progress log should expose final accepted transcript path:\n{progress_log}"
    );
    assert!(
        progress_log.contains(
            "review transcript path=.ralphterm/progress/plan-task-1-attempt-2-review.transcript"
        ),
        "progress log should expose final accepted review transcript path:\n{progress_log}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read run summary");
    assert!(
        summary.contains("Transcript: .ralphterm/progress/plan-task-1-attempt-2.transcript"),
        "summary should expose final accepted transcript path:\n{summary}"
    );
    assert!(
        summary.contains(
            "Review transcript: .ralphterm/progress/plan-task-1-attempt-2-review.transcript"
        ),
        "summary should expose final accepted review transcript path:\n{summary}"
    );
}

#[test]
fn review_retry_cleanup_preserves_preexisting_symlink() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("link-target.txt"), "baseline target\n").expect("write link target");
    symlink("link-target.txt", repo.path.join("kept-link")).expect("create baseline symlink");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_RETRY_CLEANUP_SCENARIO", "symlink-survives")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-cleanup-mutator-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should retry successfully without deleting baseline symlink\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let link_metadata = fs::symlink_metadata(repo.path.join("kept-link"))
        .expect("baseline symlink should survive retry cleanup");
    assert!(
        link_metadata.file_type().is_symlink(),
        "kept-link should still be a symlink"
    );
    assert_eq!(
        fs::read_link(repo.path.join("kept-link")).expect("read symlink target"),
        PathBuf::from("link-target.txt")
    );
}

#[test]
fn review_retry_cleanup_restores_executable_permissions() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let executable_path = repo.path.join("executable.sh");
    fs::write(
        &executable_path,
        "#!/usr/bin/env sh\nprintf 'baseline executable\\n'\n",
    )
    .expect("write baseline executable");
    fs::set_permissions(&executable_path, fs::Permissions::from_mode(0o755))
        .expect("chmod baseline executable");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_RETRY_CLEANUP_SCENARIO", "chmod-executable")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-cleanup-mutator-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should retry successfully and restore executable mode\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&executable_path).expect("read restored executable"),
        "#!/usr/bin/env sh\nprintf 'baseline executable\\n'\n"
    );
    assert_eq!(
        fs::metadata(&executable_path)
            .expect("stat restored executable")
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "retry cleanup should restore the executable bit"
    );
}

#[test]
fn review_retry_cleanup_restores_directory_permissions() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let directory_path = repo.path.join("restricted-dir");
    fs::create_dir(&directory_path).expect("create baseline directory");
    fs::set_permissions(&directory_path, fs::Permissions::from_mode(0o711))
        .expect("chmod baseline directory");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_RETRY_CLEANUP_SCENARIO", "chmod-directory")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-cleanup-mutator-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should retry successfully and restore directory mode\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::metadata(&directory_path)
            .expect("stat restored directory")
            .permissions()
            .mode()
            & 0o777,
        0o711,
        "retry cleanup should restore directory search permissions"
    );
}

#[test]
fn review_retry_cleanup_restores_non_traversable_baseline_directory_before_retry() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let directory_path = repo.path.join("restricted-dir");
    fs::create_dir(&directory_path).expect("create baseline directory");
    fs::write(
        directory_path.join("baseline-child.txt"),
        "baseline child\n",
    )
    .expect("write baseline child");
    fs::set_permissions(&directory_path, fs::Permissions::from_mode(0o755))
        .expect("chmod baseline directory");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env(
            "RALPHTERM_RETRY_CLEANUP_SCENARIO",
            "chmod-non-traversable-directory",
        )
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-cleanup-mutator-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should restore non-traversable baseline directory before retry\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(directory_path.join("baseline-child.txt"))
            .expect("read baseline child after retry cleanup"),
        "baseline child\n"
    );
    assert_eq!(
        fs::metadata(&directory_path)
            .expect("stat restored directory")
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "retry cleanup should restore directory permissions after making it traversable"
    );
}

#[test]
fn review_retry_cleanup_restores_baseline_file_replaced_by_directory() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    fs::write(repo.path.join("baseline-file"), "baseline file\n").expect("write baseline file");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .env("RALPHTERM_RETRY_CLEANUP_SCENARIO", "file-to-dir")
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-cleanup-mutator-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-once.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should clean a rejected file-to-directory type change before retry\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("baseline-file")).expect("read restored baseline file"),
        "baseline file\n"
    );
}

#[test]
fn run_without_review_retry_budget_does_not_capture_retry_cleanup_snapshot() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let unreadable_dir = repo.path.join("unreadable-baseline-dir");
    fs::create_dir(&unreadable_dir).expect("create unreadable dir");
    fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o000))
        .expect("make dir unreadable");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");
    fs::set_permissions(&unreadable_dir, fs::Permissions::from_mode(0o755))
        .expect("restore dir permissions for temp cleanup");

    assert!(
        output.status.success(),
        "run without review retries should not traverse the whole worktree for retry cleanup\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn max_review_retries_two_allows_two_review_failures_before_acceptance() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-after-review-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-twice.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--max-review-retries",
            "2",
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run should allow two failed reviews with --max-review-retries 2 before acceptance\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        fs::read_to_string(repo.path.join("first.txt")).expect("read fixed file"),
        "fixed after review\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("agent-count.txt")).expect("read agent count"),
        "3\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("review-count.txt")).expect("read review count"),
        "3\n"
    );

    for attempt in 1..=3 {
        assert!(
            repo.path
                .join(format!(
                    ".ralphterm/progress/plan-task-1-attempt-{attempt}.transcript"
                ))
                .exists(),
            "expected implementation transcript for attempt {attempt}"
        );
        assert!(
            repo.path
                .join(format!(
                    ".ralphterm/progress/plan-task-1-attempt-{attempt}-review.transcript"
                ))
                .exists(),
            "expected review transcript for attempt {attempt}"
        );
    }

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert_eq!(
        progress_log.matches("review result=failed").count(),
        2,
        "progress log should record exactly two failed reviews before the pass:\n{progress_log}"
    );
    assert_eq!(
        progress_log.matches("agent_retry").count(),
        2,
        "progress log should record exactly two review-driven implementation retries:\n{progress_log}"
    );
    assert!(
        progress_log.contains("review result=passed"),
        "{progress_log}"
    );

    let second_retry_prompt =
        fs::read_to_string(repo.path.join("agent-prompt-3.txt")).expect("read second retry prompt");
    assert!(
        second_retry_prompt.contains("Previous review failed"),
        "second retry prompt should include review failure feedback:\n{second_retry_prompt}"
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");
}

#[test]
fn failed_retry_summary_links_failing_attempt_artifacts() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-after-review-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "run should fail when the retry review also fails\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        repo.path
            .join(".ralphterm/progress/plan-task-1-attempt-2.transcript")
            .exists(),
        "attempt 2 transcript should exist"
    );
    assert!(
        repo.path
            .join(".ralphterm/progress/plan-task-1-attempt-2-review.transcript")
            .exists(),
        "attempt 2 review transcript should exist"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(
        summary.contains("Transcript: .ralphterm/progress/plan-task-1-attempt-2.transcript"),
        "failed summary should link the failing attempt transcript:\n{summary}"
    );
    assert!(
        summary.contains(
            "Review transcript: .ralphterm/progress/plan-task-1-attempt-2-review.transcript"
        ),
        "failed summary should link the failing attempt review transcript:\n{summary}"
    );
    assert!(
        !summary.contains("Transcript: .ralphterm/progress/plan-task-1.transcript"),
        "failed summary must not link stale attempt-1 transcript:\n{summary}"
    );
    assert!(
        !summary.contains("Review transcript: .ralphterm/progress/plan-task-1-review.transcript"),
        "failed summary must not link stale attempt-1 review transcript:\n{summary}"
    );
}

#[test]
fn resume_after_review_failure_prompt_links_previous_review_transcript() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let first_output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm with failing review");

    assert!(
        !first_output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );
    let plan = fs::read_to_string(&plan_path).expect("read plan after review failure");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");
    assert!(
        repo.path
            .join(".ralphterm/progress/plan-task-1-attempt-2-review.transcript")
            .exists(),
        "failed review transcript should exist"
    );

    let second_output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("rerun ralphterm after review failure");

    assert!(
        second_output.status.success(),
        "ralphterm retry failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );

    let retry_prompt = fs::read_to_string(repo.path.join("fake-agent-last-prompt.txt"))
        .expect("read retry agent prompt");
    assert!(
        retry_prompt.contains("Previous run for this task failed"),
        "retry prompt should include resume context:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains(
            "- Previous transcript: .ralphterm/progress/plan-task-1-attempt-2.transcript"
        ),
        "retry prompt should point at the previous implementation transcript:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains(
            "- Previous validation output: .ralphterm/progress/plan-task-1-validation.txt"
        ),
        "retry prompt should point at the previous validation output:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains(
            "- Previous review transcript: .ralphterm/progress/plan-task-1-attempt-2-review.transcript"
        ),
        "retry prompt should point at the failed review transcript:\n{retry_prompt}"
    );
}

#[test]
fn review_fail_line_with_reason_triggers_retry() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("retry-after-review-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail-with-reason.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "reviewer REVIEW_FAIL with reason should retry and then pass\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("agent-count.txt")).expect("read agent count"),
        "2\n"
    );
    assert_eq!(
        fs::read_to_string(repo.path.join("review-count.txt")).expect("read review count"),
        "2\n"
    );
    let retry_prompt =
        fs::read_to_string(repo.path.join("agent-prompt-2.txt")).expect("read retry prompt");
    assert!(
        retry_prompt.contains("REVIEW_FAIL needs a better file"),
        "retry prompt should include reviewer reason:\n{retry_prompt}"
    );
}

#[test]
fn review_command_fail_blocks_marking_and_commit() {
    let repo = TempRepo::new();
    repo.init_git();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    repo.git(["add", "plan.md"]);
    repo.git(["commit", "-m", "docs: add test plan"]);
    let original_head = repo.git_output(["rev-parse", "HEAD"]);

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--review-command",
            fixture_path("review-fail.sh")
                .to_str()
                .expect("utf8 fixture path"),
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("REVIEW_FAIL"), "{diagnostics}");

    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");
    assert_eq!(repo.git_output(["rev-parse", "HEAD"]), original_head);

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    let validation_index = progress_log
        .find("validation result=passed")
        .expect("validation pass logged before review failure");
    let review_index = progress_log
        .find("review result=failed")
        .expect("review failure logged");
    assert!(
        validation_index < review_index,
        "validation should run before the failed review gate:\n{progress_log}"
    );
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
    );

    let summary = fs::read_to_string(repo.path.join(".ralphterm/progress/plan-summary.md"))
        .expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(
        summary.contains(".ralphterm/progress/plan-task-1-attempt-2-review.transcript"),
        "failed summary should link the final failed review attempt after the bounded retry:\n{summary}"
    );
}

#[test]
fn failed_rerun_removes_pre_existing_passed_summary() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    let summary_path = progress_dir.join("plan-summary.md");
    fs::write(
        &summary_path,
        "# Run Summary: plan.md\n\nResult: passed\n\n- stale passed run\n",
    )
    .expect("write stale summary");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = fs::read_to_string(&summary_path).expect("read failed run summary");
    assert!(summary.contains("Result: failed"), "{summary}");
    assert!(summary.contains("Task 1: Create first file"), "{summary}");
    assert!(summary.contains("Phase: validation"), "{summary}");
    assert!(
        summary.contains(".ralphterm/progress/plan-task-1.transcript"),
        "{summary}"
    );
    assert!(
        !summary.contains("stale passed run"),
        "failed summary should replace stale passed summary: {summary}"
    );
}

#[test]
fn resume_is_not_logged_when_marker_only_appears_in_task_title() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file with task_end number=1 result=failed in the title
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    fs::write(
        progress_dir.join("plan.log"),
        "timestamp=0 task_start number=1 title=Create first file with task_end number=1 result=failed in the title\n",
    )
    .expect("seed progress log");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log =
        fs::read_to_string(progress_dir.join("plan.log")).expect("read progress log");
    assert!(
        !progress_log.contains("resume number=1 previous_result=failed"),
        "resume should not be logged for marker text in a task title:\n{progress_log}"
    );
}

#[test]
fn resume_is_not_logged_when_latest_task_end_is_successful() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    fs::write(
        progress_dir.join("plan.log"),
        concat!(
            "timestamp=0 task_end number=1 result=failed\n",
            "timestamp=1 task_start number=1 title=Create first file\n",
            "timestamp=2 task_end number=1\n",
        ),
    )
    .expect("seed progress log");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log =
        fs::read_to_string(progress_dir.join("plan.log")).expect("read progress log");
    assert!(
        !progress_log.contains("resume number=1 previous_result=failed"),
        "resume should not be logged when the latest task_end succeeded:\n{progress_log}"
    );
}

#[test]
fn failed_task_resume_prompt_links_latest_failed_attempt_transcript() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f first.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");
    let progress_dir = repo.path.join(".ralphterm/progress");
    fs::create_dir_all(&progress_dir).expect("create progress dir");
    fs::write(
        progress_dir.join("plan.log"),
        "timestamp=1 task_start number=1 title=Create first file\n\
         timestamp=2 signal=COMPLETED transcript path=.ralphterm/progress/plan-task-1-attempt-1.transcript\n\
         timestamp=3 agent_retry attempt=2 reason=review_failed\n\
         timestamp=4 signal=COMPLETED transcript path=.ralphterm/progress/plan-task-1-attempt-2.transcript\n\
         timestamp=5 task_end number=1 result=failed\n",
    )
    .expect("seed progress log");

    let output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("rerun ralphterm");

    assert!(
        output.status.success(),
        "ralphterm retry failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let retry_prompt = fs::read_to_string(repo.path.join("fake-agent-last-prompt.txt"))
        .expect("read retry prompt");
    assert!(
        retry_prompt.contains(".ralphterm/progress/plan-task-1-attempt-2.transcript"),
        "resume prompt should link latest failed attempt transcript:\n{retry_prompt}"
    );
    assert!(
        !retry_prompt.contains(".ralphterm/progress/plan-task-1-attempt-1.transcript"),
        "resume prompt should not link stale attempt transcript:\n{retry_prompt}"
    );
}

#[test]
fn failed_task_resume_is_logged_before_retry_start_and_completes_task() {
    let repo = TempRepo::new();
    let plan_path = repo.path.join("plan.md");
    fs::write(
        &plan_path,
        r#"# Example plan

## Validation Commands
- `test -f missing.txt`

### Task 1: Create first file
- [ ] Write first.txt
"#,
    )
    .expect("write plan");

    let first_output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("run ralphterm");

    assert!(
        !first_output.status.success(),
        "ralphterm run unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read plan after failed run");
    fs::write(
        &plan_path,
        plan.replace("test -f missing.txt", "test -f first.txt"),
    )
    .expect("fix validation command");

    let second_output = Command::new(env!("CARGO_BIN_EXE_ralphterm"))
        .current_dir(&repo.path)
        .args([
            "run",
            plan_path.to_str().expect("utf8 plan path"),
            "--agent-command",
            fixture_path("fake-agent.sh")
                .to_str()
                .expect("utf8 fixture path"),
            "--no-commit",
        ])
        .stderr(Stdio::piped())
        .output()
        .expect("rerun ralphterm");

    assert!(
        second_output.status.success(),
        "ralphterm retry failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );

    let plan = fs::read_to_string(&plan_path).expect("read completed plan");
    assert!(plan.contains("- [x] Write first.txt"), "{plan}");

    let retry_prompt = fs::read_to_string(repo.path.join("fake-agent-last-prompt.txt"))
        .expect("read retry agent prompt");
    assert!(
        retry_prompt.contains("Previous run for this task failed"),
        "retry prompt should include resume context:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains(".ralphterm/progress/plan-task-1-attempt-1.transcript"),
        "retry prompt should point at the previous transcript:\n{retry_prompt}"
    );
    assert!(
        retry_prompt.contains(".ralphterm/progress/plan-task-1-validation.txt"),
        "retry prompt should point at the failed validation output:\n{retry_prompt}"
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    let first_task_start = progress_log
        .find("task_start number=1 title=Create first file")
        .expect("first task_start logged");
    let resume = progress_log
        .find("resume number=1 previous_result=failed")
        .expect("resume logged");
    let second_task_start = progress_log[resume..]
        .find("task_start number=1 title=Create first file")
        .map(|index| index + resume)
        .expect("second task_start logged after resume");
    assert!(
        first_task_start < resume && resume < second_task_start,
        "resume should be between failed run and retry task_start:\n{progress_log}"
    );
    assert!(
        progress_log.contains("validation result=passed"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("commit no_commit=true"),
        "{progress_log}"
    );
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_with_test_timeout(mut command: Command, timeout: Duration) -> Output {
    let mut child = command.spawn().expect("spawn ralphterm");
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().expect("poll ralphterm").is_some() {
            return child.wait_with_output().expect("collect ralphterm output");
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out ralphterm test command");
            let output = child
                .wait_with_output()
                .expect("collect killed ralphterm output");
            panic!(
                "ralphterm test command exceeded {timeout:?}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ralphterm-run-plan-{unique}"));
        fs::create_dir(&path).expect("create temp repo");
        Self { path }
    }

    fn init_git(&self) {
        self.git(["init"]);
        self.git(["config", "user.email", "test@example.invalid"]);
        self.git(["config", "user.name", "RalphTerm Test"]);
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git stdout utf8")
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
