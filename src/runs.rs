use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunPhase {
    Planning,
    Executing,
    Reviewing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Created,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub phase: RunPhase,
    pub status: RunStatus,
    pub plan_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedRunRecord {
    pub id: Uuid,
    pub created_at: String,
    pub phase: RunPhase,
    pub status: RunStatus,
    pub plan_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub status: RunStatus,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProgressEvent {
    pub event_type: String,
    pub task_number: Option<usize>,
    pub task_title: Option<String>,
    pub attempt: Option<usize>,
    pub artifact_path: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResultArtifacts {
    pub summary_markdown: String,
    pub summary_json: Option<String>,
    pub diff_patch: String,
}

pub struct RunStore;

impl RunStore {
    pub fn create(base_dir: impl AsRef<Path>, request: RunRecord) -> Result<CreatedRunRecord> {
        let now = timestamp();
        let record = CreatedRunRecord {
            id: Uuid::new_v4(),
            created_at: now.clone(),
            phase: request.phase,
            status: request.status,
            plan_path: request.plan_path,
            workspace_path: request.workspace_path,
        };
        let run_dir = base_dir
            .as_ref()
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("create run directory {}", run_dir.display()))?;

        let run_json_path = run_dir.join("run.json");
        write_record_atomically(&run_json_path, &record)?;

        let event = RunEvent {
            event_type: "run_created".to_string(),
            status: record.status.clone(),
            timestamp: now,
            task_number: None,
            task_title: None,
            attempt: None,
            artifact_path: None,
            message: None,
        };
        let event_json = serde_json::to_string(&event).context("serialize run event")?;
        let mut events = OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join("events.jsonl"))
            .with_context(|| format!("open {}", run_dir.join("events.jsonl").display()))?;
        writeln!(events, "{event_json}").context("append run event")?;

        Ok(record)
    }

    pub fn list(base_dir: impl AsRef<Path>) -> Result<Vec<CreatedRunRecord>> {
        let runs_dir = base_dir.as_ref().join(".ralphterm").join("runs");
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&runs_dir)
            .with_context(|| format!("read runs directory {}", runs_dir.display()))?
        {
            let entry = entry.with_context(|| format!("read entry in {}", runs_dir.display()))?;
            if !entry.file_type().context("read run entry type")?.is_dir() {
                continue;
            }
            let run_json_path = entry.path().join("run.json");
            if !run_json_path.exists() {
                continue;
            }
            records.push(read_record(&run_json_path)?);
        }
        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(records)
    }

    pub fn get(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<CreatedRunRecord>> {
        let run_json_path = run_dir(base_dir.as_ref(), id).join("run.json");
        if !run_json_path.exists() {
            return Ok(None);
        }
        read_record(&run_json_path).map(Some)
    }

    pub fn events(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<Vec<RunEvent>>> {
        if Self::get(base_dir.as_ref(), id)?.is_none() {
            return Ok(None);
        }
        let events_path = run_dir(base_dir.as_ref(), id).join("events.jsonl");
        if !events_path.exists() {
            return Ok(Some(Vec::new()));
        }
        let input = fs::read_to_string(&events_path)
            .with_context(|| format!("read {}", events_path.display()))?;
        let mut events = Vec::new();
        for line in input.lines().filter(|line| !line.trim().is_empty()) {
            events.push(serde_json::from_str(line).context("parse run event")?);
        }
        Ok(Some(events))
    }

    pub fn summary_path(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<PathBuf>> {
        Self::artifact_path(base_dir, id, "summary.md")
    }

    pub fn summary_json_path(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<PathBuf>> {
        Self::artifact_path(base_dir, id, "summary.json")
    }

    pub fn diff_path(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<PathBuf>> {
        Self::artifact_path(base_dir, id, "diff.patch")
    }

    fn artifact_path(base_dir: impl AsRef<Path>, id: Uuid, name: &str) -> Result<Option<PathBuf>> {
        if Self::get(base_dir.as_ref(), id)?.is_none() {
            return Ok(None);
        }

        Ok(Some(run_dir(base_dir.as_ref(), id).join(name)))
    }

    pub fn start(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<CreatedRunRecord>> {
        let _guard = record_mutation_lock()
            .lock()
            .expect("run record lock poisoned");
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        record.phase = RunPhase::Executing;
        record.status = RunStatus::Running;
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: "run_started".to_string(),
                status: record.status.clone(),
                timestamp: timestamp(),
                task_number: None,
                task_title: None,
                attempt: None,
                artifact_path: None,
                message: None,
            },
        )?;
        write_record(base_dir.as_ref(), &record)?;
        Ok(Some(record))
    }

    pub fn cancel(base_dir: impl AsRef<Path>, id: Uuid) -> Result<Option<CreatedRunRecord>> {
        let _guard = record_mutation_lock()
            .lock()
            .expect("run record lock poisoned");
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        if matches!(record.status, RunStatus::Succeeded | RunStatus::Failed) {
            return Ok(Some(record));
        }

        let dir = run_dir(base_dir.as_ref(), id);
        let summary_path = dir.join("summary.md");
        fs::write(&summary_path, default_cancelled_summary(&record))
            .with_context(|| format!("write {}", summary_path.display()))?;
        let diff_path = dir.join("diff.patch");
        fs::write(&diff_path, "").with_context(|| format!("write {}", diff_path.display()))?;

        record.phase = RunPhase::Complete;
        record.status = RunStatus::Failed;
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: "run_cancelled".to_string(),
                status: record.status.clone(),
                timestamp: timestamp(),
                task_number: None,
                task_title: None,
                attempt: None,
                artifact_path: None,
                message: None,
            },
        )?;
        write_record(base_dir.as_ref(), &record)?;
        Ok(Some(record))
    }

    pub fn write_result(
        base_dir: impl AsRef<Path>,
        id: Uuid,
        artifacts: RunResultArtifacts,
    ) -> Result<Option<CreatedRunRecord>> {
        let _guard = record_mutation_lock()
            .lock()
            .expect("run record lock poisoned");
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        if record.status != RunStatus::Running {
            return Ok(Some(record));
        }

        let dir = run_dir(base_dir.as_ref(), id);
        let summary_path = dir.join("summary.md");
        fs::write(&summary_path, artifacts.summary_markdown)
            .with_context(|| format!("write {}", summary_path.display()))?;
        if let Some(summary_json) = artifacts.summary_json {
            let summary_json_path = dir.join("summary.json");
            fs::write(&summary_json_path, summary_json)
                .with_context(|| format!("write {}", summary_json_path.display()))?;
        }
        let diff_path = dir.join("diff.patch");
        fs::write(&diff_path, artifacts.diff_patch)
            .with_context(|| format!("write {}", diff_path.display()))?;

        record.phase = RunPhase::Complete;
        record.status = RunStatus::Succeeded;
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: "run_succeeded".to_string(),
                status: record.status.clone(),
                timestamp: timestamp(),
                task_number: None,
                task_title: None,
                attempt: None,
                artifact_path: None,
                message: None,
            },
        )?;
        write_record(base_dir.as_ref(), &record)?;
        Ok(Some(record))
    }

    pub fn write_failure(
        base_dir: impl AsRef<Path>,
        id: Uuid,
        summary_markdown: Option<String>,
        summary_json: Option<String>,
        diff_patch: Option<String>,
    ) -> Result<Option<CreatedRunRecord>> {
        let _guard = record_mutation_lock()
            .lock()
            .expect("run record lock poisoned");
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        if record.status != RunStatus::Running {
            return Ok(Some(record));
        }

        let dir = run_dir(base_dir.as_ref(), id);
        let summary_path = dir.join("summary.md");
        fs::write(
            &summary_path,
            summary_markdown.unwrap_or_else(|| default_failure_summary(&record)),
        )
        .with_context(|| format!("write {}", summary_path.display()))?;
        if let Some(summary_json) = summary_json {
            let summary_json_path = dir.join("summary.json");
            fs::write(&summary_json_path, summary_json)
                .with_context(|| format!("write {}", summary_json_path.display()))?;
        }
        let diff_path = dir.join("diff.patch");
        fs::write(&diff_path, diff_patch.unwrap_or_default())
            .with_context(|| format!("write {}", diff_path.display()))?;

        record.phase = RunPhase::Complete;
        record.status = RunStatus::Failed;
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: "run_failed".to_string(),
                status: record.status.clone(),
                timestamp: timestamp(),
                task_number: None,
                task_title: None,
                attempt: None,
                artifact_path: None,
                message: None,
            },
        )?;
        write_record(base_dir.as_ref(), &record)?;
        Ok(Some(record))
    }

    pub fn append_progress_event(
        base_dir: impl AsRef<Path>,
        id: Uuid,
        event: RunProgressEvent,
    ) -> Result<Option<()>> {
        let _guard = record_mutation_lock()
            .lock()
            .expect("run record lock poisoned");
        let Some(mut record) = Self::get(base_dir.as_ref(), id)? else {
            return Ok(None);
        };
        if record.status != RunStatus::Running {
            return Ok(None);
        }
        let next_phase = match event.event_type.as_str() {
            "review_started" => Some(RunPhase::Reviewing),
            "review_passed" | "review_failed" | "agent_retry_started" => Some(RunPhase::Executing),
            _ => None,
        };
        if let Some(next_phase) = next_phase {
            if record.phase != next_phase {
                record.phase = next_phase;
                write_record(base_dir.as_ref(), &record)?;
            }
        }
        append_event(
            base_dir.as_ref(),
            id,
            RunEvent {
                event_type: event.event_type,
                status: record.status,
                timestamp: timestamp(),
                task_number: event.task_number,
                task_title: event.task_title,
                attempt: event.attempt,
                artifact_path: event.artifact_path,
                message: event.message,
            },
        )?;
        Ok(Some(()))
    }
}

