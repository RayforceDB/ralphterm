use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::symlink,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::json;

use crate::plan::Task;

pub const DEFAULT_PLAN_AGENT_COMMAND: &str = "claude";

pub(crate) fn count_unchecked_tasks(plan_path: &std::path::Path) -> std::io::Result<usize> {
    let body = std::fs::read_to_string(plan_path)?;
    let count = body
        .lines()
        .filter(|line| line.trim_start().starts_with("- [ ]"))
        .count();
    Ok(count)
}

#[derive(Debug, Default)]
#[allow(dead_code)]
struct NoCommitBaseline {
    paths: BTreeSet<String>,
    tracked_file_contents: BTreeMap<String, Vec<u8>>,
    tracked_non_file_paths: BTreeSet<String>,
}

#[derive(Debug, Default)]
#[allow(dead_code)]
struct RetryCleanupSnapshot {
    dirs: BTreeMap<PathBuf, fs::Permissions>,
    files: BTreeMap<PathBuf, RetryCleanupFileSnapshot>,
    symlinks: BTreeMap<PathBuf, PathBuf>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct RetryCleanupFileSnapshot {
    contents: Vec<u8>,
    permissions: fs::Permissions,
}

impl RetryCleanupSnapshot {
    #[allow(dead_code)]
    fn capture(root: &Path) -> Result<Self> {
        let mut snapshot = Self::default();
        collect_retry_cleanup_snapshot(root, root, &mut snapshot)?;
        Ok(snapshot)
    }

    #[allow(dead_code)]
    fn restore(&self, root: &Path) -> Result<()> {
        self.restore_existing_baseline_dir_permissions_shallow_to_deep(root)?;

        let mut current = Vec::new();
        collect_retry_cleanup_paths(root, root, &self.dirs, &mut current)?;
        current.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for relative_path in current {
            let path = root.join(&relative_path);
            if self.path_matches_baseline_kind(root, &relative_path)? {
                continue;
            }
            if path
                .symlink_metadata()
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false)
            {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).with_context(
                    || {
                        format!(
                            "make rejected directory removable before cleanup {}",
                            path.display()
                        )
                    },
                )?;
                fs::remove_dir_all(&path)
                    .with_context(|| format!("remove rejected directory {}", path.display()))?;
            } else if path.symlink_metadata().is_ok() {
                fs::remove_file(&path)
                    .with_context(|| format!("remove rejected file {}", path.display()))?;
            }
        }
        for relative_dir in self.dirs.keys() {
            let path = root.join(relative_dir);
            fs::create_dir_all(&path)
                .with_context(|| format!("restore baseline directory {}", path.display()))?;
        }
        for (relative_file, file_snapshot) in &self.files {
            let path = root.join(relative_file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("restore parent directory {}", parent.display()))?;
            }
            if path
                .symlink_metadata()
                .map(|metadata| !metadata.is_file())
                .unwrap_or(false)
            {
                remove_path_for_restore(&path)?;
            }
            fs::write(&path, &file_snapshot.contents)
                .with_context(|| format!("restore baseline file {}", path.display()))?;
            fs::set_permissions(&path, file_snapshot.permissions.clone())
                .with_context(|| format!("restore baseline file permissions {}", path.display()))?;
        }
        for (relative_link, target) in &self.symlinks {
            let path = root.join(relative_link);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("restore symlink parent directory {}", parent.display())
                })?;
            }
            if path.symlink_metadata().is_ok() {
                remove_path_for_restore(&path)?;
            }
            symlink(target, &path)
                .with_context(|| format!("restore baseline symlink {}", path.display()))?;
        }
        let mut dirs: Vec<_> = self.dirs.iter().collect();
        dirs.sort_by_key(|(relative_dir, _)| std::cmp::Reverse(relative_dir.components().count()));
        for (relative_dir, permissions) in dirs {
            let path = root.join(relative_dir);
            fs::set_permissions(&path, permissions.clone()).with_context(|| {
                format!("restore baseline directory permissions {}", path.display())
            })?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn path_matches_baseline_kind(&self, root: &Path, relative_path: &Path) -> Result<bool> {
        let is_baseline_dir = self.dirs.contains_key(relative_path);
        let is_baseline_file = self.files.contains_key(relative_path);
        let is_baseline_symlink = self.symlinks.contains_key(relative_path);
        if !is_baseline_dir && !is_baseline_file && !is_baseline_symlink {
            return Ok(false);
        }
        let path = root.join(relative_path);
        let metadata = path
            .symlink_metadata()
            .with_context(|| format!("stat cleanup path {}", path.display()))?;
        let file_type = metadata.file_type();
        Ok((is_baseline_dir && file_type.is_dir())
            || (is_baseline_file && file_type.is_file())
            || (is_baseline_symlink && file_type.is_symlink()))
    }

    #[allow(dead_code)]
    fn restore_existing_baseline_dir_permissions_shallow_to_deep(&self, root: &Path) -> Result<()> {
        let mut dirs: Vec<_> = self.dirs.iter().collect();
        dirs.sort_by_key(|(relative_dir, _)| relative_dir.components().count());
        for (relative_dir, permissions) in dirs {
            let path = root.join(relative_dir);
            let Ok(metadata) = path.symlink_metadata() else {
                continue;
            };
            if metadata.is_dir() {
                fs::set_permissions(&path, permissions.clone()).with_context(|| {
                    format!(
                        "make baseline directory traversable before cleanup {}",
                        path.display()
                    )
                })?;
            }
        }
        Ok(())
    }
}

pub type RunEventSink = Arc<dyn Fn(PlanRunEvent) -> Result<()> + Send + Sync>;
pub type RunCancellationCheck = Arc<dyn Fn() -> Result<()> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct PlanRunEvent {
    pub event_type: &'static str,
    pub task_number: Option<usize>,
    pub task_title: Option<String>,
    pub attempt: Option<usize>,
    pub artifact_path: Option<String>,
    pub message: Option<String>,
}

impl PlanRunEvent {
    #[allow(dead_code)]
    fn for_task(event_type: &'static str, task: &Task, attempt: Option<usize>) -> Self {
        Self {
            event_type,
            task_number: Some(task.number),
            task_title: Some(task.title.clone()),
            attempt,
            artifact_path: None,
            message: None,
        }
    }

    #[allow(dead_code)]
    fn with_artifact(mut self, artifact_path: impl Into<String>) -> Self {
        self.artifact_path = Some(artifact_path.into());
        self
    }

    #[allow(dead_code)]
    fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    /// Default mode: task phase plus review gate.
    #[default]
    Full,
    /// Run task phase only; skip the review gate.
    TasksOnly,
    /// Skip the task phase; run the reviewer once against the current state.
    ReviewOnly,
    /// Skip the task phase; iterate implementer + reviewer until the reviewer
    /// accepts or the iteration ceiling is reached.
    ExternalOnly,
}

#[derive(Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub agent_command: Option<String>,
    pub review_command: Option<String>,
    pub agent_timeout: Option<Duration>,
    pub require_review: bool,
    pub max_review_retries: usize,
    pub no_commit: bool,
    pub dry_run: bool,
    pub event_sink: Option<RunEventSink>,
    pub cancellation_check: Option<RunCancellationCheck>,
    /// Maximum number of consecutive identical review failure categories
    /// (matched on the first line of the REVIEW_FAIL reason) before the run
    /// aborts with a stalemate error. `None` disables the guard.
    pub review_patience: Option<usize>,
    /// Selects how the runner orchestrates implementer and reviewer phases.
    pub mode: RunMode,
    /// Ceiling on external review-loop iterations when `mode` is
    /// `RunMode::ExternalOnly`.
    pub max_external_iterations: Option<usize>,
}

