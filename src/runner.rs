use std::{
    collections::{BTreeMap, BTreeSet},
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

#[derive(Debug, Default)]
struct NoCommitBaseline {
    paths: BTreeSet<String>,
    tracked_file_contents: BTreeMap<String, Vec<u8>>,
    tracked_non_file_paths: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub agent_command: Option<String>,
    pub review_command: Option<String>,
    pub require_review: bool,
    pub max_review_retries: usize,
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

    let review_command = options.review_command.clone();
    let agent_command = options
        .agent_command
        .clone()
        .unwrap_or_else(|| "claude".to_string());
    if options.require_review && review_command.is_none() {
        bail!("--require-review needs --review-command or --review-agent");
    }
    if let Some(review_command) = review_command.as_deref() {
        if parse_agent_command(review_command)? == parse_agent_command(&agent_command)? {
            bail!("independent review command must differ from agent command");
        }
    }

    let mut output = format!("Executing {plan_name}\n");
    if pending.is_empty() {
        output.push_str("No pending tasks.\n");
        if !options.dry_run {
            write_run_diff_patch(
                &plan_slug(&options.plan_path),
                false,
                None,
                &NoCommitBaseline::default(),
            )?;
        }
        return Ok(output);
    }

    if options.dry_run {
        return Ok(describe_dry_run(
            plan_name,
            &plan.validation_commands,
            &pending,
            review_command.as_deref(),
        ));
    }

    validate_interactive_agent_command(&agent_command)?;
    if let Some(review_command) = options.review_command.as_deref() {
        validate_interactive_agent_command(review_command)?;
    }
    let plan_slug = plan_slug(&options.plan_path);
    remove_stale_run_summary(&plan_slug)?;
    let run_baseline_revision = if options.no_commit {
        None
    } else {
        git_head_revision()
    };
    let run_baseline_paths = if options.no_commit {
        git_run_baseline().context("snapshot run baseline git status")?
    } else {
        NoCommitBaseline::default()
    };
    let mut executed_tasks = Vec::new();

    for task in pending {
        let progress = ProgressPaths::new(&plan_slug, task.number)?;
        let last_task_end = last_task_end_status(&progress.log_path, task.number)?;
        let resume_context = if last_task_end.failed {
            append_progress(
                &progress.log_path,
                &format!("resume number={} previous_result=failed", task.number),
            )?;
            let previous_attempt = progress.attempt(1);
            let transcript_display = last_task_end
                .transcript_display
                .filter(|path| path != &progress.transcript_display)
                .unwrap_or(previous_attempt.transcript_display);
            Some(ResumeContext {
                transcript_display,
                validation_output_display: progress.validation_output_display.clone(),
                review_transcript_display: last_task_end.review_transcript_display,
            })
        } else {
            None
        };
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
        let mut review_feedback = None;
        let mut attempt = 1;
        let final_transcript_display: String;
        let final_review_transcript_display: String;
        let (_transcript, _validation_output) = loop {
            let attempt_progress = progress.attempt(attempt);
            if attempt > 1 {
                append_progress(
                    &progress.log_path,
                    &format!("agent_retry attempt={attempt} reason=review_failed"),
                )?;
            }
            let prompt = build_task_prompt(
                plan_name,
                task,
                &plan.validation_commands,
                review_feedback.as_deref(),
                resume_context.as_ref(),
            );
            let timeout = agent_timeout();
            let agent_run = match run_agent_command_with_timeout(&agent_command, &prompt, timeout)
                .with_context(|| format!("run agent for task {}", task.number))
            {
                Ok(agent_run) => agent_run,
                Err(err) => {
                    append_progress(
                        &progress.log_path,
                        &format!("task_end number={} result=failed", task.number),
                    )?;
                    let summary_result = write_failed_run_summary(
                        plan_name,
                        &plan_slug,
                        &executed_tasks,
                        task,
                        "agent execution",
                        &format!("{err:#}"),
                        &progress,
                        Some(&attempt_progress),
                        false,
                        false,
                    );
                    return Err(failed_run_error(err, summary_result));
                }
            };
            let transcript = agent_run.transcript;
            fs::write(&attempt_progress.transcript_path, &transcript).with_context(|| {
                format!(
                    "write transcript {}",
                    attempt_progress.transcript_path.display()
                )
            })?;
            if attempt == 1 {
                fs::write(&progress.transcript_path, &transcript).with_context(|| {
                    format!("write transcript {}", progress.transcript_path.display())
                })?;
            }
            let current_transcript_display = if attempt == 1 {
                progress.transcript_display.clone()
            } else {
                attempt_progress.transcript_display.clone()
            };
            let signal = completion_signal(&transcript, &prompt);
            append_progress(
                &progress.log_path,
                &format!(
                    "signal={} transcript path={}",
                    signal.as_str(),
                    current_transcript_display
                ),
            )?;
            output.push_str(&transcript);
            if !transcript.ends_with('\n') {
                output.push('\n');
            }
            if agent_run.timed_out {
                append_progress(
                    &progress.log_path,
                    &format!("task_end number={} result=failed", task.number),
                )?;
                let detail = format!("agent command timed out after {timeout:?}\n{transcript}");
                let summary_result = write_failed_run_summary(
                    plan_name,
                    &plan_slug,
                    &executed_tasks,
                    task,
                    "agent execution",
                    &detail,
                    &progress,
                    Some(&attempt_progress),
                    false,
                    false,
                );
                let err = anyhow::anyhow!(
                    "agent command timed out after {:?} for task {}\n{}",
                    timeout,
                    task.number,
                    transcript
                );
                return Err(failed_run_error(err, summary_result));
            }
            if agent_run.exit_code != 0 {
                append_progress(
                    &progress.log_path,
                    &format!("task_end number={} result=failed", task.number),
                )?;
                let summary_result = write_failed_run_summary(
                    plan_name,
                    &plan_slug,
                    &executed_tasks,
                    task,
                    "agent execution",
                    &format!("agent command exited with {}", agent_run.exit_code),
                    &progress,
                    Some(&attempt_progress),
                    false,
                    false,
                );
                let err = anyhow::anyhow!(
                    "agent command exited with {} for task {}",
                    agent_run.exit_code,
                    task.number
                );
                return Err(failed_run_error(err, summary_result));
            }
            if signal != CompletionSignal::Completed {
                append_progress(
                    &progress.log_path,
                    &format!("task_end number={} result=failed", task.number),
                )?;
                let detail = format!(
                    "missing required COMPLETED signal from agent (detected signal={})",
                    signal.as_str()
                );
                let summary_result = write_failed_run_summary(
                    plan_name,
                    &plan_slug,
                    &executed_tasks,
                    task,
                    "agent completion",
                    &detail,
                    &progress,
                    Some(&attempt_progress),
                    false,
                    false,
                );
                let err = anyhow::anyhow!(
                    "agent for task {} did not emit required COMPLETED signal (detected signal={})",
                    task.number,
                    signal.as_str()
                );
                return Err(failed_run_error(err, summary_result));
            }
            let validation_output = match run_validation_commands(
                &plan.validation_commands,
                &progress.validation_output_path,
            ) {
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
                    let summary_result = write_failed_run_summary(
                        plan_name,
                        &plan_slug,
                        &executed_tasks,
                        task,
                        "validation",
                        &err.to_string(),
                        &progress,
                        Some(&attempt_progress),
                        true,
                        false,
                    );
                    return Err(failed_run_error(err, summary_result));
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
                    timeout,
                    &progress,
                    &attempt_progress,
                ) {
                    Ok(review_output) => {
                        output.push_str(&review_output);
                        append_progress(&progress.log_path, "review result=passed")?;
                        final_transcript_display = current_transcript_display;
                        final_review_transcript_display = if attempt == 1 {
                            progress.review_transcript_display.clone()
                        } else {
                            attempt_progress.review_transcript_display.clone()
                        };
                        break (transcript, validation_output);
                    }
                    Err(err) => {
                        append_progress(&progress.log_path, "review result=failed")?;
                        let feedback = err.to_string();
                        let review_retries_used = attempt - 1;
                        if err.explicit_fail() && review_retries_used < options.max_review_retries {
                            output.push_str(&format!(
                                "Review failed on attempt {attempt}; retrying implementation.\n"
                            ));
                            review_feedback = Some(feedback);
                            attempt += 1;
                            continue;
                        }
                        append_progress(
                            &progress.log_path,
                            &format!("task_end number={} result=failed", task.number),
                        )?;
                        let summary_result = write_failed_run_summary(
                            plan_name,
                            &plan_slug,
                            &executed_tasks,
                            task,
                            "review",
                            &err.to_string(),
                            &progress,
                            Some(&attempt_progress),
                            true,
                            true,
                        );
                        return Err(failed_run_error(err.into(), summary_result));
                    }
                }
            } else {
                output.push_str("Review: skipped\n");
                append_progress(&progress.log_path, "review result=skipped")?;
                final_transcript_display = current_transcript_display;
                final_review_transcript_display = progress.review_transcript_display.clone();
                break (transcript, validation_output);
            }
        };
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
            &format!("task_end number={} result=passed", task.number),
        )?;
        executed_tasks.push(ExecutedTask {
            number: task.number,
            title: task.title.clone(),
            transcript_display: final_transcript_display,
            validation_output_display: progress.validation_output_display,
            review_transcript_display: options
                .review_command
                .as_ref()
                .map(|_| final_review_transcript_display),
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
    validate_interactive_agent_command(agent_command)?;
    let prompt = "RalphTerm PTY smoke check. Print COMPLETED and exit after a minimal response.";
    let timeout = smoke_timeout();
    let agent_run = run_agent_command_with_timeout(agent_command, prompt, timeout)
        .context("run smoke agent")?;
    let mut output = format!("Smoke: {agent_command}\n");
    output.push_str(&agent_run.transcript);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let signal = completion_signal(&agent_run.transcript, prompt);
    output.push_str(&format!("Signal: {}\n", signal.as_str()));
    if agent_run.timed_out {
        bail!("smoke timed out after {timeout:?}\n{output}");
    }
    if agent_run.exit_code != 0 {
        bail!(
            "agent command exited with {} during smoke\n{output}",
            agent_run.exit_code,
        );
    }
    if signal != CompletionSignal::Completed {
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

fn agent_timeout() -> Duration {
    const DEFAULT_AGENT_TIMEOUT_MS: u64 = 30 * 60 * 1_000;
    let timeout_ms = std::env::var("RALPHTERM_AGENT_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AGENT_TIMEOUT_MS);
    Duration::from_millis(timeout_ms)
}

fn describe_dry_run(
    plan_name: &str,
    validation_commands: &[String],
    pending: &[&Task],
    review_command: Option<&str>,
) -> String {
    let mut output = format!("Dry run: {plan_name}\n");
    match review_command {
        Some(command) => output.push_str(&format!("Review: {command}\n")),
        None => output.push_str("Review: skipped\n"),
    }
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
    validation_output_path: PathBuf,
    transcript_display: String,
    review_transcript_display: String,
    validation_output_display: String,
}

struct ExecutedTask {
    number: usize,
    title: String,
    transcript_display: String,
    validation_output_display: String,
    review_transcript_display: Option<String>,
}

struct AttemptProgressPaths {
    transcript_path: PathBuf,
    review_transcript_path: PathBuf,
    legacy_review_transcript_path: Option<PathBuf>,
    transcript_display: String,
    review_transcript_display: String,
}

struct ResumeContext {
    transcript_display: String,
    validation_output_display: String,
    review_transcript_display: Option<String>,
}

struct AgentRun {
    transcript: String,
    exit_code: u32,
    timed_out: bool,
}

#[derive(Debug)]
struct ReviewCommandError {
    message: String,
    explicit_fail: bool,
}

impl ReviewCommandError {
    fn new(message: String, explicit_fail: bool) -> Self {
        Self {
            message,
            explicit_fail,
        }
    }

    fn explicit_fail(&self) -> bool {
        self.explicit_fail
    }
}

impl std::fmt::Display for ReviewCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReviewCommandError {}

impl From<anyhow::Error> for ReviewCommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(error.to_string(), false)
    }
}

impl From<std::io::Error> for ReviewCommandError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string(), false)
    }
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
        let validation_output_path =
            progress_dir.join(format!("{plan_slug}-task-{task_number}-validation.txt"));
        let transcript_display = transcript_path.to_string_lossy().into_owned();
        let review_transcript_display = review_transcript_path.to_string_lossy().into_owned();
        let validation_output_display = validation_output_path.to_string_lossy().into_owned();
        Ok(Self {
            log_path,
            transcript_path,
            review_transcript_path,
            validation_output_path,
            transcript_display,
            review_transcript_display,
            validation_output_display,
        })
    }

    fn attempt(&self, attempt: usize) -> AttemptProgressPaths {
        let transcript_stem = self
            .transcript_path
            .file_stem()
            .expect("progress transcript path has a file stem")
            .to_string_lossy();
        let transcript_path = self
            .transcript_path
            .with_file_name(format!("{transcript_stem}-attempt-{attempt}.transcript"));
        let review_transcript_path = self.review_transcript_path.with_file_name(format!(
            "{transcript_stem}-attempt-{attempt}-review.transcript"
        ));
        let transcript_display = transcript_path.to_string_lossy().into_owned();
        let review_transcript_display = review_transcript_path.to_string_lossy().into_owned();
        let legacy_review_transcript_path =
            (attempt == 1).then(|| self.review_transcript_path.clone());
        AttemptProgressPaths {
            transcript_path,
            review_transcript_path,
            legacy_review_transcript_path,
            transcript_display,
            review_transcript_display,
        }
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
            "- Task {}: {} — passed\n  - Transcript: {}\n  - Validation: {}\n",
            task.number, task.title, task.transcript_display, task.validation_output_display
        ));
        if let Some(review_transcript_display) = &task.review_transcript_display {
            summary.push_str(&format!(
                "  - Review transcript: {review_transcript_display}\n"
            ));
        }
    }
    fs::write(&summary_path, summary)
        .with_context(|| format!("write run summary {}", summary_path.display()))
}

