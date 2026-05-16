use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread,
};

use anyhow::{bail, Context};
use dashmap::DashMap;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::{
    pty_agent::{default_args, default_command, AgentKind, SessionConfig, SessionInput},
    signals::{detect_signal, AgentSignal},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: Uuid,
    pub agent: AgentKind,
    pub status: SessionStatus,
    pub signal: Option<AgentSignal>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Running,
    Exited,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionEvent {
    Output { text: String },
    Signal { signal: AgentSignal },
    Exit { exit_code: Option<i32> },
    Error { message: String },
}

struct SessionHandle {
    record: Mutex<SessionRecord>,
    transcript: Mutex<String>,
    input_tx: mpsc::UnboundedSender<SessionInput>,
    resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    event_tx: broadcast::Sender<SessionEvent>,
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
}

#[derive(Default)]
pub struct SessionStore {
    sessions: DashMap<Uuid, Arc<SessionHandle>>,
}

impl SessionStore {
    pub async fn spawn(&self, cfg: SessionConfig) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        let (event_tx, _) = broadcast::channel(1024);
        let (input_tx, input_rx) = mpsc::unbounded_channel::<SessionInput>();
        let (resize_tx, resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

        let handle = Arc::new(SessionHandle {
            record: Mutex::new(SessionRecord {
                id,
                agent: cfg.agent,
                status: SessionStatus::Running,
                signal: None,
                exit_code: None,
            }),
            transcript: Mutex::new(String::new()),
            input_tx,
            resize_tx,
            event_tx,
            child: Mutex::new(None),
        });
        self.sessions.insert(id, handle.clone());

        thread::Builder::new()
            .name(format!("pty-session-{id}"))
            .spawn(move || {
                if let Err(err) = run_session_thread(cfg, handle.clone(), input_rx, resize_rx) {
                    let _ = handle.event_tx.send(SessionEvent::Error {
                        message: err.to_string(),
                    });
                    if let Ok(mut rec) = handle.record.lock() {
                        rec.status = SessionStatus::Failed;
                    }
                }
            })
            .context("spawn session thread")?;

        Ok(id)
    }

    pub fn get(&self, id: Uuid) -> Option<SessionRecord> {
        self.sessions
            .get(&id)?
            .record
            .lock()
            .ok()
            .map(|r| r.clone())
    }

    pub fn list(&self) -> Vec<SessionRecord> {
        let mut records: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|entry| entry.record.lock().ok().map(|record| record.clone()))
            .collect();
        records.sort_by_key(|record| record.id);
        records
    }

    pub async fn send(&self, id: Uuid, input: SessionInput) -> anyhow::Result<()> {
        self.sessions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("session not found"))?
            .input_tx
            .send(input)
            .context("send input")
    }

    pub async fn resize(&self, id: Uuid, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.sessions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("session not found"))?
            .resize_tx
            .send((cols, rows))
            .context("send resize")
    }

    pub async fn cancel(&self, id: Uuid) -> anyhow::Result<()> {
        let session = self
            .sessions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("session not found"))?;
        if let Some(child) = session.child.lock().expect("child lock").as_mut() {
            child.kill().context("kill child")?;
        }
        if let Ok(mut rec) = session.record.lock() {
            rec.status = SessionStatus::Cancelled;
        }
        Ok(())
    }

    pub fn transcript(&self, id: Uuid) -> Option<String> {
        self.sessions
            .get(&id)?
            .transcript
            .lock()
            .ok()
            .map(|t| t.clone())
    }

    pub fn subscribe(&self, id: Uuid) -> Option<broadcast::Receiver<SessionEvent>> {
        Some(self.sessions.get(&id)?.event_tx.subscribe())
    }
}

fn run_session_thread(
    cfg: SessionConfig,
    handle: Arc<SessionHandle>,
    mut input_rx: mpsc::UnboundedReceiver<SessionInput>,
    mut resize_rx: mpsc::UnboundedReceiver<(u16, u16)>,
) -> anyhow::Result<()> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: cfg.rows,
            cols: cfg.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open pty")?;

    let command = cfg
        .command
        .clone()
        .unwrap_or_else(|| default_command(cfg.agent).to_string());
    let mut args = if cfg.args.is_empty() {
        default_args(cfg.agent)
    } else {
        cfg.args.clone()
    };
    let mut cmd = CommandBuilder::new(command);
    for arg in args.drain(..) {
        cmd.arg(arg);
    }
    if let Some(cwd) = &cfg.cwd {
        cmd.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn agent command")?;
    drop(pair.slave);
    *handle.child.lock().expect("child lock") = Some(child);

    let mut reader = pair.master.try_clone_reader().context("clone pty reader")?;
    let writer = Arc::new(Mutex::new(
        pair.master.take_writer().context("take pty writer")?,
    ));

    {
        let writer = writer.clone();
        let prompt = cfg.prompt.clone();
        thread::spawn(move || {
            // Paste after the terminal app has had a moment to draw its prompt.
            thread::sleep(std::time::Duration::from_millis(800));
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(prompt.as_bytes());
                let _ = w.write_all(b"\n");
                let _ = w.flush();
            }
        });
    }

    {
        let writer = writer.clone();
        thread::spawn(move || {
            while let Some(input) = input_rx.blocking_recv() {
                if let Ok(mut w) = writer.lock() {
                    let _ = w.write_all(input.text.as_bytes());
                    if input.enter {
                        let _ = w.write_all(b"\n");
                    }
                    let _ = w.flush();
                }
            }
        });
    }

    // Resize requests are accepted by the API now; wiring them to the PTY master is
    // part of the next milestone because portable-pty exposes resize on the master
    // handle that is also used for reader/writer ownership.
    thread::spawn(move || while resize_rx.blocking_recv().is_some() {});

    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                append_output(&handle, &text);
            }
            Err(err) => bail!("read pty: {err}"),
        }
    }

    let exit_code = handle
        .child
        .lock()
        .expect("child lock")
        .as_mut()
        .and_then(|child| child.wait().ok())
        .map(|status| status.exit_code() as i32);

    if let Ok(mut rec) = handle.record.lock() {
        rec.status = SessionStatus::Exited;
        rec.exit_code = exit_code;
    }
    let _ = handle.event_tx.send(SessionEvent::Exit { exit_code });
    Ok(())
}

fn append_output(handle: &Arc<SessionHandle>, text: &str) {
    if let Ok(mut transcript) = handle.transcript.lock() {
        transcript.push_str(text);
        if let Some(signal) = detect_signal(&transcript) {
            if let Ok(mut rec) = handle.record.lock() {
                if rec.signal.is_none() {
                    rec.signal = Some(signal.clone());
                    let _ = handle.event_tx.send(SessionEvent::Signal { signal });
                }
            }
        }
    }
    let _ = handle.event_tx.send(SessionEvent::Output {
        text: text.to_string(),
    });
}
