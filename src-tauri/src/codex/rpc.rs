use super::metrics::AccountLimits;
use crate::collector::{
    builtin,
    process::{Framing, Session},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
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
            match self.rpc.session.messages.try_recv() {
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
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ScriptAccount {
        login: String,
        plan: Option<String>,
        #[serde(default)]
        limits: AccountLimits,
    }

    let script: ScriptAccount = builtin::codex_export("readAccount", json!({}), move |call| {
        let method = call.params["method"].as_str().unwrap_or_default();
        if call.operation != "rpc"
            || call.alias != "appServer"
            || !matches!(method, "account/read" | "account/rateLimits/read")
        {
            return Err("Codex 采集脚本调用了未授权的 RPC 方法。".into());
        }
        rpc.session.request(
            method,
            call.params.get("params").cloned(),
            Instant::now() + RPC_TIMEOUT,
        )
    })
    .map_err(|_| Failure::Unavailable("Codex 未能读取账号数据。请检查登录状态和网络后重试。"))?;

    match script.login.as_str() {
        "chatgpt" => Ok(Account {
            plan: script.plan,
            limits: script.limits,
            codex_home,
        }),
        "missing" => Err(Failure::NeedsLogin),
        _ => Err(Failure::UnsupportedLogin),
    }
}

pub(super) fn begin_login(cli: &Path, home: &Path) -> Result<Login, Failure> {
    let (mut rpc, _) = Rpc::initialize(cli, home)?;
    let response = rpc.request("account/login/start", Some(json!({"type":"chatgpt"})))?;
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
    session: Session,
}

impl Rpc {
    fn initialize(cli: &Path, home: &Path) -> Result<(Self, Value), Failure> {
        let mut rpc = Self::start(cli, home)?;
        let response = rpc.request(
            "initialize",
            Some(json!({
                "clientInfo":{"name":"tdisplay-studio","version":env!("CARGO_PKG_VERSION")}
            })),
        )?;
        rpc.session
            .notify("initialized", None, Instant::now() + RPC_TIMEOUT)
            .map_err(|_| {
                Failure::Unavailable("Codex CLI 提前退出，请确认 CLI 支持 App Server。")
            })?;
        Ok((rpc, response))
    }

    fn start(cli: &Path, home: &Path) -> Result<Self, Failure> {
        let mut command = Command::new(cli);
        command
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(home);
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
        let session = Session::start(&mut command, Framing::Versionless)
            .map_err(|_| Failure::Unavailable("无法启动 Codex CLI，请检查路径和执行权限。"))?;
        Ok(Self { session })
    }

    fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, Failure> {
        self.session
            .request(method, params, Instant::now() + RPC_TIMEOUT)
            .map_err(|_| {
                Failure::Unavailable("Codex 未能读取账号数据。请检查登录状态和网络后重试。")
            })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn codex_script_reads_account_and_limits_over_the_stdio_transport() {
        let root = std::env::temp_dir().join(format!("tdisplay-rpc-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let cli = root.join("codex-test");
        fs::write(
            &cli,
            r#"#!/bin/sh
printf '%s\n' 'test wrapper banner'
IFS= read -r init
case "$init" in *'"method":"initialize"'*) ;; *) exit 1 ;; esac
printf '%s\n' '{"id":1,"result":{}}'
IFS= read -r ready
case "$ready" in *'"method":"initialized"'*) ;; *) exit 2 ;; esac
IFS= read -r account
case "$account" in *'"method":"account/read","params":{"refreshToken":false}'*) ;; *) exit 3 ;; esac
printf '%s\n' '{"id":2,"result":{"account":{"type":"chatgpt","planType":"plus"}}}'
IFS= read -r limits
case "$limits" in *'"method":"account/rateLimits/read"'*) ;; *) exit 4 ;; esac
printf '%s\n' '{"id":3,"result":{"rateLimits":{"primary":{"usedPercent":27}}}}'
"#,
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();

        let account = read_account(&cli, &root);
        fs::remove_dir_all(&root).unwrap();
        let account = account
            .ok()
            .expect("script and stdio account query succeeded");
        assert_eq!(account.plan.as_deref(), Some("plus"));
        assert_eq!(account.limits.windows[0].remaining_percent, 73);
    }
}
