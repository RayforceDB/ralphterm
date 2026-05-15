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
    None
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
}
