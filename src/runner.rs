use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::plan::{parse_plan, Task};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub agent_command: Option<String>,
    pub review_command: Option<String>,
    pub require_review: bool,
    pub no_commit: bool,
    pub dry_run: bool,
}

pub fn run_plan(options: RunOptions) -> Result<String> {
    let input = fs::read_to_string(&options.plan_path)
        .with_context(|| format!("read plan {}", options.plan_path.display()))?;
    let mut plan_text = input;
    let plan = parse_plan(&plan_text).context("parse plan")?;
    let pending = plan.pending_tasks();
    let plan_name = options
        .plan_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plan");

    let mut output = format!("Executing {plan_name}\n");
    if pending.is_empty() {
        output.push_str("No pending tasks.\n");
        return Ok(output);
    }

    if options.dry_run {
        return Ok(describe_dry_run(
            plan_name,
            &plan.validation_commands,
            &pending,
        ));
    }

    let agent_command = options
        .agent_command
        .unwrap_or_else(|| "claude".to_string());
    if options.require_review && options.review_command.is_none() {
        bail!("--require-review needs --review-command");
    }
    let plan_slug = plan_slug(&options.plan_path);
    remove_stale_run_summary(&plan_slug)?;
    let run_baseline_revision = if options.no_commit {
        None
    } else {
        git_head_revision()
    };
    let run_baseline_paths = if options.no_commit {
        git_status_paths().unwrap_or_default()
    } else {
        BTreeSet::new()
    };
    let mut executed_tasks = Vec::new();

    for task in pending {
        let progress = ProgressPaths::new(&plan_slug, task.number)?;
        if last_task_end_failed(&progress.log_path, task.number)? {
            append_progress(
                &progress.log_path,
                &format!("resume number={} previous_result=failed", task.number),
            )?;
        }
        append_progress(
            &progress.log_path,
            &format!("task_start number={} title={}", task.number, task.title),
        )?;
        let baseline_paths = if options.no_commit {
            BTreeSet::new()
        } else {
            git_status_paths().context("snapshot git status before task")?
        };
        output.push_str(&format!("Task {}: {}\n", task.number, task.title));
        let prompt = build_task_prompt(plan_name, task, &plan.validation_commands);
        let agent_run = run_agent_command(&agent_command, &prompt)
            .with_context(|| format!("run agent for task {}", task.number))?;
        let transcript = agent_run.transcript;
        fs::write(&progress.transcript_path, &transcript)
            .with_context(|| format!("write transcript {}", progress.transcript_path.display()))?;
        append_progress(
            &progress.log_path,
            &format!(
                "signal={} transcript path={}",
                completion_signal(&transcript, &prompt),
                progress.transcript_display
            ),
        )?;
        output.push_str(&transcript);
        if !transcript.ends_with('\n') {
            output.push('\n');
        }
        if agent_run.exit_code != 0 {
            append_progress(
                &progress.log_path,
                &format!("task_end number={} result=failed", task.number),
            )?;
            bail!(
                "agent command exited with {} for task {}",
                agent_run.exit_code,
                task.number
            );
        }
        let validation_output = match run_validation_commands(&plan.validation_commands) {
            Ok(validation_output) => {
                output.push_str(&validation_output);
                validation_output
            }
            Err(err) => {
                append_progress(&progress.log_path, "validation result=failed")?;
                append_progress(
                    &progress.log_path,
                    &format!("task_end number={} result=failed", task.number),
                )?;
                return Err(err);
            }
        };
        append_progress(&progress.log_path, "validation result=passed")?;
        if let Some(review_command) = options.review_command.as_deref() {
            match run_review_command(
                review_command,
                plan_name,
                task,
                &transcript,
                &validation_output,
                &progress,
            ) {
                Ok(review_output) => {
                    output.push_str(&review_output);
                    append_progress(&progress.log_path, "review result=passed")?;
                }
                Err(err) => {
                    append_progress(&progress.log_path, "review result=failed")?;
                    append_progress(
                        &progress.log_path,
                        &format!("task_end number={} result=failed", task.number),
                    )?;
                    return Err(err);
                }
            }
        } else {
            output.push_str("Review: skipped\n");
            append_progress(&progress.log_path, "review result=skipped")?;
        }
        plan_text = crate::plan::mark_task_complete(&plan_text, task.number)
            .with_context(|| format!("mark task {} complete", task.number))?;
        fs::write(&options.plan_path, &plan_text)
            .with_context(|| format!("write plan {}", options.plan_path.display()))?;
        output.push_str(&format!("Marked task {} complete\n", task.number));
        if !options.no_commit {
            let commit = commit_task(&task.title, &baseline_paths)
                .with_context(|| format!("commit task {}", task.number))?;
            append_progress(&progress.log_path, &format!("commit hash={commit}"))?;
            output.push_str(&format!("Committed {commit}\n"));
        } else {
            append_progress(&progress.log_path, "commit no_commit=true")?;
        }
        append_progress(
            &progress.log_path,
            &format!("task_end number={}", task.number),
        )?;
        executed_tasks.push(ExecutedTask {
            number: task.number,
            title: task.title.clone(),
            transcript_display: progress.transcript_display,
        });
    }

    write_run_summary(plan_name, &plan_slug, &executed_tasks)?;
    write_run_diff_patch(
        &plan_slug,
        options.no_commit,
        run_baseline_revision.as_deref(),
        &run_baseline_paths,
    )?;

    Ok(output)
}

