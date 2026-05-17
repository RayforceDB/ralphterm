use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;

use crate::prompts::{substitute, Prompts};

#[derive(Debug, Clone)]
pub enum ReviewOutcome {
    Pass,
    Issues(Vec<String>),
}

pub struct FirstReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
}

const FIRST_REVIEW_AGENTS: &[&str] = &[
    "quality",
    "implementation",
    "testing",
    "simplification",
    "documentation",
];

pub fn first_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, FIRST_REVIEW_AGENTS)
}

const SECOND_REVIEW_AGENTS: &[&str] = &["quality", "implementation"];

pub fn second_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, SECOND_REVIEW_AGENTS)
}

fn run_parallel_review(args: FirstReviewArgs<'_>, agent_names: &[&str]) -> Result<ReviewOutcome> {
    let reviewer_command = args.reviewer_command.to_string();
    let plan_path = args.plan_path.to_path_buf();
    let progress_path = args.progress_path.to_path_buf();
    let default_branch = args.default_branch.to_string();
    let timeout = args.agent_timeout;
    let review_template = args.prompts.review_first.clone();

    let mut handles: Vec<std::thread::JoinHandle<Result<(String, String)>>> = Vec::new();
    for name in agent_names {
        let name = (*name).to_string();
        let agent_template = args.prompts.agents.get(&name).cloned().unwrap_or_default();
        let reviewer = reviewer_command.clone();
        let plan = plan_path.clone();
        let progress = progress_path.clone();
        let default_branch = default_branch.clone();
        let review_template = review_template.clone();
        handles.push(std::thread::spawn(move || {
            run_one_reviewer(
                &reviewer,
                &review_template,
                &agent_template,
                &name,
                &plan,
                &progress,
                &default_branch,
                timeout,
            )
            .map(|transcript| (name, transcript))
        }));
    }

    let mut findings: Vec<String> = Vec::new();
    for h in handles {
        let (name, transcript) = h
            .join()
            .map_err(|_| anyhow::anyhow!("reviewer thread panicked"))??;
        if transcript_has_critical_issues(&transcript) {
            findings.push(format!("[{name}] {}", first_line_of_findings(&transcript)));
        }
    }

    if findings.is_empty() {
        Ok(ReviewOutcome::Pass)
    } else {
        Ok(ReviewOutcome::Issues(findings))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_one_reviewer(
    reviewer_command: &str,
    review_template: &str,
    agent_template: &str,
    agent_name: &str,
    plan_path: &std::path::Path,
    progress_path: &std::path::Path,
    default_branch: &str,
    timeout: Duration,
) -> Result<String> {
    let plan_str = plan_path.to_string_lossy().to_string();
    let progress_str = progress_path.to_string_lossy().to_string();
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("PLAN_FILE", &plan_str);
    vars.insert("PROGRESS_FILE", &progress_str);
    vars.insert("DEFAULT_BRANCH", default_branch);
    vars.insert("AGENT_NAME", agent_name);
    vars.insert("AGENT_INSTRUCTIONS", agent_template);

    let prompt = substitute(review_template, &vars);
    let run = crate::runner::run_agent_command_with_timeout(reviewer_command, &prompt, timeout)?;
    if run.exit_code != 0 {
        anyhow::bail!("reviewer {agent_name} exited with {}", run.exit_code);
    }
    Ok(run.transcript)
}

fn transcript_has_critical_issues(transcript: &str) -> bool {
    let upper = transcript.to_ascii_uppercase();
    upper.contains("CRITICAL") || upper.contains("MAJOR")
}

fn first_line_of_findings(transcript: &str) -> String {
    transcript
        .lines()
        .find(|l| {
            let u = l.to_ascii_uppercase();
            u.contains("CRITICAL") || u.contains("MAJOR")
        })
        .unwrap_or("")
        .trim()
        .to_string()
}

pub struct ExternalReviewArgs<'a> {
    pub prompts: &'a Prompts,
    pub implementer_command: &'a str,
    pub reviewer_command: &'a str,
    pub plan_path: &'a std::path::Path,
    pub progress_path: &'a std::path::Path,
    pub default_branch: &'a str,
    pub agent_timeout: Duration,
    pub max_iterations: usize,
}

pub fn external_review(args: ExternalReviewArgs<'_>) -> Result<ReviewOutcome> {
    // Stub — Task 9 implements this by lifting the fixer loop out of
    // runner::run_plan_external_only.
    let _ = args;
    Ok(ReviewOutcome::Pass)
}
