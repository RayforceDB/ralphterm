use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::plan::{parse_plan, Task};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub agent_command: Option<String>,
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
    let plan_slug = plan_slug(&options.plan_path);
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
        let transcript = run_agent_command(&agent_command, &prompt)
            .with_context(|| format!("run agent for task {}", task.number))?;
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
        match run_validation_commands(&plan.validation_commands) {
            Ok(validation_output) => output.push_str(&validation_output),
            Err(err) => {
                append_progress(&progress.log_path, "validation result=failed")?;
                append_progress(
                    &progress.log_path,
                    &format!("task_end number={} result=failed", task.number),
                )?;
                return Err(err);
            }
        }
        append_progress(&progress.log_path, "validation result=passed")?;
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

    Ok(output)
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
    transcript_display: String,
}

struct ExecutedTask {
    number: usize,
    title: String,
    transcript_display: String,
}

impl ProgressPaths {
    fn new(plan_slug: &str, task_number: usize) -> Result<Self> {
        let progress_dir = PathBuf::from(".ralphterm").join("progress");
        fs::create_dir_all(&progress_dir).context("create progress directory")?;
        let log_path = progress_dir.join(format!("{plan_slug}.log"));
        let transcript_path =
            progress_dir.join(format!("{plan_slug}-task-{task_number}.transcript"));
        let transcript_display = transcript_path.to_string_lossy().into_owned();
        Ok(Self {
            log_path,
            transcript_path,
            transcript_display,
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

fn write_run_summary(plan_name: &str, plan_slug: &str, tasks: &[ExecutedTask]) -> Result<()> {
    let progress_dir = PathBuf::from(".ralphterm").join("progress");
    fs::create_dir_all(&progress_dir).context("create progress directory")?;
    let summary_path = progress_dir.join(format!("{plan_slug}-summary.md"));
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

fn run_git(args: &[&str]) -> Result<String> {
    run_git_with_paths(args, &[])
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

fn run_agent_command(agent_command: &str, prompt: &str) -> Result<String> {
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

    let mut child = pair
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

    let mut reader = pair.master.try_clone_reader().context("clone pty reader")?;
    let mut transcript = String::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => transcript.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(err) => bail!("read pty: {err}"),
        }
    }

    let status = child.wait().context("wait for agent command")?;
    let code = status.exit_code();
    if code != 0 {
        bail!("agent command exited with {code}: {transcript}");
    }

    Ok(transcript)
}