pub fn run_smoke(agent_command: &str) -> Result<String> {
    let prompt = "RalphTerm PTY smoke check. Print COMPLETED and exit after a minimal response.";
    let agent_run = run_agent_command_with_timeout(agent_command, prompt, smoke_timeout())
        .context("run smoke agent")?;
    let mut output = format!("Smoke: {agent_command}\n");
    output.push_str(&agent_run.transcript);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let signal = completion_signal(&agent_run.transcript, prompt);
    output.push_str(&format!("Signal: {signal}\n"));
    if agent_run.timed_out {
        bail!("smoke timed out after {:?}\n{output}", smoke_timeout());
    }
    if agent_run.exit_code != 0 {
        bail!(
            "agent command exited with {} during smoke\n{output}",
            agent_run.exit_code,
        );
    }
    if signal != "COMPLETED" {
        bail!("smoke transcript did not contain COMPLETED signal\n{output}");
    }
    Ok(output)
}

fn smoke_timeout() -> Duration {
    const DEFAULT_SMOKE_TIMEOUT_MS: u64 = 30_000;
    let timeout_ms = std::env::var("RALPHTERM_SMOKE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SMOKE_TIMEOUT_MS);
    Duration::from_millis(timeout_ms)
}

fn describe_dry_run(plan_name: &str, validation_commands: &[String], pending: &[&Task]) -> String {
    let mut output = format!("Dry run: {plan_name}\n");
    if validation_commands.is_empty() {
        output.push_str("Validation: none\n");
    } else {
        for command in validation_commands {
            output.push_str(&format!("Validation: {command}\n"));
        }
    }
    for task in pending {
        output.push_str(&format!("Task {}: {}\n", task.number, task.title));
    }
    output
}

struct ProgressPaths {
    log_path: PathBuf,
    transcript_path: PathBuf,
    review_transcript_path: PathBuf,
    transcript_display: String,
    review_transcript_display: String,
}

struct ExecutedTask {
    number: usize,
    title: String,
    transcript_display: String,
}

struct AgentRun {
    transcript: String,
    exit_code: u32,
    timed_out: bool,
}

impl ProgressPaths {
    fn new(plan_slug: &str, task_number: usize) -> Result<Self> {
        ensure_ralphterm_git_excluded()?;
        let progress_dir = PathBuf::from(".ralphterm").join("progress");
        fs::create_dir_all(&progress_dir).context("create progress directory")?;
        let log_path = progress_dir.join(format!("{plan_slug}.log"));
        let transcript_path =
            progress_dir.join(format!("{plan_slug}-task-{task_number}.transcript"));
        let review_transcript_path =
            progress_dir.join(format!("{plan_slug}-task-{task_number}-review.transcript"));
        let transcript_display = transcript_path.to_string_lossy().into_owned();
        let review_transcript_display = review_transcript_path.to_string_lossy().into_owned();
        Ok(Self {
            log_path,
            transcript_path,
            review_transcript_path,
            transcript_display,
            review_transcript_display,
        })
    }
}

fn append_progress(path: &Path, event: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open progress log {}", path.display()))?;
    writeln!(file, "timestamp={} {event}", timestamp()).context("write progress log")
}

fn run_summary_path(plan_slug: &str) -> PathBuf {
    PathBuf::from(".ralphterm")
        .join("progress")
        .join(format!("{plan_slug}-summary.md"))
}

fn remove_stale_run_summary(plan_slug: &str) -> Result<()> {
    let summary_path = run_summary_path(plan_slug);
    match fs::remove_file(&summary_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("remove stale run summary {}", summary_path.display()))
        }
    }
}

