use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentSignal {
    Completed,
    Failed,
    ReviewDone,
    PlanReady,
    Question,
}

pub fn detect_signal(text: &str) -> Option<AgentSignal> {
    let upper = text.to_ascii_uppercase();
    if upper.contains("ALL_TASKS_DONE") || upper.contains("COMPLETED") {
        return Some(AgentSignal::Completed);
    }
    if upper.contains("REVIEW_DONE") {
        return Some(AgentSignal::ReviewDone);
    }
    if upper.contains("PLAN_READY") {
        return Some(AgentSignal::PlanReady);
    }
    if upper.contains("QUESTION") {
        return Some(AgentSignal::Question);
    }
    if upper.contains("FAILED") {
        return Some(AgentSignal::Failed);
    }
    // Fall back to prefixed signals: <<<RALPHTERM:X>>>.
    if contains_prefixed_signal(&upper, "ALL_TASKS_DONE") {
        return Some(AgentSignal::Completed);
    }
    if contains_prefixed_signal(&upper, "TASK_FAILED") {
        return Some(AgentSignal::Failed);
    }
    if contains_prefixed_signal(&upper, "REVIEW_DONE")
        || contains_prefixed_signal(&upper, "CODEX_REVIEW_DONE")
    {
        return Some(AgentSignal::ReviewDone);
    }
    None
}

fn contains_prefixed_signal(upper_text: &str, signal: &str) -> bool {
    let bracketed = format!("<<<RALPHTERM:{signal}>>>");
    let bare = format!("RALPHTERM:{signal}");
    upper_text.contains(&bracketed) || upper_text.contains(&bare)
}

pub fn detect_approval_request(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("APPROVE?")
        || upper.contains("APPROVE THE")
        || upper.contains("APPROVE THIS")
        || upper.contains("PLEASE APPROVE")
        || upper.contains("NEEDS YOUR APPROVAL")
        || upper.contains("PROCEED?")
        || upper.contains("DO YOU WANT TO PROCEED")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_completion() {
        assert_eq!(
            detect_signal("done\nCOMPLETED"),
            Some(AgentSignal::Completed)
        );
        assert_eq!(
            detect_signal("ALL_TASKS_DONE"),
            Some(AgentSignal::Completed)
        );
    }

    #[test]
    fn detects_review() {
        assert_eq!(detect_signal("REVIEW_DONE"), Some(AgentSignal::ReviewDone));
    }

    #[test]
    fn detects_ralphterm_prefixed_signals() {
        assert_eq!(
            detect_signal("<<<RALPHTERM:ALL_TASKS_DONE>>>"),
            Some(AgentSignal::Completed)
        );
        assert_eq!(
            detect_signal("noise\nRALPHTERM:TASK_FAILED\nmore"),
            Some(AgentSignal::Failed)
        );
        assert_eq!(
            detect_signal("<<<RALPHTERM:REVIEW_DONE>>>"),
            Some(AgentSignal::ReviewDone)
        );
        assert_eq!(
            detect_signal("RALPHTERM:CODEX_REVIEW_DONE"),
            Some(AgentSignal::ReviewDone)
        );
    }

    // Old legacy-RALPHEX-prefix-detector test removed in 0.4.3 alongside
    // the prefix itself. Most "RALPHEX:X" strings either contain a
    // canonical bare signal word (e.g. "FAILED" inside "TASK_FAILED",
    // "REVIEW_DONE") that the first-pass `contains` match still
    // matches, so a negative assertion is misleading. The positive
    // ralphterm-prefix tests above cover the contract.

    #[test]
    fn detects_approval_request_prompts() {
        assert!(detect_approval_request(
            "Claude needs your approval before running this command. Approve?"
        ));
        assert!(detect_approval_request("Do you want to proceed? (y/N)"));
        assert!(detect_approval_request("Approve the command?"));
        assert!(!detect_approval_request(
            "Approval policy documentation was updated."
        ));
        assert_eq!(
            detect_signal("PLAN_READY\nApprove the command?"),
            Some(AgentSignal::PlanReady)
        );
    }
}