#[allow(clippy::too_many_arguments)]
fn write_failed_run_summary(
    plan_name: &str,
    plan_slug: &str,
    passed_tasks: &[ExecutedTask],
    task: &Task,
    phase: &str,
    reason: &str,
    progress: &ProgressPaths,
    attempt_progress: Option<&AttemptProgressPaths>,
    link_validation_output: bool,
    link_review_transcript: bool,
) -> Result<()> {
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let summary_path = run_summary_path(plan_slug);
    let mut summary = format!("# Run Summary: {plan_name}\n\nResult: failed\n\n");
    for passed_task in passed_tasks {
        summary.push_str(&format!(
            "- Task {}: {} — passed\n  - Transcript: {}\n  - Validation: {}\n",
            passed_task.number,
            passed_task.title,
            passed_task.transcript_display,
            passed_task.validation_output_display
        ));
        if let Some(review_transcript_display) = &passed_task.review_transcript_display {
            summary.push_str(&format!(
                "  - Review transcript: {review_transcript_display}\n"
            ));
        }
    }
    summary.push_str(&format!(
        "- Task {}: {} — failed\n  - Phase: {phase}\n  - Reason: {reason}\n",
        task.number, task.title
    ));
    let transcript_display = attempt_progress
        .and_then(|attempt| {
            if attempt.legacy_review_transcript_path.is_some() {
                progress
                    .transcript_path
                    .exists()
                    .then_some(progress.transcript_display.as_str())
            } else {
                attempt
                    .transcript_path
                    .exists()
                    .then_some(attempt.transcript_display.as_str())
            }
        })
        .or_else(|| {
            progress
                .transcript_path
                .exists()
                .then_some(progress.transcript_display.as_str())
        });
    if let Some(transcript_display) = transcript_display {
        summary.push_str(&format!("  - Transcript: {transcript_display}\n"));
    }
    if link_validation_output && progress.validation_output_path.exists() {
        summary.push_str(&format!(
            "  - Validation: {}\n",
            progress.validation_output_display
        ));
    }
    if link_review_transcript {
        let review_transcript_display = attempt_progress
            .and_then(|attempt| {
                if let Some(legacy_review_transcript_path) = &attempt.legacy_review_transcript_path
                {
                    legacy_review_transcript_path
                        .exists()
                        .then_some(progress.review_transcript_display.as_str())
                } else {
                    attempt
                        .review_transcript_path
                        .exists()
                        .then_some(attempt.review_transcript_display.as_str())
                }
            })
            .or_else(|| {
                progress
                    .review_transcript_path
                    .exists()
                    .then_some(progress.review_transcript_display.as_str())
            });
        if let Some(review_transcript_display) = review_transcript_display {
            summary.push_str(&format!(
                "  - Review transcript: {review_transcript_display}\n"
            ));
        }
    }
    fs::write(&summary_path, summary)
        .with_context(|| format!("write failed run summary {}", summary_path.display()))
}

