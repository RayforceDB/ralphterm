use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

use crate::plan::parse_plan;

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

    let mut output = format!("Pending tasks for {plan_name}:\n");
    for (index, task) in pending.iter().enumerate() {
        output.push_str(&format!(
            "{}. Task {}: {}\n",
            index + 1,
            task.number,
            task.title
        ));
    }

    if let Some(agent_command) = options.agent_command {
        output.push_str(&format!("Agent command: {agent_command}\n"));
    }

    Ok(output)
}