fn record_mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run_dir(base_dir: &Path, id: Uuid) -> std::path::PathBuf {
    base_dir
        .join(".ralphterm")
        .join("runs")
        .join(id.to_string())
}

fn read_record(path: &Path) -> Result<CreatedRunRecord> {
    let input = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&input).with_context(|| format!("parse {}", path.display()))
}

fn write_record(base_dir: &Path, record: &CreatedRunRecord) -> Result<()> {
    let path = run_dir(base_dir, record.id).join("run.json");
    write_record_atomically(&path, record)
}

fn write_record_atomically(path: &Path, record: &CreatedRunRecord) -> Result<()> {
    let run_json = serde_json::to_string_pretty(record).context("serialize run record")?;
    let content = format!("{run_json}\n");
    let parent = path
        .parent()
        .with_context(|| format!("resolve parent directory for {}", path.display()))?;
    let temp_path = parent.join(format!(
        ".run.json.{}.{}.tmp",
        std::process::id(),
        Uuid::new_v4()
    ));

    let write_result = (|| -> Result<()> {
        let mut temp_file = File::create(&temp_path)
            .with_context(|| format!("create temporary run record {}", temp_path.display()))?;
        temp_file
            .write_all(content.as_bytes())
            .with_context(|| format!("write temporary run record {}", temp_path.display()))?;
        temp_file
            .sync_all()
            .with_context(|| format!("sync temporary run record {}", temp_path.display()))?;
        drop(temp_file);

        fs::rename(&temp_path, path)
            .with_context(|| format!("rename {} to {}", temp_path.display(), path.display()))?;
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result.with_context(|| format!("write {}", path.display()))
}

fn append_event(base_dir: &Path, id: Uuid, event: RunEvent) -> Result<()> {
    let path = run_dir(base_dir, id).join("events.jsonl");
    let event_json = serde_json::to_string(&event).context("serialize run event")?;
    let mut events = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(events, "{event_json}").context("append run event")
}

fn default_failure_summary(record: &CreatedRunRecord) -> String {
    let plan = record.plan_path.as_deref().unwrap_or("unknown plan");
    format!("# Run Summary\n\nResult: failed\n\nPlan: {plan}\n")
}

fn default_cancelled_summary(record: &CreatedRunRecord) -> String {
    let plan = record.plan_path.as_deref().unwrap_or("unknown plan");
    format!("# Run Summary\n\nResult: failed (cancelled)\n\nPlan: {plan}\n")
}

fn timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis();
    format!("unix-ms:{millis}")
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
    };

    use serde_json::Value;

    use super::{
        read_record, write_record, CreatedRunRecord, RunPhase, RunRecord, RunResultArtifacts,
        RunStatus, RunStore,
    };

    #[test]
    fn readers_never_observe_partial_run_json_during_updates() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-atomic-record-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Executing,
                status: RunStatus::Running,
                plan_path: Some("plans/task.md".into()),
                workspace_path: Some("initial".into()),
            },
        )
        .unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let run_json = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string())
            .join("run.json");
        let reader = thread::spawn(move || {
            while !reader_stop.load(Ordering::Acquire) {
                read_record(&run_json).expect("reader must only observe complete run.json records");
            }
        });

        let mut updated = CreatedRunRecord {
            workspace_path: Some("x".repeat(4 * 1024 * 1024)),
            ..record.clone()
        };
        for iteration in 0..80 {
            updated.status = if iteration % 2 == 0 {
                RunStatus::Running
            } else {
                RunStatus::Succeeded
            };
            write_record(&temp, &updated).unwrap();
        }
        stop.store(true, Ordering::Release);
        reader.join().unwrap();

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn create_writes_run_json_and_initial_event() {
        let temp =
            std::env::temp_dir().join(format!("ralphterm-run-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Planning,
                status: RunStatus::Created,
                plan_path: Some("docs/plan.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();

        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        let run_json = fs::read_to_string(run_dir.join("run.json")).unwrap();
        let persisted: Value = serde_json::from_str(&run_json).unwrap();
        assert_eq!(persisted["id"], record.id.to_string());
        assert_eq!(persisted["phase"], "planning");
        assert_eq!(persisted["status"], "created");
        assert_eq!(persisted["plan_path"], "docs/plan.md");
        assert!(!persisted["created_at"].as_str().unwrap().is_empty());

        let events = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let lines: Vec<_> = events.lines().collect();
        assert_eq!(lines.len(), 1);
        let event: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event["type"], "run_created");
        assert_eq!(event["status"], "created");
        assert!(!event["timestamp"].as_str().unwrap().is_empty());

        fs::remove_file(run_dir.join("events.jsonl")).unwrap();
        let missing_events = RunStore::events(&temp, record.id).unwrap().unwrap();
        assert!(
            missing_events.is_empty(),
            "missing event log for an existing run should return an empty event list"
        );

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn append_progress_event_skips_cancelled_runs() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-progress-cancelled-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Planning,
                status: RunStatus::Created,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();
        RunStore::start(&temp, record.id).unwrap().unwrap();
        RunStore::cancel(&temp, record.id).unwrap().unwrap();

        let appended = RunStore::append_progress_event(
            &temp,
            record.id,
            super::RunProgressEvent {
                event_type: "task_succeeded".into(),
                task_number: Some(1),
                task_title: Some("Create first file".into()),
                attempt: None,
                artifact_path: None,
                message: None,
            },
        )
        .unwrap();
        assert_eq!(appended, None);

        let events = RunStore::events(&temp, record.id).unwrap().unwrap();
        let event_types: Vec<_> = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect();
        assert_eq!(
            event_types,
            vec!["run_created", "run_started", "run_cancelled"]
        );

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn start_marks_run_running_executing_and_appends_event() {
        let temp =
            std::env::temp_dir().join(format!("ralphterm-run-start-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Planning,
                status: RunStatus::Created,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();

        let updated = RunStore::start(&temp, record.id)
            .unwrap()
            .expect("existing run should be updated");

        assert_eq!(updated.id, record.id);
        assert_eq!(updated.phase, RunPhase::Executing);
        assert_eq!(updated.status, RunStatus::Running);

        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        let run_json = fs::read_to_string(run_dir.join("run.json")).unwrap();
        let persisted: Value = serde_json::from_str(&run_json).unwrap();
        assert_eq!(persisted["phase"], "executing");
        assert_eq!(persisted["status"], "running");

        let events = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let lines: Vec<_> = events.lines().collect();
        assert_eq!(lines.len(), 2);
        let event: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(event["type"], "run_started");
        assert_eq!(event["status"], "running");

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn write_result_writes_artifacts_marks_succeeded_and_appends_event() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-result-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Executing,
                status: RunStatus::Running,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();

        let updated = RunStore::write_result(
            &temp,
            record.id,
            RunResultArtifacts {
                summary_markdown: "# Summary\n\nDone.\n".into(),
                summary_json: Some("{\"result\":\"passed\"}\n".into()),
                diff_patch: "diff --git a/file b/file\n".into(),
            },
        )
        .unwrap()
        .expect("existing run should be updated");

        assert_eq!(updated.id, record.id);
        assert_eq!(updated.created_at, record.created_at);
        assert_eq!(updated.plan_path, record.plan_path);
        assert_eq!(updated.phase, RunPhase::Complete);
        assert_eq!(updated.status, RunStatus::Succeeded);

        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        assert_eq!(
            fs::read_to_string(run_dir.join("summary.md")).unwrap(),
            "# Summary\n\nDone.\n"
        );
        assert_eq!(
            fs::read_to_string(run_dir.join("diff.patch")).unwrap(),
            "diff --git a/file b/file\n"
        );

        let run_json = fs::read_to_string(run_dir.join("run.json")).unwrap();
        let persisted: Value = serde_json::from_str(&run_json).unwrap();
        assert_eq!(persisted["phase"], "complete");
        assert_eq!(persisted["status"], "succeeded");

        let events = fs::read_to_string(run_dir.join("events.jsonl")).unwrap();
        let lines: Vec<_> = events.lines().collect();
        assert_eq!(lines.len(), 2);
        let event: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(event["type"], "run_succeeded");
        assert_eq!(event["status"], "succeeded");
        assert!(!event["timestamp"].as_str().unwrap().is_empty());

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn cancel_succeeded_run_preserves_terminal_status_and_artifacts() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-cancel-succeeded-preserves-artifacts-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Executing,
                status: RunStatus::Running,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();
        RunStore::write_result(
            &temp,
            record.id,
            RunResultArtifacts {
                summary_markdown: "# Summary\n\nSucceeded artifact.\n".into(),
                summary_json: None,
                diff_patch: "diff --git a/file b/file\n".into(),
            },
        )
        .unwrap()
        .unwrap();

        let updated = RunStore::cancel(&temp, record.id)
            .unwrap()
            .expect("existing terminal run should be returned");

        assert_eq!(updated.phase, RunPhase::Complete);
        assert_eq!(updated.status, RunStatus::Succeeded);
        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        assert_eq!(
            fs::read_to_string(run_dir.join("summary.md")).unwrap(),
            "# Summary\n\nSucceeded artifact.\n"
        );
        assert_eq!(
            fs::read_to_string(run_dir.join("diff.patch")).unwrap(),
            "diff --git a/file b/file\n"
        );

        let events = RunStore::events(&temp, record.id).unwrap().unwrap();
        let event_types: Vec<_> = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect();
        assert_eq!(event_types, vec!["run_created", "run_succeeded"]);

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn write_failure_without_artifacts_still_writes_auditable_summary_and_diff() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-failure-artifacts-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Executing,
                status: RunStatus::Running,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();

        let updated = RunStore::write_failure(&temp, record.id, None, None, None)
            .unwrap()
            .expect("existing run should be updated");

        assert_eq!(updated.phase, RunPhase::Complete);
        assert_eq!(updated.status, RunStatus::Failed);
        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        let summary = fs::read_to_string(run_dir.join("summary.md")).unwrap();
        assert!(summary.contains("# Run Summary"));
        assert!(summary.contains("Result: failed"));
        assert_eq!(fs::read_to_string(run_dir.join("diff.patch")).unwrap(), "");

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn cancel_writes_auditable_summary_and_diff() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-run-cancel-artifacts-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();

        let record = RunStore::create(
            &temp,
            RunRecord {
                phase: RunPhase::Executing,
                status: RunStatus::Running,
                plan_path: Some("plans/task.md".into()),
                workspace_path: None,
            },
        )
        .unwrap();

        let updated = RunStore::cancel(&temp, record.id)
            .unwrap()
            .expect("existing run should be cancelled");

        assert_eq!(updated.phase, RunPhase::Complete);
        assert_eq!(updated.status, RunStatus::Failed);
        let run_dir = temp
            .join(".ralphterm")
            .join("runs")
            .join(record.id.to_string());
        let summary = fs::read_to_string(run_dir.join("summary.md")).unwrap();
        assert!(summary.contains("# Run Summary"));
        assert!(summary.contains("Result: failed"));
        assert!(summary.contains("cancelled"));
        assert!(summary.contains("Plan: plans/task.md"));
        assert_eq!(fs::read_to_string(run_dir.join("diff.patch")).unwrap(), "");

        remove_dir_all_if_exists(&temp);
    }

    #[test]
    fn write_result_for_missing_run_returns_none_and_does_not_create_directory() {
        let temp = std::env::temp_dir().join(format!(
            "ralphterm-missing-result-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp).unwrap();
        let missing_id = uuid::Uuid::new_v4();

        let result = RunStore::write_result(
            &temp,
            missing_id,
            RunResultArtifacts {
                summary_markdown: "# Summary\n".into(),
                summary_json: None,
                diff_patch: "diff --git a/missing b/missing\n".into(),
            },
        )
        .unwrap();

        assert!(result.is_none());
        assert!(!temp
            .join(".ralphterm")
            .join("runs")
            .join(missing_id.to_string())
            .exists());

        remove_dir_all_if_exists(&temp);
    }

    fn remove_dir_all_if_exists(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
    }
}