fn failed_run_error(original: anyhow::Error, summary_result: Result<()>) -> anyhow::Error {
    match summary_result {
        Ok(()) => original,
        Err(summary_err) => anyhow::anyhow!(
            "{original}; additionally failed to write failed run summary: {summary_err}"
        ),
    }
}

fn write_run_diff_patch(
    plan_slug: &str,
    no_commit: bool,
    baseline_revision: Option<&str>,
    baseline: &NoCommitBaseline,
) -> Result<()> {
    ensure_ralphterm_git_excluded()?;
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let diff_path = progress_dir.join(format!("{plan_slug}-diff.patch"));
    let patch = if !git_inside_work_tree() {
        String::new()
    } else if no_commit {
        working_tree_diff_patch(baseline).context("generate working tree diff patch")?
    } else if let Some(baseline_revision) = baseline_revision {
        git_diff_patch(&[baseline_revision, "HEAD"]).context("generate committed diff patch")?
    } else {
        String::new()
    };
    fs::write(&diff_path, patch)
        .with_context(|| format!("write run diff patch {}", diff_path.display()))
}

fn working_tree_diff_patch(baseline: &NoCommitBaseline) -> Result<String> {
    let mut patch = String::new();
    let untracked_files = git_untracked_files()?;
    let paths = git_status_paths()?
        .into_iter()
        .chain(untracked_files.iter().cloned())
        .chain(baseline.tracked_non_file_paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    for path in paths {
        if is_ralphterm_artifact(&path) {
            continue;
        }
        if is_baseline_path(&path, &baseline.paths) {
            if let Some(contents) = baseline.tracked_file_contents.get(&path) {
                patch.push_str(&git_no_index_file_patch_from_contents(&path, contents)?);
            } else {
                let recreated_baseline_non_file =
                    baseline.tracked_non_file_paths.contains(&path) && Path::new(&path).is_file();
                if recreated_baseline_non_file {
                    patch.push_str(&git_no_index_new_file_patch(&path)?);
                }
            }
            continue;
        }
        patch.push_str(&git_cached_path_diff_patch(&path)?);
        patch.push_str(&git_worktree_path_diff_patch(&path)?);
        if untracked_files.contains(&path) {
            patch.push_str(&git_no_index_new_file_patch(&path)?);
        }
    }
    Ok(patch)
}

fn is_baseline_path(path: &str, baseline_paths: &BTreeSet<String>) -> bool {
    baseline_paths
        .iter()
        .any(|baseline| path == baseline || baseline.ends_with('/') && path.starts_with(baseline))
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

fn git_no_index_file_patch_from_contents(path: &str, contents: &[u8]) -> Result<String> {
    let temp_path = PathBuf::from(".ralphterm")
        .join("progress")
        .join(format!(".baseline-{}", timestamp()));
    fs::write(&temp_path, contents)
        .with_context(|| format!("write baseline snapshot {}", temp_path.display()))?;
    let temp_path_string = temp_path.to_string_lossy().into_owned();
    let result = run_git_allow_exit_codes(
        &[
            "diff",
            "--binary",
            "--no-index",
            "--",
            &temp_path_string,
            path,
        ],
        &[0, 1],
    )
    .map(|patch| rewrite_no_index_snapshot_paths(&patch, &temp_path_string, path));
    let remove_result = fs::remove_file(&temp_path)
        .with_context(|| format!("remove baseline snapshot {}", temp_path.display()));
    match (result, remove_result) {
        (Ok(patch), Ok(())) => Ok(patch),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

fn rewrite_no_index_snapshot_paths(patch: &str, temp_path: &str, path: &str) -> String {
    patch
        .replace(&format!("a/{temp_path}"), &format!("a/{path}"))
        .replace(temp_path, path)
}

fn git_cached_path_diff_patch(path: &str) -> Result<String> {
    run_git_allow_exit_codes(&["diff", "--binary", "--cached", "--", path], &[0])
}

fn git_worktree_path_diff_patch(path: &str) -> Result<String> {
    run_git_allow_exit_codes(&["diff", "--binary", "--", path], &[0])
}

fn git_untracked_files() -> Result<BTreeSet<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()
        .context("run git")?;
    if !output.status.success() {
        bail!(
            "git command failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
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

#[derive(Default)]
struct LastTaskEndStatus {
    failed: bool,
    transcript_display: Option<String>,
    review_transcript_display: Option<String>,
}

fn last_task_end_status(path: &Path, task_number: usize) -> Result<LastTaskEndStatus> {
    if !path.exists() {
        return Ok(LastTaskEndStatus::default());
    }

    let log = fs::read_to_string(path)
        .with_context(|| format!("read progress log {}", path.display()))?;
    let task_start_prefix = format!("task_start number={task_number}");
    let task_end_prefix = format!("task_end number={task_number}");
    let mut in_task = false;
    let mut latest_task_transcript = None;
    let mut latest_review_transcript = None;
    let mut last_status = LastTaskEndStatus::default();
    for line in log.lines() {
        let Some(event) = progress_event(line) else {
            continue;
        };
        if event_starts_with_token(event, &task_start_prefix) {
            in_task = true;
            latest_task_transcript = None;
            latest_review_transcript = None;
            continue;
        }
        if in_task {
            if let Some(transcript_display) = signal_transcript_display(event) {
                latest_task_transcript = Some(transcript_display.to_string());
            }
            if let Some(review_transcript_display) = review_transcript_display(event) {
                latest_review_transcript = Some(review_transcript_display.to_string());
            }
        }
        if event_starts_with_token(event, &task_end_prefix) {
            let failed = event.contains("result=failed");
            last_status = LastTaskEndStatus {
                failed,
                transcript_display: failed.then(|| latest_task_transcript.clone()).flatten(),
                review_transcript_display: failed
                    .then(|| latest_review_transcript.clone())
                    .flatten(),
            };
            in_task = false;
        }
    }
    Ok(last_status)
}

fn signal_transcript_display(event: &str) -> Option<&str> {
    if !event.starts_with("signal=") {
        return None;
    }
    event
        .split_once("transcript path=")
        .map(|(_, transcript_display)| transcript_display.trim())
        .filter(|transcript_display| !transcript_display.is_empty())
}

fn review_transcript_display(event: &str) -> Option<&str> {
    event
        .strip_prefix("review transcript path=")
        .map(str::trim)
        .filter(|review_transcript_display| !review_transcript_display.is_empty())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompletionSignal {
    Completed,
    None,
}

impl CompletionSignal {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "COMPLETED",
            Self::None => "NONE",
        }
    }
}

fn completion_signal(transcript: &str, prompt: &str) -> CompletionSignal {
    let output = transcript_without_prompt_echo(transcript, prompt);
    if output.lines().any(|line| line == "COMPLETED") {
        CompletionSignal::Completed
    } else {
        CompletionSignal::None
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

fn git_run_baseline() -> Result<NoCommitBaseline> {
    if !git_inside_work_tree() {
        return Ok(NoCommitBaseline::default());
    }

    let tracked_paths = git_status_paths_excluding_untracked()?;
    let paths = tracked_paths
        .iter()
        .cloned()
        .chain(git_untracked_files()?)
        .collect();
    let mut tracked_file_contents = BTreeMap::new();
    let mut tracked_non_file_paths = BTreeSet::new();
    for path in tracked_paths {
        let file_path = PathBuf::from(&path);
        if file_path.is_file() {
            tracked_file_contents.insert(
                path,
                fs::read(&file_path)
                    .with_context(|| format!("read baseline file {}", file_path.display()))?,
            );
        } else {
            tracked_non_file_paths.insert(path);
        }
    }
    Ok(NoCommitBaseline {
        paths,
        tracked_file_contents,
        tracked_non_file_paths,
    })
}

fn git_status_paths() -> Result<BTreeSet<String>> {
    git_status_paths_from_porcelain(true)
}

fn git_status_paths_excluding_untracked() -> Result<BTreeSet<String>> {
    git_status_paths_from_porcelain(false)
}

fn git_status_paths_from_porcelain(include_untracked: bool) -> Result<BTreeSet<String>> {
    let output = run_git(&["status", "--porcelain", "-z"])?;
    let mut paths = BTreeSet::new();
    for entry in output.split('\0').filter(|entry| !entry.is_empty()) {
        if entry.len() >= 4 {
            if !include_untracked && entry.starts_with("?? ") {
                continue;
            }
            paths.insert(entry[3..].to_string());
        }
    }
    Ok(paths)
}

fn is_ralphterm_artifact(path: &str) -> bool {
    path == ".ralphterm" || path.starts_with(".ralphterm/")
}

fn git_inside_work_tree() -> bool {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true"
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

fn build_task_prompt(
    plan_name: &str,
    task: &Task,
    validation_commands: &[String],
    review_feedback: Option<&str>,
    resume_context: Option<&ResumeContext>,
) -> String {
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
    if let Some(review_feedback) = review_feedback {
        prompt.push_str("\nPrevious review failed. Fix the task using this independent reviewer feedback before printing COMPLETED:\n");
        prompt.push_str(review_feedback);
        if !review_feedback.ends_with('\n') {
            prompt.push('\n');
        }
    }
    if let Some(resume_context) = resume_context {
        prompt.push_str("\nPrevious run for this task failed. You may inspect prior artifacts before continuing:\n");
        prompt.push_str("- Previous transcript: ");
        prompt.push_str(&resume_context.transcript_display);
        prompt.push('\n');
        prompt.push_str("- Previous validation output: ");
        prompt.push_str(&resume_context.validation_output_display);
        prompt.push('\n');
        if let Some(review_transcript_display) = &resume_context.review_transcript_display {
            prompt.push_str("- Previous review transcript: ");
            prompt.push_str(review_transcript_display);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nWhen the task is complete, print COMPLETED.\n");
    prompt
}

fn run_validation_commands(commands: &[String], output_path: &Path) -> Result<String> {
    let mut output = String::new();
    fs::write(output_path, &output)
        .with_context(|| format!("write validation output {}", output_path.display()))?;
    for command in commands {
        output.push_str(&format!("Validation: {command}\n"));
        let result = match Command::new("sh").arg("-lc").arg(command).output() {
            Ok(result) => result,
            Err(err) => {
                fs::write(output_path, &output).with_context(|| {
                    format!("write validation output {}", output_path.display())
                })?;
                return Err(err).with_context(|| format!("run validation command `{command}`"));
            }
        };
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        if !stdout.is_empty() {
            output.push_str(&stdout);
        }
        if !stderr.is_empty() {
            output.push_str(&stderr);
        }
        if result.status.success() {
            output.push_str("Validation passed\n");
        } else {
            fs::write(output_path, &output)
                .with_context(|| format!("write validation output {}", output_path.display()))?;
            bail!(
                "validation command failed `{command}` with {}\nstdout:\n{}\nstderr:\n{}",
                result.status,
                stdout,
                stderr
            );
        }
    }
    fs::write(output_path, &output)
        .with_context(|| format!("write validation output {}", output_path.display()))?;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn run_review_command(
    review_command: &str,
    plan_name: &str,
    task: &Task,
    agent_transcript: &str,
    validation_output: &str,
    timeout: Duration,
    progress: &ProgressPaths,
    attempt_progress: &AttemptProgressPaths,
) -> std::result::Result<String, ReviewCommandError> {
    let prompt = build_review_prompt(
        plan_name,
        task,
        agent_transcript,
        validation_output,
        &git_state_for_review(),
    );
    let review_run = run_agent_command_with_timeout(review_command, &prompt, timeout)
        .with_context(|| format!("run review for task {}", task.number))?;
    fs::write(
        &attempt_progress.review_transcript_path,
        &review_run.transcript,
    )
    .with_context(|| {
        format!(
            "write review transcript {}",
            attempt_progress.review_transcript_path.display()
        )
    })?;
    if let Some(legacy_review_transcript_path) = &attempt_progress.legacy_review_transcript_path {
        fs::write(legacy_review_transcript_path, &review_run.transcript).with_context(|| {
            format!(
                "write review transcript {}",
                legacy_review_transcript_path.display()
            )
        })?;
    }
    let review_transcript_display = if attempt_progress.legacy_review_transcript_path.is_some() {
        &progress.review_transcript_display
    } else {
        &attempt_progress.review_transcript_display
    };
    append_progress(
        &progress.log_path,
        &format!("review transcript path={review_transcript_display}"),
    )?;

    let mut output = review_run.transcript;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    if review_run.timed_out {
        return Err(ReviewCommandError::new(
            format!(
                "review command timed out after {:?} for task {}\n{}",
                timeout, task.number, output
            ),
            false,
        ));
    }
    if review_run.exit_code != 0 {
        return Err(ReviewCommandError::new(
            format!(
                "review command exited with {} for task {}\n{}",
                review_run.exit_code, task.number, output
            ),
            false,
        ));
    }
    match review_output_decision(&output, &prompt) {
        Some(true) => {
            output.push_str("Review passed\n");
            Ok(output)
        }
        Some(false) => Err(ReviewCommandError::new(
            format!("review failed for task {}\n{}", task.number, output),
            true,
        )),
        None => Err(ReviewCommandError::new(
            format!(
                "review did not emit REVIEW_PASS or REVIEW_FAIL for task {}\n{}",
                task.number, output
            ),
            false,
        )),
    }
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
    "Print REVIEW_PASS only if the task matches the spec and the validation output supports accepting it. Print REVIEW_FAIL with the reason otherwise."
}

fn review_output_decision(transcript: &str, _prompt: &str) -> Option<bool> {
    let normalized = transcript.replace("\r\n", "\n").replace('\r', "\n");
    let reviewer_output = normalized
        .rfind(review_instruction())
        .map(|start| &normalized[start + review_instruction().len()..])
        .unwrap_or(normalized.as_str());

    reviewer_output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line == "REVIEW_PASS" {
                Some(true)
            } else if line == "REVIEW_FAIL"
                || line.starts_with("REVIEW_FAIL ")
                || line.starts_with("REVIEW_FAIL:")
            {
                Some(false)
            } else {
                None
            }
        })
        .next_back()
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
            agent.child.kill().context("kill timed out agent")?;
            break agent.child.wait().context("wait for killed agent")?;
        }

        thread::sleep(Duration::from_millis(10));
    };

    if timed_out {
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
    } else {
        while !reader_done {
            match rx.recv() {
                Ok(ReaderEvent::Chunk(chunk)) => transcript.push_str(&chunk),
                Ok(ReaderEvent::Error(err)) => {
                    read_error = Some(err);
                    reader_done = true;
                }
                Ok(ReaderEvent::Done) => reader_done = true,
                Err(_) => break,
            }
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
    let mut parts = parse_agent_command(agent_command)?;
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

fn parse_agent_command(agent_command: &str) -> Result<Vec<String>> {
    shlex::split(agent_command)
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid agent command"))
}

fn validate_interactive_agent_command(agent_command: &str) -> Result<()> {
    let parts = parse_agent_command(agent_command)?;
    if parts
        .iter()
        .skip(1)
        .any(|arg| arg == "-p" || arg == "--print")
    {
        bail!(
            "one-shot prompt mode is not supported; RalphTerm requires the official CLI in an interactive PTY"
        );
    }
    Ok(())
}
