//! Procesos de llama.cpp que el nodo arranca y supervisa.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, bail};
use serde::Serialize;

const MAX_LOG_LINES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// `ggml-rpc-server`: presta la GPU de este nodo a otro.
    Rpc,
    /// `llama-server`: sirve un modelo con API compatible con OpenAI y chat web.
    Llm,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Rpc => "ggml-rpc-server",
            Kind::Llm => "llama-server",
        }
    }
}

type Log = Arc<Mutex<VecDeque<String>>>;

struct Running {
    child: Child,
    started: Instant,
}

struct Slot {
    running: Option<Running>,
    command_line: String,
    last_exit: Option<String>,
    log: Log,
}

impl Slot {
    fn new() -> Self {
        Self {
            running: None,
            command_line: String::new(),
            last_exit: None,
            log: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Recoge el proceso si ya terminó por su cuenta.
    fn reap(&mut self) {
        if let Some(run) = &mut self.running {
            if let Ok(Some(status)) = run.child.try_wait() {
                self.last_exit = Some(status.to_string());
                self.running = None;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub kind: Kind,
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub command_line: String,
    pub last_exit: Option<String>,
    pub log: Vec<String>,
}

pub struct Procs {
    rpc: Mutex<Slot>,
    llm: Mutex<Slot>,
}

impl Default for Procs {
    fn default() -> Self {
        Self {
            rpc: Mutex::new(Slot::new()),
            llm: Mutex::new(Slot::new()),
        }
    }
}

fn push_line(log: &Log, line: String) {
    if let Ok(mut log) = log.lock() {
        if log.len() >= MAX_LOG_LINES {
            log.pop_front();
        }
        log.push_back(line);
    }
}

fn pump(reader: impl Read + Send + 'static, log: Log) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => push_line(&log, line),
                Err(_) => break,
            }
        }
    });
}

impl Procs {
    fn slot(&self, kind: Kind) -> &Mutex<Slot> {
        match kind {
            Kind::Rpc => &self.rpc,
            Kind::Llm => &self.llm,
        }
    }

    /// `envs`: variables de entorno extra. Sirven para pasar secretos sin que
    /// aparezcan en la línea de comandos del proceso.
    pub fn start(
        &self,
        kind: Kind,
        exe: &Path,
        args: &[String],
        envs: &[(&str, &str)],
    ) -> anyhow::Result<()> {
        if !exe.is_file() {
            bail!("no se encontró {}", exe.display());
        }
        let mut slot = self
            .slot(kind)
            .lock()
            .map_err(|_| anyhow::anyhow!("estado interno bloqueado"))?;
        slot.reap();
        if slot.running.is_some() {
            bail!("{} ya está en marcha", kind.label());
        }

        let mut command = Command::new(exe);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(envs.iter().copied());
        if let Some(dir) = exe.parent() {
            command.current_dir(dir);
        }
        uac_probe::no_console_window(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("no se pudo arrancar {}", exe.display()))?;

        if let Ok(mut log) = slot.log.lock() {
            log.clear();
        }
        if let Some(out) = child.stdout.take() {
            pump(out, slot.log.clone());
        }
        if let Some(err) = child.stderr.take() {
            pump(err, slot.log.clone());
        }
        slot.command_line = std::iter::once(kind.label().to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        slot.last_exit = None;
        slot.running = Some(Running {
            child,
            started: Instant::now(),
        });
        Ok(())
    }

    pub fn stop(&self, kind: Kind) -> anyhow::Result<()> {
        let mut slot = self
            .slot(kind)
            .lock()
            .map_err(|_| anyhow::anyhow!("estado interno bloqueado"))?;
        if let Some(mut run) = slot.running.take() {
            run.child.kill().ok();
            let status = run.child.wait().ok();
            slot.last_exit = Some(
                status
                    .map(|s| format!("detenido ({s})"))
                    .unwrap_or_else(|| "detenido".to_string()),
            );
        }
        Ok(())
    }

    pub fn is_running(&self, kind: Kind) -> bool {
        let Ok(mut slot) = self.slot(kind).lock() else {
            return false;
        };
        slot.reap();
        slot.running.is_some()
    }

    pub fn stop_all(&self) {
        self.stop(Kind::Rpc).ok();
        self.stop(Kind::Llm).ok();
    }

    pub fn status(&self, kind: Kind) -> Status {
        let Ok(mut slot) = self.slot(kind).lock() else {
            return Status {
                kind,
                running: false,
                pid: None,
                uptime_secs: None,
                command_line: String::new(),
                last_exit: Some("estado interno bloqueado".to_string()),
                log: Vec::new(),
            };
        };
        slot.reap();
        let log = slot
            .log
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();
        Status {
            kind,
            running: slot.running.is_some(),
            pid: slot.running.as_ref().map(|r| r.child.id()),
            uptime_secs: slot.running.as_ref().map(|r| r.started.elapsed().as_secs()),
            command_line: slot.command_line.clone(),
            last_exit: slot.last_exit.clone(),
            log,
        }
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        self.stop_all();
    }
}
