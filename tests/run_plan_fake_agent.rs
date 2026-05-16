use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
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
    assert!(progress_log.contains("task_end number=1"), "{progress_log}");

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
        output.status.success(),
        "ralphterm run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let progress_log = fs::read_to_string(repo.path.join(".ralphterm/progress/plan.log"))
        .expect("read progress log");
    assert!(progress_log.contains("signal=NONE"), "{progress_log}");
    assert!(!progress_log.contains("signal=COMPLETED"), "{progress_log}");
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
    assert!(
        !repo
            .path
            .join(".ralphterm/progress/plan-summary.md")
            .exists(),
        "failed run should not write passed summary"
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
    assert!(
        progress_log.contains("review result=passed"),
        "{progress_log}"
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
        review_prompt.contains("\nfirst.txt\n"),
        "review prompt should expose newly-created files for review:\n{review_prompt}"
    );
}

#[test]
fn require_review_without_review_command_refuses_to_run_agent() {
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
        "agent should not run when review is required without a review command"
    );
    let plan = fs::read_to_string(&plan_path).expect("read plan");
    assert!(plan.contains("- [ ] Write first.txt"), "{plan}");

    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostics.contains("--require-review needs --review-command"),
        "{diagnostics}"
    );
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
    assert!(
        progress_log.contains("validation result=passed"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("review result=failed"),
        "{progress_log}"
    );
    assert!(
        progress_log.contains("task_end number=1 result=failed"),
        "{progress_log}"
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
    assert!(
        !summary_path.exists(),
        "failed rerun should remove stale passed summary"
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
