use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::Command,
};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::plan::{parse_plan, Task};

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub plan_path: PathBuf,
    pub agent_command: Option<String>,
}

pub fn run_plan(options: RunOptions) -> Result<String> {
    let input = fs::read_to_string(&options.plan_path)
        .with_context(|| format!("read plan {}", options.plan_path.display()))?;
    let plan = parse_plan(&input).context("parse plan")?;
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

    let agent_command = options
        .agent_command
        .unwrap_or_else(|| "claude".to_string());

    for task in pending {
        output.push_str(&format!("Task {}: {}\n", task.number, task.title));
        let prompt = build_task_prompt(plan_name, task, &plan.validation_commands);
        let transcript = run_agent_command(&agent_command, &prompt)
            .with_context(|| format!("run agent for task {}", task.number))?;
        output.push_str(&transcript);
        if !transcript.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&run_validation_commands(&plan.validation_commands)?);
    }

    Ok(output)
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