pub async fn run_plan(options: RunOptions) -> Result<String> {
    match options.mode {
        RunMode::Full | RunMode::TasksOnly => run_plan_default(options).await,
        RunMode::ReviewOnly => run_plan_review_only(options).await,
        RunMode::ExternalOnly => run_plan_external_only(options).await,
    }
}

async fn run_plan_default(options: RunOptions) -> Result<String> {
    use crate::output_format as fmt;
    use crate::preflight::Preflight;
    use crate::progress_log::ProgressLog;
    use crate::prompts::{substitute, Prompts};
    use std::collections::HashMap;
    use std::time::Instant;

    let RunOptions {
        plan_path,
        agent_command,
        mode,
        no_commit,
        dry_run,
        agent_timeout,
        review_command,
        event_sink,
        ..
    } = options;

    let agent_cmd = agent_command.unwrap_or_else(|| DEFAULT_PLAN_AGENT_COMMAND.to_string());
    let review_override = review_command.as_deref();
    let repo_root = std::env::current_dir()?;
    let plan_path = plan_path.canonicalize()?;
    let plan_display = relpath_from(&repo_root, &plan_path);

    let max_iterations: usize = std::env::var("RALPHTERM_MAX_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    let preflight = Preflight {
        repo_root: &repo_root,
        plan_path: &plan_path,
        branch_override: None,
        use_worktree: false,
        allow_dirty: dry_run,
        skip_trust_check: !will_invoke_bare_claude(&agent_cmd),
    }
    .check()?;

    fmt::print_version_banner();
    if preflight.created_branch {
        fmt::print_branch_creating(&preflight.branch);
    }

    let mut progress = ProgressLog::open(&repo_root, &preflight.plan_slug)?;
    let mode_label = fmt::mode_label(
        matches!(mode, RunMode::TasksOnly),
        matches!(mode, RunMode::ReviewOnly),
        matches!(mode, RunMode::ExternalOnly),
    );
    let progress_display = relpath_from(&repo_root, progress.path());
    fmt::print_run_header(
        max_iterations,
        mode_label,
        &plan_display,
        &preflight.branch,
        &progress_display,
    );
    progress.write_control(&format!("creating branch: {}", preflight.branch))?;
    progress.write_control(&format!(
        "starting ralphex loop (max {max_iterations} iterations) ({mode_label})"
    ))?;
    progress.write_control(&format!("plan: {}", plan_display.display()))?;
    progress.write_control(&format!("branch: {}", preflight.branch))?;

    fmt::print_task_phase_start();
    progress.write_control("starting task execution phase")?;

    let prompts = Prompts::load(&repo_root, None);
    let start = Instant::now();
    let mut agent_declared_done = false;

    for iteration in 1..=max_iterations {
        if count_unchecked_tasks(&plan_path)? == 0 {
            break;
        }
        fmt::print_iteration_header(iteration);
        progress.write_control(&format!("--- task iteration {iteration} ---"))?;

        if let Some(sink) = event_sink.as_ref() {
            let _ = sink(PlanRunEvent {
                event_type: "iteration_started",
                task_number: None,
                task_title: None,
                attempt: Some(iteration),
                artifact_path: None,
                message: None,
            });
        }

        let plan_str = plan_path.to_string_lossy().to_string();
        let progress_path_str = progress.path().to_string_lossy().to_string();
        let goal = plan_first_h1(&plan_path).unwrap_or_default();
        let default_branch = preflight.default_branch.clone();

        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_path_str);
        vars.insert("GOAL", &goal);
        vars.insert("DEFAULT_BRANCH", &default_branch);

        let prompt = substitute(&prompts.task, &vars);
        let idle_timeout = agent_timeout.unwrap_or_else(agent_timeout_default);

        // Liveness spinner. Returns None when stderr isn't a TTY or
        // the user opts out via NO_COLOR / RALPHTERM_NO_SPINNER, in
        // which case driver events still flow through to event_sink
        // but no spinner paints.
        let spinner =
            crate::spinner::Spinner::start(format!("iteration {iteration}: starting agent"));

        let driver_sink = {
            let outer_sink = event_sink.clone();
            let attempt = iteration;
            let spinner = spinner.clone();
            let sink: crate::agent_driver::EventSink =
                Arc::new(move |ev: crate::agent_driver::DriverEvent| {
                    if let Some(spinner) = spinner.as_ref() {
                        // Any event = activity (resets idle counter).
                        spinner.bump_activity();
                        if let Some(label) = crate::spinner::label_for_event(ev.kind) {
                            spinner.set_label(format!("iteration {attempt}: {label}"));
                        }
                    }
                    if let Some(ref sink) = outer_sink {
                        let _ = sink(PlanRunEvent {
                            event_type: ev.kind,
                            task_number: None,
                            task_title: None,
                            attempt: Some(attempt),
                            artifact_path: None,
                            message: ev.detail.clone(),
                        });
                    }
                });
            Some(sink)
        };

        // Drive the agent through the v0.3 TTY-native file-handoff
        // contract: PTY-only (no --print), bracketed-paste keystrokes,
        // captured response retrieved from .ralphterm/iteration-output/
        // <nonce>.md (see agent_driver.rs for the formal protocol).
        let run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
            command: &agent_cmd,
            task_prompt: &prompt,
            repo_root: &repo_root,
            idle_timeout,
            cancel: None,
            event_sink: driver_sink,
        })
        .await?;

        // Stop the spinner before we start printing the captured
        // response so the lines don't get overwritten by the next paint.
        if let Some(s) = spinner.as_ref() {
            s.stop();
        }
        drop(spinner);

        // Stream the captured response (the agent's curated account of
        // this iteration) into the progress log so the next iteration's
        // fresh implementer and downstream reviewers can see what
        // happened. Each line gets the ralphex-style [YYYY-MM-DD ...]
        // prefix on stdout.
        if let Some(captured) = run.captured_response.as_ref() {
            for line in captured.lines() {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                let ts_now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                let stamp = crate::color::dim(&format!("[{ts_now}]"));
                let body = if crate::color::is_section_header(trimmed) {
                    crate::color::cyan(&crate::color::bold(trimmed))
                } else {
                    trimmed.to_string()
                };
                println!("{stamp} {body}");
                let _ = progress.write_narration(trimmed);
            }
            if let Some(sink) = event_sink.as_ref() {
                let _ = sink(PlanRunEvent {
                    event_type: "iteration_captured",
                    task_number: None,
                    task_title: None,
                    attempt: Some(iteration),
                    artifact_path: Some(run.output_path.display().to_string()),
                    message: Some(captured.clone()),
                });
            }
        } else {
            // No captured response. The full transcript still has the
            // tool-use / TUI noise; record a single explanatory line so
            // the progress log isn't silent for this iteration.
            let reason = if run.timed_out {
                "agent timed out before END marker"
            } else if run.crashed_before_done {
                "agent process exited before END marker"
            } else if run.cancelled {
                "agent cancelled"
            } else {
                "agent exited without writing a complete output file"
            };
            let ts_now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            println!(
                "{} {}",
                crate::color::dim(&format!("[{ts_now}]")),
                crate::color::yellow(reason)
            );
            let _ = progress.write_narration(reason);
        }

        // Stop early when the agent signals the whole plan is done OR
        // we observe all checkboxes flipped (cross-check, see
        // count_unchecked_tasks).
        let all_done_signal = run
            .captured_response
            .as_deref()
            .map(|c| c.contains("RALPHTERM:ALL_TASKS_DONE") || c.contains("ALL_TASKS_DONE"))
            .unwrap_or(false);
        if all_done_signal {
            agent_declared_done = true;
            break;
        }
        if count_unchecked_tasks(&plan_path)? == 0 {
            break;
        }

        // Soft-failures (no output file, clean exit without END, or
        // child crash) are logged and the loop continues — the
        // max_iterations cap at the bottom catches "no progress at all".
        // Hard-failures (idle hang) abort immediately because retrying
        // a stuck agent just waits the same timeout again.
        if run.timed_out {
            anyhow::bail!(
                "agent iteration {iteration} timed out after {:?} with no END marker",
                idle_timeout
            );
        }
        if run.crashed_before_done {
            eprintln!(
                "{}",
                crate::color::warn_line(format!(
                    "iteration {iteration} agent exited (code={}) before writing the output file — continuing to next iteration",
                    run.exit_code
                ))
            );
        } else if !run.done_via_file {
            eprintln!(
                "{}",
                crate::color::warn_line(format!(
                    "iteration {iteration} produced no valid output file at {} — continuing to next iteration",
                    run.output_path.display()
                ))
            );
        }
    }

    // Only bail with "hit max iterations" when (a) the agent never
    // signalled completion AND (b) the plan still has unchecked boxes.
    // If the agent explicitly said ALL_TASKS_DONE we accept that — the
    // agent might be writing to a different artifact (e.g. a docker
    // smoke run that only touches first.txt, not the plan checkboxes).
    // Warn but don't abort when the signal arrives with unchecked boxes
    // remaining — the operator can see in the progress log what was
    // actually done.
    let remaining_unchecked = count_unchecked_tasks(&plan_path)?;
    if !agent_declared_done && remaining_unchecked > 0 {
        anyhow::bail!("hit max iterations ({max_iterations}) without ALL_TASKS_DONE");
    }
    if agent_declared_done && remaining_unchecked > 0 {
        let plural = if remaining_unchecked == 1 { "" } else { "es" };
        eprintln!(
            "{}",
            crate::color::warn_line(format!(
                "agent declared the plan done while {remaining_unchecked} checkbox{plural} remained unchecked — accepting on the agent's word (see the iteration log above for its reasoning)"
            ))
        );
    }

    // Blank line separates agent narration from the wrap-up control lines,
    // matching ralphex's output.
    println!();

    // Ralphex prints "all tasks completed, starting code review..." even in
    // --tasks-only mode (the wording is a leftover from full mode but
    // matches their actual behaviour). We mirror it. The "task execution
    // completed successfully" wrap-up comes after the review phases (or
    // immediately, in tasks-only mode).
    crate::output_format::print_all_tasks_completed();
    progress.write_control("all tasks completed, starting code review...")?;

    if matches!(mode, RunMode::TasksOnly) {
        crate::output_format::print_task_execution_completed();
        progress.write_control("task execution completed successfully")?;
    }

    if !matches!(mode, RunMode::TasksOnly) {
        // Below: review pipeline (phases 1-3) only in non-tasks-only modes.

        let reviewer_cmd = derive_reviewer_command(review_override, &repo_root)?;

        crate::output_format::print_phase_start(
            "phase 1 first review (one reviewer session, 5 dimensions: quality, implementation, testing, simplification, documentation)",
        );
        progress.write_control("phase 1 first review (one reviewer session, 5 dimensions)")?;
        let started = std::time::Instant::now();
        let outcome = crate::review_phases::first_review(crate::review_phases::FirstReviewArgs {
            prompts: &prompts,
            reviewer_command: &reviewer_cmd,
            plan_path: &plan_path,
            progress_path: progress.path(),
            default_branch: &preflight.default_branch,
            agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
        })
        .await?;
        crate::output_format::print_phase_done("phase 1 first review", started.elapsed());
        if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
            for f in findings {
                eprintln!("[review-first] {f}");
            }
            anyhow::bail!("first review found critical issues");
        }
    }

    if !matches!(mode, RunMode::TasksOnly | RunMode::ReviewOnly) {
        let reviewer_cmd = derive_reviewer_command(review_override, &repo_root)?;
        crate::output_format::print_phase_start(
            "phase 2 external review (codex, iterative fixer loop)",
        );
        progress.write_control("phase 2 external review")?;
        let started = std::time::Instant::now();
        let outcome =
            crate::review_phases::external_review(crate::review_phases::ExternalReviewArgs {
                prompts: &prompts,
                implementer_command: &agent_cmd,
                reviewer_command: &reviewer_cmd,
                plan_path: &plan_path,
                progress_path: progress.path(),
                default_branch: &preflight.default_branch,
                agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
                max_iterations: 3,
            })
            .await?;
        crate::output_format::print_phase_done("phase 2 external review", started.elapsed());
        if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
            for f in findings {
                eprintln!("[review-external] {f}");
            }
            anyhow::bail!("external review found critical issues");
        }
    }

    if !matches!(
        mode,
        RunMode::TasksOnly | RunMode::ReviewOnly | RunMode::ExternalOnly
    ) {
        let reviewer_cmd = derive_reviewer_command(review_override, &repo_root)?;
        crate::output_format::print_phase_start(
            "phase 3 second review (one reviewer session, 2 dimensions: quality, implementation)",
        );
        progress.write_control("phase 3 second review (one reviewer session, 2 dimensions)")?;
        let started = std::time::Instant::now();
        let outcome = crate::review_phases::second_review(crate::review_phases::FirstReviewArgs {
            prompts: &prompts,
            reviewer_command: &reviewer_cmd,
            plan_path: &plan_path,
            progress_path: progress.path(),
            default_branch: &preflight.default_branch,
            agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
        })
        .await?;
        crate::output_format::print_phase_done("phase 3 second review", started.elapsed());
        if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
            for f in findings {
                eprintln!("[review-second] {f}");
            }
            anyhow::bail!("second review found critical issues");
        }
    }

    if !matches!(mode, RunMode::TasksOnly) {
        let plan_str = plan_path.to_string_lossy().to_string();
        let progress_str = progress.path().to_string_lossy().to_string();
        let default_branch = preflight.default_branch.clone();
        let mut vars: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_str);
        vars.insert("DEFAULT_BRANCH", &default_branch);
        let prompt = crate::prompts::substitute(&prompts.finalize, &vars);
        let _ = run_agent_command_with_timeout(
            &agent_cmd,
            &prompt,
            agent_timeout.unwrap_or_else(agent_timeout_default),
        )?;
    }

    if !matches!(mode, RunMode::TasksOnly) {
        crate::output_format::print_task_execution_completed();
        progress.write_control("task execution completed successfully")?;
    }

    let elapsed = start.elapsed();
    let (files, additions, deletions) = git_shortstat(&repo_root, &preflight.default_branch)?;

    let plan_dest = if no_commit {
        plan_path.clone()
    } else {
        move_plan_to_completed(&plan_path)?
    };
    let plan_dest_display = relpath_from(&repo_root, &plan_dest);
    if plan_dest != plan_path {
        // print_moved_plan: ralphex uses absolute here.
        fmt::print_moved_plan(&plan_dest);
        progress.write_control(&format!("moved plan to {}", plan_dest.display()))?;
    }

    fmt::print_completion_summary(
        elapsed,
        files,
        additions,
        deletions,
        &plan_dest_display,
        &preflight.branch,
        &progress_display,
    );

    Ok(String::new())
}

