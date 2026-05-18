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

    let mut handles: Vec<std::thread::JoinHandle<Result<(String, String, std::time::Duration)>>> =
        Vec::new();
    for name in agent_names {
        let name = (*name).to_string();
        let agent_template = args.prompts.agents.get(&name).cloned().unwrap_or_default();
        let reviewer = reviewer_command.clone();
        let plan = plan_path.clone();
        let progress = progress_path.clone();
        let default_branch = default_branch.clone();
        let review_template = review_template.clone();
        handles.push(std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let res = run_one_reviewer(
                &reviewer,
                &review_template,
                &agent_template,
                &name,
                &plan,
                &progress,
                &default_branch,
                timeout,
            );
            res.map(|transcript| (name, transcript, started.elapsed()))
        }));
    }

    let mut findings: Vec<String> = Vec::new();
    for h in handles {
        let (name, transcript, elapsed) = h
            .join()
            .map_err(|_| anyhow::anyhow!("reviewer thread panicked"))??;
        // Per-agent heartbeat so the user sees individual reviewers
        // returning instead of waiting silently for the whole phase.
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        println!(
            "[{ts}]   reviewer '{name}' returned in {}s",
            elapsed.as_secs()
        );
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
    // Strip the prompt echo so substring matching on "CRITICAL"/"MAJOR"
    // sees only the agent's actual findings, not the literal "# CRITICAL:"
    // instructions baked into review_first.txt that claude echoes back in
    // --print mode.
    Ok(crate::runner::transcript_without_prompt_echo(
        &run.transcript,
        &prompt,
    ))
}

/// Decide whether an external (codex) reviewer transcript says the
/// codebase is clean. The vendored codex_review.txt template asks the
/// reviewer to say "NO ISSUES FOUND" for clean reviews and otherwise
/// list findings with file:line refs. Older protocols used REVIEW_PASS
/// / REVIEW_FAIL — we still recognise those so users can override the
/// prompt with a stricter contract.
///
/// Returns:
///   Some(true)  – clean ("NO ISSUES FOUND" or REVIEW_PASS)
///   Some(false) – explicit failure (REVIEW_FAIL or critical/major
///                 findings detected by `transcript_has_critical_issues`)
///   None        – ambiguous; caller decides (we currently treat None as
///                 pass to avoid blocking on reviewers that respond
///                 narratively without any of these markers).
fn external_review_decision(transcript: &str) -> Option<bool> {
    let upper = transcript.to_ascii_uppercase();
    if upper.contains("REVIEW_FAIL") {
        return Some(false);
    }
    if upper.contains("REVIEW_PASS") {
        return Some(true);
    }
    if upper.contains("NO ISSUES FOUND")
        || upper.contains("NO ISSUES.")
        || upper.contains("NO ISSUES\n")
    {
        return Some(true);
    }
    if transcript_has_critical_issues(transcript) {
        return Some(false);
    }
    // No explicit marker. Treat as pass — the reviewer ran, exited 0,
    // and didn't surface critical findings.
    Some(true)
}

fn transcript_has_critical_issues(transcript: &str) -> bool {
    // Only treat findings-style markers as critical, never the bare word.
    // Common shapes claude/codex actually emit when reporting an issue:
    //   - "Severity: critical"
    //   - "[CRITICAL]" / "**CRITICAL**" / "Critical:" at the start of a line
    //   - "CRITICAL ISSUE" / "CRITICAL FINDING" / "CRITICAL BUG"
    // We deliberately do NOT match the bare word "critical" anywhere in
    // the transcript because review prompts use it for procedural framing
    // (e.g. "CRITICAL: Do NOT proceed to Step 3 until ...") and the agent
    // tends to quote that phrasing back even when the underlying review
    // finds no problems.
    transcript.lines().any(line_flags_issue)
}

fn line_flags_issue(line: &str) -> bool {
    let trimmed = line.trim();
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("SEVERITY:") {
        return upper.contains("CRITICAL") || upper.contains("MAJOR");
    }
    for marker in [
        "[CRITICAL]",
        "[MAJOR]",
        "**CRITICAL**",
        "**MAJOR**",
        "CRITICAL ISSUE",
        "CRITICAL BUG",
        "CRITICAL FINDING",
        "MAJOR ISSUE",
        "MAJOR BUG",
        "MAJOR FINDING",
    ] {
        if upper.contains(marker) {
            return true;
        }
    }
    // "Critical:" / "Major:" at the start of a line (common bullet style).
    if upper.starts_with("CRITICAL:") || upper.starts_with("MAJOR:") {
        return true;
    }
    false
}

fn first_line_of_findings(transcript: &str) -> String {
    transcript
        .lines()
        .find(|line| line_flags_issue(line))
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
    let plan_str = args.plan_path.to_string_lossy().to_string();
    let progress_str = args.progress_path.to_string_lossy().to_string();
    let default_branch = args.default_branch.to_string();

    let mut last_category: Option<String> = None;
    let mut consecutive = 0usize;

    for iteration in 1..=args.max_iterations {
        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_str);
        vars.insert("DEFAULT_BRANCH", &default_branch);

        let review_prompt = substitute(&args.prompts.codex_review, &vars);
        let review_run = crate::runner::run_agent_command_with_timeout(
            args.reviewer_command,
            &review_prompt,
            args.agent_timeout,
        )?;
        if review_run.exit_code != 0 {
            anyhow::bail!(
                "external reviewer iteration {iteration} exited with {}",
                review_run.exit_code
            );
        }

        // The codex_review.txt prompt template doesn't ask for
        // REVIEW_PASS/REVIEW_FAIL — it asks codex to print "NO ISSUES
        // FOUND" when the review is clean and otherwise list findings.
        // Strip the prompt echo first so substring matches don't pick up
        // the instructions themselves.
        let agent_transcript =
            crate::runner::transcript_without_prompt_echo(&review_run.transcript, &review_prompt);
        let decision = external_review_decision(&agent_transcript);
        match decision {
            Some(true) => return Ok(ReviewOutcome::Pass),
            Some(false) => {
                let category = crate::runner::review_failure_category(&review_run.transcript);
                if last_category.as_deref() == Some(category.as_str()) {
                    consecutive += 1;
                } else {
                    consecutive = 1;
                    last_category = Some(category.clone());
                }
                if consecutive >= 3 {
                    return Ok(ReviewOutcome::Issues(vec![format!(
                        "external review stalemate: category '{category}' repeated {consecutive} times"
                    )]));
                }
                let findings = format!("Previous external review failed: {category}");
                let mut fix_vars = vars.clone();
                fix_vars.insert("REVIEW_FINDINGS", &findings);
                let fix_prompt = substitute(&args.prompts.codex, &fix_vars);
                let fix_run = crate::runner::run_agent_command_with_timeout(
                    args.implementer_command,
                    &fix_prompt,
                    args.agent_timeout,
                )?;
                if fix_run.exit_code != 0 {
                    anyhow::bail!(
                        "external review fixer iteration {iteration} exited with {}",
                        fix_run.exit_code
                    );
                }
            }
            None => {
                anyhow::bail!(
                    "external review iteration {iteration} did not emit REVIEW_PASS or REVIEW_FAIL"
                );
            }
        }
    }

    Ok(ReviewOutcome::Issues(vec![format!(
        "external review exhausted {} iterations without REVIEW_PASS",
        args.max_iterations
    )]))
}
