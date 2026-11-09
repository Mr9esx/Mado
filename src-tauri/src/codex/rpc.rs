use super::metrics::{parse_limits, AccountLimits};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const RPC_TIMEOUT: Duration = Duration::from_secs(20);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

pub(super) enum Failure {
    MissingCli,
    NeedsLogin,
    UnsupportedLogin,
    Unavailable(&'static str),
}

pub(super) struct Account {
    pub plan: Option<String>,
    pub limits: AccountLimits,
    pub codex_home: Option<PathBuf>,
}

pub(super) struct Login {
    rpc: Rpc,
    id: String,
    deadline: Instant,
}

impl Login {
    pub fn poll(&mut self) -> Option<bool> {
        loop {
            match self.rpc.messages.try_recv() {
                Ok(message) => {
                    if message.get("method").and_then(Value::as_str)
                        == Some("account/login/completed")
                        && message.pointer("/params/loginId").and_then(Value::as_str)
                            == Some(self.id.as_str())
                    {
                        return Some(
                            message.pointer("/params/success").and_then(Value::as_bool)
                                == Some(true),
                        );
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    return (Instant::now() >= self.deadline).then_some(false)
                }
                Err(mpsc::TryRecvError::Disconnected) => return Some(false),
            }
        }
    }
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !path.is_absolute() || !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(super) fn find_cli(explicit: Option<&Path>, home: &Path) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return is_executable(path).then(|| path.to_path_buf());
    }
    let filename = if cfg!(windows) { "codex.exe" } else { "codex" };
    let search = std::env::var_os("PATH").unwrap_or_default();
    let defaults = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join("bin"),
    ];
    std::env::split_paths(&search)
        .chain(defaults)
        .map(|path| path.join(filename))
        .find(|path| is_executable(path))
}

pub(super) fn read_account(cli: &Path, home: &Path) -> Result<Account, Failure> {
    let (mut rpc, initialization) = Rpc::initialize(cli, home)?;
    let codex_home = initialization
        .get("codexHome")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let account = rpc.request(2, "account/read", Some(json!({ "refreshToken": false })))?;
    let account = account.get("account").filter(|value| !value.is_null());
    match account
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
    {
        Some("chatgpt") => {}
        Some(_) => return Err(Failure::UnsupportedLogin),
        None => return Err(Failure::NeedsLogin),
    }
    let plan = account
        .and_then(|value| value.get("planType"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let response = rpc.request(3, "account/rateLimits/read", None)?;
    Ok(Account {
        plan,
        limits: parse_limits(&response),
        codex_home,
    })
}

pub(super) fn begin_login(cli: &Path, home: &Path) -> Result<Login, Failure> {
    let (mut rpc, _) = Rpc::initialize(cli, home)?;
    let response = rpc.request(2, "account/login/start", Some(json!({"type":"chatgpt"})))?;
    let id = response
        .get("loginId")
        .and_then(Value::as_str)
        .ok_or(Failure::Unavailable("Codex 未返回登录会话。"))?;
    let url = response
        .get("authUrl")
        .and_then(Value::as_str)
        .and_then(|url| tauri::Url::parse(url).ok())
        .filter(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port_or_known_default() == Some(443)
                && matches!(url.host_str(), Some("chatgpt.com" | "auth.openai.com"))
        })
        .ok_or(Failure::Unavailable(
            "无法验证官方登录地址，请使用终端的 Codex 登录命令。",
        ))?;
    tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
        .map_err(|_| Failure::Unavailable("无法打开浏览器。可在终端运行 codex login。"))?;
    Ok(Login {
        rpc,
        id: id.to_owned(),
        deadline: Instant::now() + LOGIN_TIMEOUT,
    })
}

struct Rpc {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
}

impl Rpc {
    fn initialize(cli: &Path, home: &Path) -> Result<(Self, Value), Failure> {
        let mut rpc = Self::start(cli, home)?;
        let response = rpc.request(
            1,
            "initialize",
            Some(json!({
                "clientInfo":{"name":"tdisplay-studio","version":env!("CARGO_PKG_VERSION")}
            })),
        )?;
        rpc.write(&json!({ "method": "initialized" }))?;
        Ok((rpc, response))
    }

    fn start(cli: &Path, home: &Path) -> Result<Self, Failure> {
        let mut command = Command::new(cli);
        command
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let paths = std::env::split_paths(&inherited)
            .filter(|path| path.is_absolute())
            .chain(cli.parent().map(Path::to_path_buf))
            .chain([
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ]);
        if let Ok(path) = std::env::join_paths(paths) {
            command.env("PATH", path);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        let mut child = command
            .spawn()
            .map_err(|_| Failure::Unavailable("无法启动 Codex CLI，请检查路径和执行权限。"))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (sender, messages) = mpsc::sync_channel(16);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(message) = serde_json::from_str(&line) {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            messages,
        })
    }

    fn write(&mut self, message: &Value) -> Result<(), Failure> {
        writeln!(self.stdin, "{message}")
            .and_then(|()| self.stdin.flush())
            .map_err(|_| Failure::Unavailable("Codex CLI 提前退出，请确认 CLI 支持 App Server。"))
    }

    fn request(&mut self, id: u64, method: &str, params: Option<Value>) -> Result<Value, Failure> {
        let mut request = json!({ "id": id, "method": method });
        if let Some(params) = params {
            request["params"] = params;
        }
        self.write(&request)?;
        let deadline = Instant::now() + RPC_TIMEOUT;
        loop {
            let message = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => {
                        Failure::Unavailable("Codex 响应超时，请稍后重试。")
                    }
                    mpsc::RecvTimeoutError::Disconnected => {
                        Failure::Unavailable("Codex CLI 已退出，请检查版本。")
                    }
                })?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            return message.get("result").cloned().ok_or(Failure::Unavailable(
                "Codex 未能读取账号数据。请检查登录状态和网络后重试。",
            ));
        }
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