async fn run_plan_review_only(options: RunOptions) -> Result<String> {
    use crate::preflight::Preflight;
    use crate::progress_log::ProgressLog;
    use crate::prompts::Prompts;

    let RunOptions {
        plan_path,
        agent_timeout,
        review_command,
        ..
    } = options;
    let review_override = review_command.as_deref();
    let repo_root = std::env::current_dir()?;
    let plan_path = plan_path.canonicalize()?;
    let preflight = Preflight {
        repo_root: &repo_root,
        plan_path: &plan_path,
        branch_override: None,
        use_worktree: false,
        allow_dirty: true,
        // --review mode does NOT spawn an implementer; only reviewer
        // agents (which use the codex wrapper, not bare claude). Trust
        // check is therefore not required for this entry point.
        skip_trust_check: true,
    }
    .check()?;
    let progress = ProgressLog::open(&repo_root, &preflight.plan_slug)?;
    let prompts = Prompts::load(&repo_root, None);
    let reviewer_cmd = derive_reviewer_command(review_override, &repo_root)?;
    let args = crate::review_phases::FirstReviewArgs {
        prompts: &prompts,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
    };
    let outcome = crate::review_phases::first_review(args).await?;
    if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
        for f in findings {
            eprintln!("[review-first] {f}");
        }
        anyhow::bail!("review-only first review found critical issues");
    }
    // For --review mode we also exercise phase 3 to mirror ralphex's "full
    // review pipeline" wording. Reuse FirstReviewArgs because the signature
    // is identical.
    let args = crate::review_phases::FirstReviewArgs {
        prompts: &prompts,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
    };
    let outcome = crate::review_phases::second_review(args).await?;
    if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
        for f in findings {
            eprintln!("[review-second] {f}");
        }
        anyhow::bail!("review-only second review found critical issues");
    }
    Ok(String::new())
}

