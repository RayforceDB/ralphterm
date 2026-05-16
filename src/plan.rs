use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub validation_commands: Vec<String>,
    pub tasks: Vec<Task>,
}

impl Plan {
    pub fn pending_tasks(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|task| {
                task.checkboxes
                    .iter()
                    .any(|item| item.state == CheckboxState::Open)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub number: usize,
    pub title: String,
    pub body: String,
    pub checkboxes: Vec<CheckboxItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckboxItem {
    pub state: CheckboxState,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckboxState {
    Open,
    Done,
}

pub fn parse_plan(input: &str) -> Result<Plan> {
    let mut validation_commands = Vec::new();
    let mut tasks = Vec::new();
    let mut in_validation = false;
    let mut current: Option<Task> = None;

    for line in input.lines() {
        if line.starts_with("## ") {
            in_validation = line.trim() == "## Validation Commands";
            if !line.starts_with("### ") && line.starts_with("## ") {
                // A top-level section closes any task body.
            }
        }

        if line.starts_with("### ") {
            if let Some(task) = current.take() {
                tasks.push(task);
            }
            in_validation = false;
            if let Some((number, title)) = parse_task_header(line) {
                current = Some(Task {
                    number,
                    title,
                    body: String::new(),
                    checkboxes: Vec::new(),
                });
            }
            continue;
        }

        if in_validation {
            if let Some(command) = parse_command_bullet(line) {
                validation_commands.push(command);
            }
            continue;
        }

        if let Some(task) = current.as_mut() {
            if line.starts_with("## ") {
                if let Some(done) = current.take() {
                    tasks.push(done);
                }
                continue;
            }
            if let Some(item) = parse_checkbox(line) {
                task.checkboxes.push(item);
            }
            task.body.push_str(line);
            task.body.push('\n');
        }
    }

    if let Some(task) = current.take() {
        tasks.push(task);
    }

    if tasks.is_empty() {
        bail!("plan contains no task sections");
    }

    Ok(Plan {
        validation_commands,
        tasks,
    })
}

pub fn mark_task_complete(input: &str, task_number: usize) -> Result<String> {
    let header_prefix = format!("### Task {task_number}:");
    let mut in_target = false;
    let mut marked_open_checkboxes = 0;
    let mut output = Vec::new();

    for line in input.lines() {
        if line.starts_with("### ") {
            in_target = line.starts_with(&header_prefix);
        } else if in_target && line.starts_with("## ") {
            in_target = false;
        }

        if in_target {
            if let Some(updated) = mark_open_checkbox(line) {
                output.push(updated);
                marked_open_checkboxes += 1;
                continue;
            }
        }
        output.push(line.to_string());
    }

    if marked_open_checkboxes == 0 {
        bail!("task {task_number} has no open checkbox to mark complete");
    }

    let mut result = output.join("\n");
    if input.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn mark_open_checkbox(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("- [ ]") {
        return None;
    }
    let checkbox_start = line.len() - trimmed.len();
    let mut updated = line.to_string();
    updated.replace_range(checkbox_start..checkbox_start + 5, "- [x]");
    Some(updated)
}

fn parse_task_header(line: &str) -> Option<(usize, String)> {
    let rest = line.strip_prefix("### Task ")?;
    let (number, title) = rest.split_once(':')?;
    let number = number.trim().parse().ok()?;
    Some((number, title.trim().to_string()))
}

fn parse_command_bullet(line: &str) -> Option<String> {
    let text = line.trim().strip_prefix("- ")?.trim();
    if text.starts_with('`') && text.ends_with('`') && text.len() >= 2 {
        Some(text[1..text.len() - 1].to_string())
    } else {
        Some(text.to_string())
    }
}

fn parse_checkbox(line: &str) -> Option<CheckboxItem> {
    let text = line.trim().strip_prefix("- [")?;
    let (mark, rest) = text.split_once(']')?;
    let state = match mark.trim() {
        "" => CheckboxState::Open,
        "x" | "X" => CheckboxState::Done,
        _ => return None,
    };
    Some(CheckboxItem {
        state,
        text: rest.trim().to_string(),
    })
}