fn write_run_summary(plan_name: &str, plan_slug: &str, tasks: &[ExecutedTask]) -> Result<()> {
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let summary_path = run_summary_path(plan_slug);
    let mut summary = format!("# Run Summary: {plan_name}\n\nResult: passed\n\n");
    for task in tasks {
        summary.push_str(&format!(
            "- Task {}: {} — passed\n  - Transcript: {}\n",
            task.number, task.title, task.transcript_display
        ));
    }
    fs::write(&summary_path, summary)
        .with_context(|| format!("write run summary {}", summary_path.display()))
}

fn write_run_diff_patch(
    plan_slug: &str,
    no_commit: bool,
    baseline_revision: Option<&str>,
    baseline_paths: &BTreeSet<String>,
) -> Result<()> {
    ensure_ralphterm_git_excluded()?;
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let diff_path = progress_dir.join(format!("{plan_slug}-diff.patch"));
    let patch = if no_commit {
        working_tree_diff_patch(baseline_paths).unwrap_or_default()
    } else if let Some(baseline_revision) = baseline_revision {
        git_diff_patch(&[baseline_revision, "HEAD"]).unwrap_or_default()
    } else {
        String::new()
    };
    fs::write(&diff_path, patch)
        .with_context(|| format!("write run diff patch {}", diff_path.display()))
}

fn working_tree_diff_patch(baseline_paths: &BTreeSet<String>) -> Result<String> {
    let mut patch = git_diff_patch(&[])?;
    for path in git_untracked_paths()? {
        if baseline_paths.contains(&path) || is_ralphterm_artifact(&path) {
            continue;
        }
        patch.push_str(&git_no_index_new_file_patch(&path)?);
    }
    Ok(patch)
}

fn git_diff_patch(revisions: &[&str]) -> Result<String> {
    let mut args = vec!["diff", "--binary", "--"];
    if !revisions.is_empty() {
        args = vec!["diff", "--binary"];
        args.extend_from_slice(revisions);
        args.push("--");
    }
    run_git_allow_exit_codes(&args, &[0])
}

fn git_no_index_new_file_patch(path: &str) -> Result<String> {
    run_git_allow_exit_codes(
        &["diff", "--binary", "--no-index", "--", "/dev/null", path],
        &[0, 1],
    )
}

fn git_untracked_paths() -> Result<Vec<String>> {
    let output = run_git(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn git_head_revision() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn last_task_end_failed(path: &Path, task_number: usize) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let log = fs::read_to_string(path)
        .with_context(|| format!("read progress log {}", path.display()))?;
    let task_end_prefix = format!("task_end number={task_number}");
    let mut last_failed = None;
    for line in log.lines() {
        let Some(event) = progress_event(line) else {
            continue;
        };
        if event_starts_with_token(event, &task_end_prefix) {
            last_failed = Some(event.contains("result=failed"));
        }
    }
    Ok(last_failed.unwrap_or(false))
}

fn progress_event(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("timestamp=")?;
    rest.split_once(' ').map(|(_, event)| event)
}

fn event_starts_with_token(event: &str, token: &str) -> bool {
    let Some(rest) = event.strip_prefix(token) else {
        return false;
    };
    rest.is_empty() || rest.starts_with(' ')
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    seconds.to_string()
}

fn completion_signal(transcript: &str, prompt: &str) -> &'static str {
    let output = transcript_without_prompt_echo(transcript, prompt);
    if output.contains("COMPLETED") {
        "COMPLETED"
    } else {
        "NONE"
    }
}