async fn run_plan_external_only(options: RunOptions) -> Result<String> {
    use crate::preflight::Preflight;
    use crate::progress_log::ProgressLog;
    use crate::prompts::Prompts;

    let RunOptions {
        plan_path,
        agent_command,
        review_command,
        agent_timeout,
        max_external_iterations,
        ..
    } = options;
    let agent_cmd = agent_command.unwrap_or_else(|| DEFAULT_PLAN_AGENT_COMMAND.to_string());
    let reviewer_cmd = review_command
        .ok_or_else(|| anyhow::anyhow!("review command required for --external-only"))?;
    let repo_root = std::env::current_dir()?;
    let plan_path = plan_path.canonicalize()?;

    let preflight = Preflight {
        repo_root: &repo_root,
        plan_path: &plan_path,
        branch_override: None,
        use_worktree: false,
        allow_dirty: true,
        skip_trust_check: !will_invoke_bare_claude(&agent_cmd),
    }
    .check()?;
    let progress = ProgressLog::open(&repo_root, &preflight.plan_slug)?;
    let prompts = Prompts::load(&repo_root, None);

    let outcome = crate::review_phases::external_review(crate::review_phases::ExternalReviewArgs {
        prompts: &prompts,
        implementer_command: &agent_cmd,
        reviewer_command: &reviewer_cmd,
        plan_path: &plan_path,
        progress_path: progress.path(),
        default_branch: &preflight.default_branch,
        agent_timeout: agent_timeout.unwrap_or_else(agent_timeout_default),
        max_iterations: max_external_iterations.unwrap_or(3),
    })
    .await?;
    if let crate::review_phases::ReviewOutcome::Issues(findings) = outcome {
        for f in findings {
            eprintln!("[review-external] {f}");
        }
        anyhow::bail!("external-only review found critical issues");
    }
    Ok(String::new())
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

#[allow(dead_code)]
fn describe_dry_run(
    plan_name: &str,
    validation_commands: &[String],
    pending: &[&Task],
    review_command: Option<&str>,
    max_review_retries: usize,
) -> String {
    let mut output = format!("Dry run: {plan_name}\n");
    match review_command {
        Some(command) => output.push_str(&format!("Review: {command}\n")),
        None => output.push_str("Review: skipped\n"),
    }
    output.push_str(&format!("Review retries: {max_review_retries}\n"));
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

#[allow(dead_code)]
fn emit_plan_event(options: &RunOptions, event: PlanRunEvent) -> Result<()> {
    if let Some(event_sink) = &options.event_sink {
        event_sink(event)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn emit_task_failed_event(
    options: &RunOptions,
    task: &Task,
    attempt: usize,
    message: impl Into<String>,
) -> Result<()> {
    emit_plan_event(
        options,
        PlanRunEvent::for_task("task_failed", task, Some(attempt)).with_message(message),
    )
}

#[allow(dead_code)]
struct ProgressPaths {
    log_path: PathBuf,
    transcript_path: PathBuf,
    review_transcript_path: PathBuf,
    validation_output_path: PathBuf,
    transcript_display: String,
    review_transcript_display: String,
    validation_output_display: String,
}

#[allow(dead_code)]
struct ExecutedTask {
    number: usize,
    title: String,
    attempts: usize,
    review_attempts: usize,
    transcript_display: String,
    validation_output_display: String,
    review_transcript_display: Option<String>,
    commit: Option<String>,
    commit_status: &'static str,
}

#[allow(dead_code)]
struct AttemptProgressPaths {
    transcript_path: PathBuf,
    review_transcript_path: PathBuf,
    legacy_review_transcript_path: Option<PathBuf>,
    transcript_display: String,
    review_transcript_display: String,
}

#[allow(dead_code)]
struct ResumeContext {
    transcript_display: String,
    validation_output_display: Option<String>,
    review_transcript_display: Option<String>,
}

pub(crate) struct AgentRun {
    pub(crate) transcript: String,
    pub(crate) exit_code: u32,
    pub(crate) timed_out: bool,
}

#[derive(Debug)]
#[allow(dead_code)]
struct ReviewCommandError {
    message: String,
    explicit_fail: bool,
    transcript_written: bool,
}

impl ReviewCommandError {
    #[allow(dead_code)]
    fn new(message: String, explicit_fail: bool, transcript_written: bool) -> Self {
        Self {
            message,
            explicit_fail,
            transcript_written,
        }
    }

    #[allow(dead_code)]
    fn explicit_fail(&self) -> bool {
        self.explicit_fail
    }

    #[allow(dead_code)]
    fn transcript_written(&self) -> bool {
        self.transcript_written
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
        Self::new(error.to_string(), false, false)
    }
}

impl From<std::io::Error> for ReviewCommandError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string(), false, false)
    }
}

impl ProgressPaths {
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

#[allow(dead_code)]
fn append_progress(path: &Path, event: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open progress log {}", path.display()))?;
    writeln!(file, "timestamp={} {event}", timestamp()).context("write progress log")
}

#[allow(dead_code)]
fn run_summary_path(plan_slug: &str) -> PathBuf {
    PathBuf::from(".ralphterm")
        .join("progress")
        .join(format!("{plan_slug}-summary.md"))
}

#[allow(dead_code)]
fn run_summary_json_path(plan_slug: &str) -> PathBuf {
    PathBuf::from(".ralphterm")
        .join("progress")
        .join(format!("{plan_slug}-summary.json"))
}

#[allow(dead_code)]
fn remove_stale_run_summary(plan_slug: &str) -> Result<()> {
    let summary_path = run_summary_path(plan_slug);
    match fs::remove_file(&summary_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("remove stale run summary {}", summary_path.display()))
        }
    }?;

    let summary_json_path = run_summary_json_path(plan_slug);
    match fs::remove_file(&summary_json_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "remove stale run summary json {}",
                summary_json_path.display()
            )
        }),
    }
}

