use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
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

pub async fn first_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, FIRST_REVIEW_AGENTS, "phase 1 first review").await
}

const SECOND_REVIEW_AGENTS: &[&str] = &["quality", "implementation"];

pub async fn second_review(args: FirstReviewArgs<'_>) -> Result<ReviewOutcome> {
    run_parallel_review(args, SECOND_REVIEW_AGENTS, "phase 3 second review").await
}

async fn run_parallel_review(
    args: FirstReviewArgs<'_>,
    agent_names: &[&str],
    phase_label: &str,
) -> Result<ReviewOutcome> {
    let reviewer_command = args.reviewer_command.to_string();
    let plan_path = args.plan_path.to_path_buf();
    let progress_path = args.progress_path.to_path_buf();
    let default_branch = args.default_branch.to_string();
    let timeout = args.agent_timeout;
    let review_template = args.prompts.review_first.clone();
    let repo_root = std::env::current_dir()?;

    let total = agent_names.len();
    let spinner =
        crate::spinner::Spinner::start(format!("{phase_label}: spawning {total} reviewers"));

    let mut handles: Vec<tokio::task::JoinHandle<Result<(String, String, std::time::Duration)>>> =
        Vec::new();
    for name in agent_names {
        let name = (*name).to_string();
        let agent_template = args.prompts.agents.get(&name).cloned().unwrap_or_default();
        let reviewer = reviewer_command.clone();
        let plan = plan_path.clone();
        let progress = progress_path.clone();
        let default_branch = default_branch.clone();
        let review_template = review_template.clone();
        let repo_root = repo_root.clone();
        // Each reviewer feeds its driver events into a no-op sink that
        // only bumps the shared spinner's activity counter. Without
        // this the spinner's "idle Ns" suffix would climb for the
        // entire response time even though bytes are flowing.
        let driver_sink: Option<crate::agent_driver::EventSink> = spinner.as_ref().map(|s| {
            let s = s.clone();
            let sink: crate::agent_driver::EventSink =
                Arc::new(move |_ev: crate::agent_driver::DriverEvent| {
                    s.bump_activity();
                });
            sink
        });
        handles.push(tokio::spawn(async move {
            let started = std::time::Instant::now();
            let transcript = run_one_reviewer(
                &reviewer,
                &review_template,
                &agent_template,
                &name,
                &plan,
                &progress,
                &default_branch,
                timeout,
                &repo_root,
                driver_sink,
            )
            .await?;
            Ok((name, transcript, started.elapsed()))
        }));
    }

    if let Some(s) = spinner.as_ref() {
        s.set_label(format!(
            "{phase_label}: 0/{total} reviewers complete (waiting for first return)"
        ));
    }

    let mut findings: Vec<String> = Vec::new();
    let mut done = 0usize;
    for h in handles {
        let (name, transcript, elapsed) = h
            .await
            .map_err(|e| anyhow::anyhow!("reviewer task panicked: {e}"))??;
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
        done += 1;
        if let Some(s) = spinner.as_ref() {
            if done < total {
                s.set_label(format!(
                    "{phase_label}: {done}/{total} reviewers complete (waiting on {} more)",
                    total - done
                ));
            } else {
                s.set_label(format!("{phase_label}: aggregating findings"));
            }
        }
    }

    if let Some(s) = spinner.as_ref() {
        s.stop();
    }
    drop(spinner);

    if findings.is_empty() {
        Ok(ReviewOutcome::Pass)
    } else {
        Ok(ReviewOutcome::Issues(findings))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_one_reviewer(
    reviewer_command: &str,
    review_template: &str,
    agent_template: &str,
    agent_name: &str,
    plan_path: &std::path::Path,
    progress_path: &std::path::Path,
    default_branch: &str,
    timeout: Duration,
    repo_root: &std::path::Path,
    event_sink: Option<crate::agent_driver::EventSink>,
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
    let run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
        command: reviewer_command,
        task_prompt: &prompt,
        repo_root,
        idle_timeout: timeout,
        cancel: None,
        event_sink,
    })
    .await?;
    if run.timed_out {
        anyhow::bail!("reviewer {agent_name} timed out");
    }
    // Prefer the captured BEGIN..END slice (clean handoff); fall back to
    // the full transcript so legacy reviewers that ignore the protocol
    // still surface their verdict to the decision logic.
    Ok(run
        .captured_response
        .unwrap_or_else(|| crate::runner::transcript_without_prompt_echo(&run.transcript, &prompt)))
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

pub async fn external_review(args: ExternalReviewArgs<'_>) -> Result<ReviewOutcome> {
    let plan_str = args.plan_path.to_string_lossy().to_string();
    let progress_str = args.progress_path.to_string_lossy().to_string();
    let default_branch = args.default_branch.to_string();
    let repo_root: PathBuf = std::env::current_dir()?;

    let mut last_category: Option<String> = None;
    let mut consecutive = 0usize;

    let spinner = crate::spinner::Spinner::start(format!(
        "phase 2 external review: spawning reviewer (1/{})",
        args.max_iterations
    ));

    for iteration in 1..=args.max_iterations {
        if let Some(s) = spinner.as_ref() {
            s.set_label(format!(
                "phase 2 external review: reviewing ({iteration}/{})",
                args.max_iterations
            ));
        }

        let mut vars: HashMap<&str, &str> = HashMap::new();
        vars.insert("PLAN_FILE", &plan_str);
        vars.insert("PROGRESS_FILE", &progress_str);
        vars.insert("DEFAULT_BRANCH", &default_branch);

        let review_prompt = substitute(&args.prompts.codex_review, &vars);
        let bumper: Option<crate::agent_driver::EventSink> = spinner.as_ref().map(|s| {
            let s = s.clone();
            let sink: crate::agent_driver::EventSink = Arc::new(move |_ev| s.bump_activity());
            sink
        });
        let review_run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
            command: args.reviewer_command,
            task_prompt: &review_prompt,
            repo_root: &repo_root,
            idle_timeout: args.agent_timeout,
            cancel: None,
            event_sink: bumper,
        })
        .await?;
        if review_run.timed_out {
            anyhow::bail!("external reviewer iteration {iteration} timed out");
        }

        // Prefer the captured BEGIN..END slice; fall back to the
        // ANSI-prompt-stripped full transcript so legacy reviewers still
        // surface their verdict.
        let agent_transcript = review_run.captured_response.clone().unwrap_or_else(|| {
            crate::runner::transcript_without_prompt_echo(&review_run.transcript, &review_prompt)
        });
        let decision = external_review_decision(&agent_transcript);
        match decision {
            Some(true) => {
                if let Some(s) = spinner.as_ref() {
                    s.stop();
                }
                drop(spinner);
                return Ok(ReviewOutcome::Pass);
            }
            Some(false) => {
                let category = crate::runner::review_failure_category(&review_run.transcript);
                if last_category.as_deref() == Some(category.as_str()) {
                    consecutive += 1;
                } else {
                    consecutive = 1;
                    last_category = Some(category.clone());
                }
                if consecutive >= 3 {
                    if let Some(s) = spinner.as_ref() {
                        s.stop();
                    }
                    drop(spinner);
                    return Ok(ReviewOutcome::Issues(vec![format!(
                        "external review stalemate: category '{category}' repeated {consecutive} times"
                    )]));
                }
                if let Some(s) = spinner.as_ref() {
                    s.set_label(format!(
                        "phase 2 external review: reviewer flagged {category}, dispatching fixer ({iteration}/{})",
                        args.max_iterations
                    ));
                }
                let findings = format!("Previous external review failed: {category}");
                let mut fix_vars = vars.clone();
                fix_vars.insert("REVIEW_FINDINGS", &findings);
                let fix_prompt = substitute(&args.prompts.codex, &fix_vars);
                let fix_bumper: Option<crate::agent_driver::EventSink> =
                    spinner.as_ref().map(|s| {
                        let s = s.clone();
                        let sink: crate::agent_driver::EventSink =
                            Arc::new(move |_ev| s.bump_activity());
                        sink
                    });
                let fix_run = crate::agent_driver::drive_agent(crate::agent_driver::AgentSpec {
                    command: args.implementer_command,
                    task_prompt: &fix_prompt,
                    repo_root: &repo_root,
                    idle_timeout: args.agent_timeout,
                    cancel: None,
                    event_sink: fix_bumper,
                })
                .await?;
                if fix_run.timed_out {
                    anyhow::bail!("external review fixer iteration {iteration} timed out");
                }
            }
            None => {
                anyhow::bail!(
                    "external review iteration {iteration} did not emit REVIEW_PASS or REVIEW_FAIL"
                );
            }
        }
    }

    if let Some(s) = spinner.as_ref() {
        s.stop();
    }
    drop(spinner);

    Ok(ReviewOutcome::Issues(vec![format!(
        "external review exhausted {} iterations without REVIEW_PASS",
        args.max_iterations
    )]))
}