fn transcript_without_prompt_echo(transcript: &str, prompt: &str) -> String {
    let mut normalized = transcript.replace("\r\n", "\n").replace('\r', "\n");
    let prompt = prompt.replace("\r\n", "\n").replace('\r', "\n");
    if let Some(start) = normalized.find(&prompt) {
        let end = start + prompt.len();
        normalized.replace_range(start..end, "");
    }
    normalized
}

fn plan_slug(plan_path: &Path) -> String {
    let raw = plan_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("plan");
    let slug: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug
    }
}

fn commit_task(title: &str, baseline_paths: &BTreeSet<String>) -> Result<String> {
    let current_paths = git_status_paths().context("snapshot git status after task")?;
    let paths_to_stage: Vec<&str> = current_paths
        .difference(baseline_paths)
        .filter(|path| !is_ralphterm_artifact(path))
        .map(String::as_str)
        .collect();
    if paths_to_stage.is_empty() {
        bail!("task produced no git changes to commit");
    }
    run_git_with_paths(&["add", "--"], &paths_to_stage)?;
    run_git_with_paths(
        &["commit", "-m", &format!("task: {title}"), "--"],
        &paths_to_stage,
    )?;
    let hash = run_git(&["rev-parse", "--short", "HEAD"])?;
    Ok(hash.trim().to_string())
}

fn git_status_paths() -> Result<BTreeSet<String>> {
    let output = run_git(&["status", "--porcelain", "-z"])?;
    let mut paths = BTreeSet::new();
    for entry in output.split('\0').filter(|entry| !entry.is_empty()) {
        if entry.len() >= 4 {
            paths.insert(entry[3..].to_string());
        }
    }
    Ok(paths)
}

fn is_ralphterm_artifact(path: &str) -> bool {
    path == ".ralphterm" || path.starts_with(".ralphterm/")
}

fn ensure_ralphterm_git_excluded() -> Result<()> {
    let git_path = Command::new("git")
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output();
    let Ok(output) = git_path else {
        return Ok(());
    };
    if !output.status.success() {
        return Ok(());
    }

    let exclude_path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if exclude_path.as_os_str().is_empty() {
        return Ok(());
    }
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == ".ralphterm/") {
        return Ok(());
    }

    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .with_context(|| format!("open {}", exclude_path.display()))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(file).with_context(|| format!("write {}", exclude_path.display()))?;
    }
    writeln!(file, ".ralphterm/").with_context(|| format!("write {}", exclude_path.display()))
}

fn run_git(args: &[&str]) -> Result<String> {
    run_git_with_paths(args, &[])
}