#[allow(dead_code)]
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
        summary.push_str(&format!("  - Commit: {}\n", task_commit_display(task)));
    }
    fs::write(&summary_path, summary)
        .with_context(|| format!("write run summary {}", summary_path.display()))?;

    let summary_json_path = run_summary_json_path(plan_slug);
    let summary_json_tasks: Vec<_> = tasks
        .iter()
        .map(|task| {
            let review_status = passed_task_review_status(&task.review_transcript_display);
            json!({
                "number": task.number,
                "title": task.title,
                "status": "passed",
                "accepted": true,
                "attempts": task.attempts,
                "review_attempts": task.review_attempts,
                "transcript": task.transcript_display,
                "validation": task.validation_output_display,
                "review_status": review_status,
                "review_transcript": task.review_transcript_display,
                "commit": task.commit,
                "commit_status": task.commit_status,
                "acceptance_gates": {
                    "agent": "passed",
                    "validation": "passed",
                    "review": review_status,
                    "commit": task.commit_status,
                },
            })
        })
        .collect();
    let summary_json = json!({
        "plan": plan_name,
        "result": "passed",
        "tasks": summary_json_tasks,
    });
    fs::write(
        &summary_json_path,
        serde_json::to_string_pretty(&summary_json).context("serialize run summary json")? + "\n",
    )
    .with_context(|| format!("write run summary json {}", summary_json_path.display()))
}

#[allow(dead_code)]
fn write_no_pending_run_summary(plan_name: &str, plan_slug: &str) -> Result<()> {
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let summary_path = run_summary_path(plan_slug);
    let summary = format!("# Run Summary: {plan_name}\n\nResult: passed\n\nNo pending tasks.\n");
    fs::write(&summary_path, summary)
        .with_context(|| format!("write run summary {}", summary_path.display()))?;

    let summary_json_path = run_summary_json_path(plan_slug);
    let summary_json = json!({
        "plan": plan_name,
        "result": "passed",
        "tasks": [],
    });
    fs::write(
        &summary_json_path,
        serde_json::to_string_pretty(&summary_json)
            .context("serialize no-pending run summary json")?
            + "\n",
    )
    .with_context(|| format!("write run summary json {}", summary_json_path.display()))
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn write_failed_run_summary(
    plan_name: &str,
    plan_slug: &str,
    passed_tasks: &[ExecutedTask],
    task: &Task,
    attempts: usize,
    review_attempts: usize,
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
        summary.push_str(&format!(
            "  - Commit: {}\n",
            task_commit_display(passed_task)
        ));
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
    let mut failed_review_transcript_display = None;
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
            failed_review_transcript_display = Some(review_transcript_display.to_string());
            summary.push_str(&format!(
                "  - Review transcript: {review_transcript_display}\n"
            ));
        }
    }
    summary.push_str(&format!(
        "  - Commit: {}\n",
        failed_task_commit_status(phase)
    ));
    fs::write(&summary_path, summary)
        .with_context(|| format!("write failed run summary {}", summary_path.display()))?;

    let summary_json_path = run_summary_json_path(plan_slug);
    let passed_json_tasks: Vec<_> = passed_tasks
        .iter()
        .map(|task| {
            let review_status = passed_task_review_status(&task.review_transcript_display);
            json!({
                "number": task.number,
                "title": task.title,
                "status": "passed",
                "accepted": true,
                "attempts": task.attempts,
                "review_attempts": task.review_attempts,
                "transcript": task.transcript_display,
                "validation": task.validation_output_display,
                "review_status": review_status,
                "review_transcript": task.review_transcript_display,
                "commit": task.commit,
                "commit_status": task.commit_status,
                "acceptance_gates": {
                    "agent": "passed",
                    "validation": "passed",
                    "review": review_status,
                    "commit": task.commit_status,
                },
            })
        })
        .collect();
    let review_status = failed_task_review_status(phase, link_review_transcript);
    let commit_status = failed_task_commit_status(phase);
    let failed_task = json!({
        "number": task.number,
        "title": task.title,
        "status": "failed",
        "accepted": false,
        "attempts": attempts,
        "review_attempts": review_attempts,
        "phase": phase,
        "reason": reason,
        "transcript": transcript_display,
        "validation": link_validation_output.then(|| progress.validation_output_display.clone()),
        "review_status": review_status,
        "review_transcript": failed_review_transcript_display,
        "commit": null,
        "commit_status": commit_status,
        "acceptance_gates": failed_task_acceptance_gates(phase, review_status, commit_status),
    });
    let summary_json = json!({
        "plan": plan_name,
        "result": "failed",
        "tasks": passed_json_tasks,
        "failed_task": failed_task,
    });
    fs::write(
        &summary_json_path,
        serde_json::to_string_pretty(&summary_json).context("serialize failed run summary json")?
            + "\n",
    )
    .with_context(|| {
        format!(
            "write failed run summary json {}",
            summary_json_path.display()
        )
    })
}

#[allow(dead_code)]
fn failed_run_error(original: anyhow::Error, summary_result: Result<()>) -> anyhow::Error {
    match summary_result {
        Ok(()) => original,
        Err(summary_err) => anyhow::anyhow!(
            "{original}; additionally failed to write failed run artifacts: {summary_err}"
        ),
    }
}

#[allow(dead_code)]
fn task_commit_display(task: &ExecutedTask) -> &str {
    task.commit.as_deref().unwrap_or("skipped (--no-commit)")
}

#[allow(dead_code)]
fn passed_task_review_status(review_transcript_display: &Option<String>) -> &'static str {
    if review_transcript_display.is_some() {
        "passed"
    } else {
        "skipped"
    }
}

#[allow(dead_code)]
fn failed_task_review_status(phase: &str, link_review_transcript: bool) -> &'static str {
    if phase == "review" || phase == "review retry cleanup" {
        "failed"
    } else if link_review_transcript {
        "passed"
    } else {
        "skipped"
    }
}

#[allow(dead_code)]
fn failed_task_commit_status(phase: &str) -> &'static str {
    if phase == "commit" {
        "failed"
    } else {
        "skipped"
    }
}

#[allow(dead_code)]
fn failed_task_acceptance_gates(
    phase: &str,
    review_status: &str,
    commit_status: &str,
) -> serde_json::Value {
    json!({
        "agent": failed_task_agent_gate(phase),
        "validation": failed_task_validation_gate(phase),
        "review": review_status,
        "commit": commit_status,
    })
}

#[allow(dead_code)]
fn failed_task_agent_gate(phase: &str) -> &'static str {
    if phase == "agent execution" || phase == "agent completion" {
        "failed"
    } else {
        "passed"
    }
}

