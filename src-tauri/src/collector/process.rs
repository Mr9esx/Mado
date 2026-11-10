use super::runner::read_line;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex, Weak,
    },
    thread,
    time::{Duration, Instant},
};

const LIMIT: usize = 128 * 1024;

struct Processes {
    closing: bool,
    active: BTreeMap<u32, Weak<Mutex<Child>>>,
}

static PROCESSES: Mutex<Processes> = Mutex::new(Processes {
    closing: false,
    active: BTreeMap::new(),
});

pub(super) struct ManagedChild {
    id: u32,
    child: Arc<Mutex<Child>>,
}

impl ManagedChild {
    pub(super) fn start(command: &mut Command) -> io::Result<Self> {
        if PROCESSES.lock().expect("process registry").closing {
            return Err(io::Error::other("Studio 正在退出。"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command.spawn()?;
        let id = child.id();
        let child = Self {
            id,
            child: Arc::new(Mutex::new(child)),
        };
        let mut processes = PROCESSES.lock().expect("process registry");
        if processes.closing {
            return Err(io::Error::other("Studio 正在退出。"));
        }
        processes.active.insert(id, Arc::downgrade(&child.child));
        Ok(child)
    }

    pub(super) fn stdin(&self) -> ChildStdin {
        self.child
            .lock()
            .expect("child")
            .stdin
            .take()
            .expect("piped stdin")
    }

    pub(super) fn stdout(&self) -> ChildStdout {
        self.child
            .lock()
            .expect("child")
            .stdout
            .take()
            .expect("piped stdout")
    }

    fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        self.child.lock().expect("child").try_wait()
    }
}

fn terminate(id: u32, child: &Mutex<Child>) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(id as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = id;
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        terminate(self.id, &self.child);
        PROCESSES
            .lock()
            .expect("process registry")
            .active
            .remove(&self.id);
        let _ = self.child.lock().expect("child").wait();
    }
}

pub(crate) fn terminate_active() {
    let mut processes = PROCESSES.lock().expect("process registry");
    processes.closing = true;
    for (&id, child) in &processes.active {
        if let Some(child) = child.upgrade() {
            terminate(id, &child);
        }
    }
}

pub(super) fn run(command: &mut Command, deadline: Instant) -> Result<Value, String> {
    let child = ManagedChild::start(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .map_err(|_| "无法启动授权的程序。")?;
    fn capture(pipe: impl Read + Send + 'static) -> Receiver<Result<String, String>> {
        let (send, receive) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = send.send(super::io::read_text(pipe, LIMIT));
        });
        receive
    }
    let stdout = capture(child.stdout());
    let stderr = capture(
        child
            .child
            .lock()
            .expect("child")
            .stderr
            .take()
            .expect("piped stderr"),
    );
    let read = |pipe: Receiver<_>| {
        pipe.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "授权程序运行超时。".to_string())
    };
    let stdout = read(stdout)??;
    let stderr = read(stderr)??;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| "无法读取程序退出状态。")? {
            return Ok(json!({ "code": status.code(), "stdout": stdout, "stderr": stderr }));
        }
        if Instant::now() >= deadline {
            return Err("授权程序运行超时。".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Framing {
    JsonRpc,
    Versionless,
}

struct Outbound {
    message: Value,
    written: SyncSender<std::io::Result<()>>,
}

pub(crate) struct Session {
    child: ManagedChild,
    outgoing: Option<SyncSender<Outbound>>,
    framing: Framing,
    pub(crate) messages: Receiver<Value>,
    next_id: u64,
}

impl Session {
    pub(crate) fn start(command: &mut Command, framing: Framing) -> Result<Self, String> {
        let child = ManagedChild::start(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .map_err(|_| "无法启动授权的程序。")?;
        let mut stdin = child.stdin();
        let mut stdout = BufReader::new(child.stdout());
        let (outgoing, receive) = mpsc::sync_channel::<Outbound>(1);
        let (send, messages) = mpsc::sync_channel(16);
        thread::spawn(move || {
            for Outbound { message, written } in receive {
                let result = writeln!(stdin, "{message}").and_then(|()| stdin.flush());
                let failed = result.is_err();
                let _ = written.send(result);
                if failed {
                    break;
                }
            }
        });
        thread::spawn(move || {
            while let Ok(line) = read_line(&mut stdout, LIMIT) {
                let message = match serde_json::from_slice(&line) {
                    Ok(message) => message,
                    Err(_) if matches!(framing, Framing::Versionless) => continue,
                    Err(_) => break,
                };
                if send.send(message).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            outgoing: Some(outgoing),
            framing,
            messages,
            next_id: 0,
        })
    }

    pub(crate) fn notify(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Instant,
    ) -> Result<(), String> {
        self.send(json!({ "method": method }), params, deadline)
    }

    fn send(
        &self,
        mut message: Value,
        params: Option<Value>,
        deadline: Instant,
    ) -> Result<(), String> {
        if matches!(self.framing, Framing::JsonRpc) {
            message["jsonrpc"] = json!("2.0");
        }
        if let Some(params) = params {
            message["params"] = params;
        }
        let (written, receive) = mpsc::sync_channel(1);
        self.outgoing
            .as_ref()
            .expect("open session")
            .try_send(Outbound { message, written })
            .map_err(|_| "授权程序无响应或已退出。")?;
        receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "等待授权程序读取请求超时。")?
            .map_err(|_| "无法写入授权程序。".into())
    }

    pub(crate) fn request(
        &mut self,
        method: &str,
        params: Option<Value>,
        deadline: Instant,
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "id": id, "method": method }), params, deadline)?;
        loop {
            let response = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| "授权程序响应超时或已退出。")?;
            if response["id"].as_u64() == Some(id) {
                return response
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "授权程序返回 RPC 错误。".into());
            }
            if Instant::now() >= deadline {
                return Err("授权程序响应超时。".into());
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Close stdin first so the program has a chance to handle its last notification.
        self.outgoing.take();
        let deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < deadline && matches!(self.child.try_wait(), Ok(None)) {
            thread::sleep(Duration::from_millis(5));
        }
    }
}