fn run_git_allow_exit_codes(args: &[&str], allowed_exit_codes: &[i32]) -> Result<String> {
    let result = Command::new("git").args(args).output().context("run git")?;
    let exit_code = result.status.code().unwrap_or(-1);
    if !allowed_exit_codes.contains(&exit_code) {
        bail!(
            "git command failed with {}\nstdout:\n{}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn run_git_with_paths(args: &[&str], paths: &[&str]) -> Result<String> {
    let result = Command::new("git")
        .args(args)
        .args(paths)
        .output()
        .context("run git")?;
    if !result.status.success() {
        bail!(
            "git command failed with {}\nstdout:\n{}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn build_task_prompt(plan_name: &str, task: &Task, validation_commands: &[String]) -> String {
    let mut prompt = format!(
        "You are executing one task from {plan_name}.\n\nTask {}: {}\n\n{}",
        task.number, task.title, task.body
    );
    if !validation_commands.is_empty() {
        prompt.push_str("\nValidation commands after this task:\n");
        for command in validation_commands {
            prompt.push_str("- ");
            prompt.push_str(command);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nWhen the task is complete, print COMPLETED.\n");
    prompt
}

fn run_validation_commands(commands: &[String]) -> Result<String> {
    let mut output = String::new();
    for command in commands {
        output.push_str(&format!("Validation: {command}\n"));
        let result = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .output()
            .with_context(|| format!("run validation command `{command}`"))?;
        if result.status.success() {
            output.push_str("Validation passed\n");
        } else {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            bail!(
                "validation command failed `{command}` with {}\nstdout:\n{}\nstderr:\n{}",
                result.status,
                stdout,
                stderr
            );
        }
    }
    Ok(output)
}

fn run_review_command(
    review_command: &str,
    plan_name: &str,
    task: &Task,
    agent_transcript: &str,
    validation_output: &str,
    progress: &ProgressPaths,
) -> Result<String> {
    let prompt = build_review_prompt(
        plan_name,
        task,
        agent_transcript,
        validation_output,
        &git_state_for_review(),
    );
    let review_run = run_agent_command(review_command, &prompt)
        .with_context(|| format!("run review for task {}", task.number))?;
    fs::write(&progress.review_transcript_path, &review_run.transcript).with_context(|| {
        format!(
            "write review transcript {}",
            progress.review_transcript_path.display()
        )
    })?;
    append_progress(
        &progress.log_path,
        &format!(
            "review transcript path={}",
            progress.review_transcript_display
        ),
    )?;

    let mut output = review_run.transcript;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    if review_run.exit_code != 0 {
        bail!(
            "review command exited with {} for task {}\n{}",
            review_run.exit_code,
            task.number,
            output
        );
    }
    if !review_output_passed(&output, &prompt) {
        bail!("review failed for task {}\n{}", task.number, output);
    }
    output.push_str("Review passed\n");
    Ok(output)
}

fn build_review_prompt(
    plan_name: &str,
    task: &Task,
    agent_transcript: &str,
    validation_output: &str,
    git_diff: &str,
) -> String {
    format!(
        "You are independently reviewing one RalphTerm plan task from {plan_name}.\n\nTask {}: {}\n\nTask body:\n{}\n\nAgent transcript:\n{}\n\nValidation output:\n{}\n\nCurrent git diff:\n{}\n\n{}\n",
        task.number,
        task.title,
        task.body,
        agent_transcript,
        validation_output,
        git_diff,
        review_instruction()
    )
}

fn review_instruction() -> &'static str {
    "Print REVIEW_PASS only if the task matches the spec and validation is trustworthy. Print REVIEW_FAIL with the reason otherwise."
}

fn review_output_passed(transcript: &str, _prompt: &str) -> bool {
    let normalized = transcript.replace("\r\n", "\n").replace('\r', "\n");
    let reviewer_output = normalized
        .rfind(review_instruction())
        .map(|start| &normalized[start + review_instruction().len()..])
        .unwrap_or(normalized.as_str());

    reviewer_output
        .lines()
        .filter_map(|line| match line.trim() {
            "REVIEW_PASS" => Some(true),
            "REVIEW_FAIL" => Some(false),
            _ => None,
        })
        .next_back()
        .unwrap_or(false)
}

fn git_state_for_review() -> String {
    let mut state = String::new();

    append_git_output(&mut state, "Unstaged diff", &["diff", "--"]);
    append_git_output(&mut state, "Staged diff", &["diff", "--cached", "--"]);

    if let Ok(output) = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()
    {
        if output.status.success() {
            let untracked: String = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|path| !is_ralphterm_artifact(path))
                .map(|path| format!("{path}\n"))
                .collect();
            if !untracked.trim().is_empty() {
                if !state.ends_with('\n') && !state.is_empty() {
                    state.push('\n');
                }
                state.push_str("Untracked files:\n");
                state.push_str(&untracked);
            }
        }
    }

    state
}

fn append_git_output(state: &mut String, label: &str, args: &[&str]) {
    match Command::new("git").args(args).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                if !state.ends_with('\n') && !state.is_empty() {
                    state.push('\n');
                }
                state.push_str(label);
                state.push_str(":\n");
                state.push_str(&stdout);
            }
        }
        Ok(output) => {
            if !state.ends_with('\n') && !state.is_empty() {
                state.push('\n');
            }
            state.push_str(label);
            state.push_str(" unavailable:\n");
            state.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        Err(err) => {
            if !state.ends_with('\n') && !state.is_empty() {
                state.push('\n');
            }
            state.push_str(&format!("{label} unavailable: {err}\n"));
        }
    }
}

fn run_agent_command(agent_command: &str, prompt: &str) -> Result<AgentRun> {
    let mut child = spawn_agent_command(agent_command, prompt)?;
    let mut reader = child
        .master
        .try_clone_reader()
        .context("clone pty reader")?;
    let mut transcript = String::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => transcript.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(err) => bail!("read pty: {err}"),
        }
    }

    let status = child.child.wait().context("wait for agent command")?;
    Ok(AgentRun {
        transcript,
        exit_code: status.exit_code(),
        timed_out: false,
    })
}