#[allow(dead_code)]
fn failed_task_validation_gate(phase: &str) -> &'static str {
    match phase {
        "agent execution" | "agent completion" => "skipped",
        "validation" => "failed",
        _ => "passed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_retry_cleanup_failure_keeps_review_gate_failed() {
        assert_eq!(
            failed_task_review_status("review retry cleanup", true),
            "failed"
        );
    }

    #[test]
    fn progress_log_for_review_reports_unavailable_log_without_failing() {
        let missing_log = Path::new("definitely-missing-progress.log");

        let progress_log = progress_log_for_review(missing_log);

        assert!(
            progress_log.contains("progress log unavailable"),
            "{progress_log}"
        );
        assert!(progress_log.contains("failed to read"), "{progress_log}");
        assert!(
            progress_log.contains("definitely-missing-progress.log"),
            "{progress_log}"
        );
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn is_baseline_path(path: &str, baseline_paths: &BTreeSet<String>) -> bool {
    baseline_paths
        .iter()
        .any(|baseline| path == baseline || baseline.ends_with('/') && path.starts_with(baseline))
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn rewrite_no_index_snapshot_paths(patch: &str, temp_path: &str, path: &str) -> String {
    patch
        .replace(&format!("a/{temp_path}"), &format!("a/{path}"))
        .replace(temp_path, path)
}

#[allow(dead_code)]
fn git_cached_path_diff_patch(path: &str) -> Result<String> {
    run_git_allow_exit_codes(&["diff", "--binary", "--cached", "--", path], &[0])
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
struct LastTaskEndStatus {
    failed: bool,
    transcript_display: Option<String>,
    validation_output_available: bool,
    review_transcript_display: Option<String>,
}

#[allow(dead_code)]
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
    let mut latest_validation_output_available = false;
    let mut latest_review_transcript = None;
    let mut last_status = LastTaskEndStatus::default();
    for line in log.lines() {
        let Some(event) = progress_event(line) else {
            continue;
        };
        if event_starts_with_token(event, &task_start_prefix) {
            in_task = true;
            latest_task_transcript = None;
            latest_validation_output_available = false;
            latest_review_transcript = None;
            continue;
        }
        if in_task {
            if let Some(transcript_display) = signal_transcript_display(event) {
                latest_task_transcript = Some(transcript_display.to_string());
            }
            if event.starts_with("validation result=") {
                latest_validation_output_available = true;
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
                // A review transcript can only be written after validation produced output.
                validation_output_available: failed
                    && (latest_validation_output_available || latest_review_transcript.is_some()),
                review_transcript_display: failed
                    .then(|| latest_review_transcript.clone())
                    .flatten(),
            };
            in_task = false;
        }
    }
    Ok(last_status)
}

#[allow(dead_code)]
fn signal_transcript_display(event: &str) -> Option<&str> {
    if !event.starts_with("signal=") {
        return None;
    }
    event
        .split_once("transcript path=")
        .map(|(_, transcript_display)| transcript_display.trim())
        .filter(|transcript_display| !transcript_display.is_empty())
}

#[allow(dead_code)]
fn review_transcript_display(event: &str) -> Option<&str> {
    event
        .strip_prefix("review transcript path=")
        .map(str::trim)
        .filter(|review_transcript_display| !review_transcript_display.is_empty())
}

#[allow(dead_code)]
fn progress_event(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("timestamp=")?;
    rest.split_once(' ').map(|(_, event)| event)
}

#[allow(dead_code)]
fn event_starts_with_token(event: &str, token: &str) -> bool {
    let Some(rest) = event.strip_prefix(token) else {
        return false;
    };
    rest.is_empty() || rest.starts_with(' ')
}

#[allow(dead_code)]
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

pub(crate) fn transcript_without_prompt_echo(transcript: &str, prompt: &str) -> String {
    let mut normalized = transcript.replace("\r\n", "\n").replace('\r', "\n");
    let prompt = prompt.replace("\r\n", "\n").replace('\r', "\n");
    if let Some(start) = normalized.find(&prompt) {
        let end = start + prompt.len();
        normalized.replace_range(start..end, "");
    }
    normalized
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct GitIndexSnapshot {
    tree: String,
}

#[allow(dead_code)]
fn git_index_snapshot() -> Result<GitIndexSnapshot> {
    let tree = run_git(&["write-tree"])
        .context("write current git index to tree")?
        .trim()
        .to_string();
    if tree.is_empty() {
        bail!("git write-tree returned an empty tree id");
    }
    Ok(GitIndexSnapshot { tree })
}

#[allow(dead_code)]
fn restore_git_index_snapshot(snapshot: &GitIndexSnapshot) -> Result<()> {
    run_git(&["read-tree", &snapshot.tree]).context("restore git index from tree snapshot")?;
    Ok(())
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn git_status_paths() -> Result<BTreeSet<String>> {
    git_status_paths_from_porcelain(true)
}

#[allow(dead_code)]
fn git_status_paths_excluding_untracked() -> Result<BTreeSet<String>> {
    git_status_paths_from_porcelain(false)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn collect_retry_cleanup_snapshot(
    root: &Path,
    path: &Path,
    snapshot: &mut RetryCleanupSnapshot,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read directory {}", path.display()))? {
        let entry = entry.with_context(|| format!("read directory entry in {}", path.display()))?;
        let entry_path = entry.path();
        let relative_path = entry_path
            .strip_prefix(root)
            .with_context(|| format!("strip root prefix from {}", entry_path.display()))?
            .to_path_buf();
        if is_retry_cleanup_ignored_path(&relative_path) {
            continue;
        }
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type {}", entry_path.display()))?;
        if file_type.is_dir() {
            let metadata = entry_path
                .symlink_metadata()
                .with_context(|| format!("snapshot directory metadata {}", entry_path.display()))?;
            snapshot
                .dirs
                .insert(relative_path.clone(), metadata.permissions());
            collect_retry_cleanup_snapshot(root, &entry_path, snapshot)?;
        } else if file_type.is_file() {
            let metadata = entry_path
                .symlink_metadata()
                .with_context(|| format!("snapshot file metadata {}", entry_path.display()))?;
            snapshot.files.insert(
                relative_path,
                RetryCleanupFileSnapshot {
                    contents: fs::read(&entry_path)
                        .with_context(|| format!("snapshot file {}", entry_path.display()))?,
                    permissions: metadata.permissions(),
                },
            );
        } else if file_type.is_symlink() {
            snapshot.symlinks.insert(
                relative_path,
                fs::read_link(&entry_path)
                    .with_context(|| format!("snapshot symlink {}", entry_path.display()))?,
            );
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn remove_path_for_restore(path: &Path) -> Result<()> {
    let metadata = path
        .symlink_metadata()
        .with_context(|| format!("stat path before restore {}", path.display()))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("remove directory before restore {}", path.display()))
    } else {
        fs::remove_file(path)
            .with_context(|| format!("remove file before restore {}", path.display()))
    }
}

#[allow(dead_code)]
fn collect_retry_cleanup_paths(
    root: &Path,
    path: &Path,
    baseline_dirs: &BTreeMap<PathBuf, fs::Permissions>,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("read directory {}", path.display()))? {
        let entry = entry.with_context(|| format!("read directory entry in {}", path.display()))?;
        let entry_path = entry.path();
        let relative_path = entry_path
            .strip_prefix(root)
            .with_context(|| format!("strip root prefix from {}", entry_path.display()))?
            .to_path_buf();
        if is_retry_cleanup_ignored_path(&relative_path) {
            continue;
        }
        if entry
            .file_type()
            .with_context(|| format!("read file type {}", entry_path.display()))?
            .is_dir()
            && baseline_dirs.contains_key(&relative_path)
        {
            collect_retry_cleanup_paths(root, &entry_path, baseline_dirs, paths)?;
        }
        paths.push(relative_path);
    }
    Ok(())
}

#[allow(dead_code)]
fn is_retry_cleanup_ignored_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(component)
            if component.as_os_str() == ".ralphterm" || component.as_os_str() == ".git"
    )
}

#[allow(dead_code)]
fn git_inside_work_tree() -> bool {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true"
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        if let Some(validation_output_display) = &resume_context.validation_output_display {
            prompt.push_str("- Previous validation output: ");
            prompt.push_str(validation_output_display);
            prompt.push('\n');
        }
        if let Some(review_transcript_display) = &resume_context.review_transcript_display {
            prompt.push_str("- Previous review transcript: ");
            prompt.push_str(review_transcript_display);
            prompt.push('\n');
        }
    }
    prompt.push_str("\nWhen the task is complete, print COMPLETED.\n");
    prompt
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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
    let progress_log = progress_log_for_review(&progress.log_path);
    let prompt = build_review_prompt(
        plan_name,
        task,
        agent_transcript,
        validation_output,
        &progress_log,
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
            true,
        ));
    }
    if review_run.exit_code != 0 {
        return Err(ReviewCommandError::new(
            format!(
                "review command exited with {} for task {}\n{}",
                review_run.exit_code, task.number, output
            ),
            false,
            true,
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
            true,
        )),
        None => Err(ReviewCommandError::new(
            format!(
                "review did not emit REVIEW_PASS or REVIEW_FAIL for task {}\n{}",
                task.number, output
            ),
            false,
            true,
        )),
    }
}