fn run_agent_command_with_timeout(
    agent_command: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<AgentRun> {
    enum ReaderEvent {
        Chunk(String),
        Error(String),
        Done,
    }

    let mut agent = spawn_agent_command(agent_command, prompt)?;
    let mut reader = agent
        .master
        .try_clone_reader()
        .context("clone pty reader")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(ReaderEvent::Done);
                    break;
                }
                Ok(n) => {
                    let _ = tx.send(ReaderEvent::Chunk(
                        String::from_utf8_lossy(&buf[..n]).into_owned(),
                    ));
                }
                Err(err) => {
                    let _ = tx.send(ReaderEvent::Error(err.to_string()));
                    break;
                }
            }
        }
    });

    let deadline = Instant::now() + timeout;
    let mut transcript = String::new();
    let mut reader_done = false;
    let mut read_error = None;
    let mut timed_out = false;
    let status = loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                ReaderEvent::Chunk(chunk) => transcript.push_str(&chunk),
                ReaderEvent::Error(err) => {
                    read_error = Some(err);
                    reader_done = true;
                }
                ReaderEvent::Done => reader_done = true,
            }
        }

        if let Some(status) = agent.child.try_wait().context("poll agent command")? {
            break status;
        }

        if Instant::now() >= deadline {
            timed_out = true;
            agent.child.kill().context("kill timed out smoke agent")?;
            break agent.child.wait().context("wait for killed smoke agent")?;
        }

        thread::sleep(Duration::from_millis(10));
    };

    let drain_deadline = Instant::now() + Duration::from_millis(200);
    while !reader_done && Instant::now() < drain_deadline {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(ReaderEvent::Chunk(chunk)) => transcript.push_str(&chunk),
            Ok(ReaderEvent::Error(err)) => {
                read_error = Some(err);
                reader_done = true;
            }
            Ok(ReaderEvent::Done) => reader_done = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(err) = read_error.filter(|_| !timed_out) {
        bail!("read pty: {err}");
    }

    Ok(AgentRun {
        transcript,
        exit_code: status.exit_code(),
        timed_out,
    })
}

struct SpawnedAgent {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

fn spawn_agent_command(agent_command: &str, prompt: &str) -> Result<SpawnedAgent> {
    let mut parts = shlex::split(agent_command)
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid agent command"))?;
    let command = parts.remove(0);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open pty")?;

    let mut cmd = CommandBuilder::new(command);
    for arg in parts {
        cmd.arg(arg);
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn agent command")?;
    drop(pair.slave);

    {
        let mut writer = pair.master.take_writer().context("take pty writer")?;
        writer
            .write_all(prompt.as_bytes())
            .context("write prompt")?;
        writer.write_all(b"\n").context("write prompt newline")?;
        writer.flush().context("flush prompt")?;
    }

    Ok(SpawnedAgent {
        child,
        master: pair.master,
    })
}