#[allow(dead_code)]
fn build_review_prompt(
    plan_name: &str,
    task: &Task,
    agent_transcript: &str,
    validation_output: &str,
    progress_log: &str,
    git_diff: &str,
) -> String {
    format!(
        "You are independently reviewing one RalphTerm plan task from {plan_name}.\n\nTask {}: {}\n\nTask body:\n{}\n\nAgent transcript:\n{}\n\nValidation output:\n{}\n\nProgress log:\n{}\n\nCurrent git diff:\n{}\n\n{}\n",
        task.number,
        task.title,
        task.body,
        agent_transcript,
        validation_output,
        progress_log,
        git_diff,
        review_instruction()
    )
}

#[allow(dead_code)]
fn progress_log_for_review(log_path: &Path) -> String {
    fs::read_to_string(log_path).unwrap_or_else(|err| {
        format!(
            "[progress log unavailable: failed to read {}: {err}]\n",
            log_path.display()
        )
    })
}

fn review_instruction() -> &'static str {
    "Print REVIEW_PASS only if the task matches the spec and the validation output supports accepting it. Print REVIEW_FAIL with the reason otherwise."
}

pub(crate) fn review_output_decision(transcript: &str, _prompt: &str) -> Option<bool> {
    let normalized = transcript.replace("\r\n", "\n").replace('\r', "\n");
    let reviewer_output = normalized
        .rfind(review_instruction())
        .map(|start| &normalized[start + review_instruction().len()..])
        .unwrap_or(normalized.as_str());

    reviewer_output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line == "REVIEW_PASS"
                || line == "<<<RALPHTERM:REVIEW_DONE>>>"
                || line == "RALPHTERM:REVIEW_DONE"
                || line == "<<<RALPHTERM:CODEX_REVIEW_DONE>>>"
                || line == "RALPHTERM:CODEX_REVIEW_DONE"
            {
                Some(true)
            } else if line == "REVIEW_FAIL"
                || line.starts_with("REVIEW_FAIL ")
                || line.starts_with("REVIEW_FAIL:")
                || line == "<<<RALPHTERM:TASK_FAILED>>>"
                || line == "RALPHTERM:TASK_FAILED"
            {
                Some(false)
            } else {
                None
            }
        })
        .next_back()
}

/// Extract a stable "category" string from a reviewer failure message so the
/// retry loop can detect stalemates (the same failure recurring). The category
/// is the trimmed reason text after the first REVIEW_FAIL signal line; if the
/// reviewer only emitted the bare signal, the category is empty so all bare
/// REVIEW_FAIL responses count as the same category.
pub(crate) fn review_failure_category(feedback: &str) -> String {
    let normalized = feedback.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.lines() {
        let trimmed = line.trim();
        if let Some(reason) = trimmed.strip_prefix("REVIEW_FAIL:") {
            return reason.trim().to_string();
        }
        if let Some(reason) = trimmed.strip_prefix("REVIEW_FAIL ") {
            return reason.trim().to_string();
        }
        if trimmed == "REVIEW_FAIL" {
            return String::new();
        }
    }
    // Fall back to the first non-empty line.
    normalized
        .lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

fn git_state_for_review() -> String {
    const REVIEW_UNTRACKED_PATCH_LIMIT_BYTES: u64 = 64 * 1024;
    let mut state = String::new();

    append_git_output(&mut state, "Unstaged diff", &["diff", "--"]);
    append_git_output(&mut state, "Staged diff", &["diff", "--cached", "--"]);

    if let Ok(untracked) = git_untracked_files() {
        let untracked_paths: Vec<String> = untracked
            .into_iter()
            .filter(|path| !is_ralphterm_artifact(path))
            .collect();
        if !untracked_paths.is_empty() {
            if !state.ends_with('\n') && !state.is_empty() {
                state.push('\n');
            }
            state.push_str("Untracked files:\n");
            for path in &untracked_paths {
                state.push_str(path);
                state.push('\n');
            }
            state.push_str("Untracked file patches:\n");
            let mut remaining_patch_budget = REVIEW_UNTRACKED_PATCH_LIMIT_BYTES;
            for path in &untracked_paths {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        state.push_str(&format!(
                            "{path} patch omitted: symbolic links are not dereferenced\n"
                        ));
                    }
                    Ok(metadata) if !metadata.is_file() => {
                        state.push_str(&format!("{path} patch omitted: not a regular file\n"));
                    }
                    Ok(metadata) if metadata.len() > REVIEW_UNTRACKED_PATCH_LIMIT_BYTES => {
                        state.push_str(&format!(
                            "{path} patch omitted: file exceeds review prompt limit ({}/{} bytes)\n",
                            metadata.len(),
                            REVIEW_UNTRACKED_PATCH_LIMIT_BYTES
                        ));
                    }
                    Ok(metadata) if metadata.len() > remaining_patch_budget => {
                        state.push_str(&format!(
                            "{path} patch omitted: untracked patch budget exhausted ({}/{} bytes remaining)\n",
                            metadata.len(),
                            remaining_patch_budget
                        ));
                    }
                    Ok(_) => match git_no_index_new_file_patch(path) {
                        Ok(patch) => {
                            let patch_len = patch.len() as u64;
                            if patch_len > remaining_patch_budget {
                                state.push_str(&format!(
                                    "{path} patch omitted: untracked patch budget exhausted ({patch_len}/{} bytes remaining)\n",
                                    remaining_patch_budget
                                ));
                            } else {
                                remaining_patch_budget -= patch_len;
                                state.push_str(&patch);
                            }
                        }
                        Err(err) => state.push_str(&format!("{path} patch unavailable: {err}\n")),
                    },
                    Err(err) => state.push_str(&format!("{path} patch unavailable: {err}\n")),
                }
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

pub(crate) fn run_agent_command_with_timeout(
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

pub(crate) struct SpawnedAgent {
    pub(crate) child: Box<dyn portable_pty::Child + Send + Sync>,
    pub(crate) master: Box<dyn portable_pty::MasterPty + Send>,
}

/// Legacy entrypoint used by `run_agent_command_with_timeout` and
/// `run_smoke`. Spawns the agent AND writes the prompt to the PTY
/// writer + newline. The new async driver in `agent_driver.rs` instead
/// uses `spawn_agent_command_promptless` so it can defer prompt
/// writing until after it sees the REPL is ready (and so it can
/// inject the BEGIN/END protocol preamble).
fn spawn_agent_command(agent_command: &str, prompt: &str) -> Result<SpawnedAgent> {
    let mut parts = parse_agent_command(agent_command)?;
    let command = parts.remove(0);
    // Founding-mission contract for bare `claude` invocation lives in
    // `apply_claude_autoflags` (shared with the promptless spawn used by
    // `agent_driver`): only --dangerously-skip-permissions is auto-added;
    // never --print or -p (Anthropic intends to sunset --print and
    // ralphterm exists to survive that removal). Workspace trust is the
    // operator's responsibility, enforced by `preflight::ensure_workspace_trusted`.
    apply_claude_autoflags(&command, &mut parts);

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

pub fn agent_commands_equivalent(agent_command: &str, review_command: &str) -> Result<bool> {
    Ok(parse_agent_command(agent_command)? == parse_agent_command(review_command)?)
}

/// Same as `spawn_agent_command` but does NOT write the prompt to the
/// PTY after spawn. Callers (notably `agent_driver::drive_agent`) write
/// the prompt themselves so they can: (a) prepend a BEGIN/END protocol
/// preamble with a per-iteration nonce, (b) optionally wait for a REPL
/// ready indicator before pasting keystrokes, (c) keep ownership of the
/// PTY writer for later mid-run input.
pub(crate) fn spawn_agent_command_promptless_with_env(
    agent_command: &str,
    extra_env: &[(&str, &str)],
) -> Result<SpawnedAgent> {
    let mut parts = parse_agent_command(agent_command)?;
    let command = parts.remove(0);
    apply_claude_autoflags(&command, &mut parts);

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
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn agent command")?;
    drop(pair.slave);

    Ok(SpawnedAgent {
        child,
        master: pair.master,
    })
}

fn apply_claude_autoflags(command: &str, parts: &mut Vec<String>) {
    let autoflags_disabled =
        std::env::var_os("RALPHTERM_NO_CLAUDE_AUTOFLAGS").is_some_and(|v| !v.is_empty());
    if autoflags_disabled {
        return;
    }
    let command_basename = std::path::Path::new(command)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| command.to_string());

    match command_basename.as_str() {
        "claude" => {
            // Use `--permission-mode bypassPermissions` rather than
            // `--dangerously-skip-permissions`: the latter shows a
            // blocking one-time safety-acceptance dialog that an
            // autonomous loop cannot answer.
            let already_set = parts.windows(2).any(|w| w[0] == "--permission-mode")
                || parts.iter().any(|a| a == "--dangerously-skip-permissions");
            if !already_set {
                parts.push("--permission-mode".to_string());
                parts.push("bypassPermissions".to_string());
            }
        }
        "codex" => {
            // Codex's interactive REPL gates writes / shell calls behind
            // an approval prompt unless we opt out. --full-auto runs in
            // workspace-write sandbox with --ask-for-approval=never, the
            // closest equivalent to claude's bypassPermissions. Only
            // inject when the operator hasn't already set an approval
            // or sandbox flag (so power users keep control).
            let already_set = parts.iter().any(|a| {
                a == "--full-auto"
                    || a == "--ask-for-approval"
                    || a.starts_with("--ask-for-approval=")
                    || a == "--sandbox"
                    || a.starts_with("--sandbox=")
            });
            if !already_set {
                parts.push("--full-auto".to_string());
            }
        }
        _ => {}
    }
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

/// Returns true when `agent_command` will spawn the official Anthropic
/// `claude` binary (as opposed to a wrapper script or a different
/// binary entirely). Used by preflight to decide whether to enforce
/// the workspace-trust precondition — wrappers and alternate providers
/// have their own trust models.
fn will_invoke_bare_claude(agent_command: &str) -> bool {
    let Ok(parts) = parse_agent_command(agent_command) else {
        return false;
    };
    let Some(first) = parts.first() else {
        return false;
    };
    let basename = std::path::Path::new(first)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| first.clone());
    basename == "claude"
}

fn plan_first_h1(plan_path: &std::path::Path) -> Option<String> {
    let body = std::fs::read_to_string(plan_path).ok()?;
    body.lines()
        .find_map(|l| l.strip_prefix("# ").map(|s| s.trim().to_string()))
}

/// Compute a relative path of `target` against `base` for user-facing
/// display. Falls back to the absolute `target` when it does not live
/// inside `base`. Matches ralphex's habit of printing repo-relative
/// paths in stdout.
fn relpath_from(base: &std::path::Path, target: &std::path::Path) -> std::path::PathBuf {
    target
        .strip_prefix(base)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| target.to_path_buf())
}

fn derive_reviewer_command(explicit: Option<&str>, _repo_root: &std::path::Path) -> Result<String> {
    // CLI/config-provided reviewer wins. `--external-review-tool=custom
    // --custom-review-script <cmd>` and the `review_command =` config keys
    // both land here as `explicit`.
    if let Some(cmd) = explicit {
        if !cmd.trim().is_empty() {
            return Ok(cmd.to_string());
        }
    }
    // Default: bare `codex`. drive_agent spawns it in a real PTY,
    // pastes the prompt as keystrokes, captures the response via the
    // same file-handoff side channel claude uses. apply_claude_autoflags
    // adds --full-auto (codex's equivalent of claude's bypassPermissions)
    // so the loop doesn't gate on an approval dialog.
    //
    // The bundled scripts/wrappers/codex.sh shim still exists for users
    // who explicitly opt in (it uses `codex exec` non-interactive); the
    // default no longer points at it because non-interactive codex
    // contradicts the PTY-native pitch.
    Ok("codex".to_string())
}

fn git_shortstat(repo: &std::path::Path, base: &str) -> Result<(usize, usize, usize)> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--shortstat", &format!("{base}..HEAD")])
        .output()
        .context("git diff --shortstat")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut files = 0usize;
    let mut adds = 0usize;
    let mut dels = 0usize;
    for part in text.split(',') {
        let part = part.trim();
        let num: usize = part
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if part.contains("file") {
            files = num;
        } else if part.contains("insertion") {
            adds = num;
        } else if part.contains("deletion") {
            dels = num;
        }
    }
    Ok((files, adds, dels))
}

fn move_plan_to_completed(plan: &std::path::Path) -> Result<std::path::PathBuf> {
    let parent = plan
        .parent()
        .ok_or_else(|| anyhow::anyhow!("plan has no parent dir"))?;
    let dest_dir = parent.join("completed");
    std::fs::create_dir_all(&dest_dir).with_context(|| format!("create {}", dest_dir.display()))?;
    let dest = dest_dir.join(
        plan.file_name()
            .ok_or_else(|| anyhow::anyhow!("plan has no filename"))?,
    );
    std::fs::rename(plan, &dest).with_context(|| format!("move plan to {}", dest.display()))?;
    Ok(dest)
}

fn agent_timeout_default() -> std::time::Duration {
    std::time::Duration::from_secs(30 * 60)
}

/// Strip ANSI escape sequences (CSI, OSC, simple ESC-prefixed) from a
/// transcript so downstream signal detection and progress logging can
/// operate on plain text.
pub(crate) fn strip_ansi_escapes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'[' {
                // CSI sequence: ESC [ params ... final-byte (0x40..=0x7E)
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            } else if next == b']' {
                // OSC sequence: ESC ] ... BEL or ESC \
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            } else if matches!(next, b'(' | b')' | b'*' | b'+' | b'-' | b'.' | b'/') {
                // Character-set selection: ESC ( char etc.
                i += 2;
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            } else {
                // Generic two-byte escape: ESC X
                i += 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
