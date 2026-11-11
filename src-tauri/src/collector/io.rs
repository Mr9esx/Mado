use super::{
    model::{path_valid, Definition, Permission},
    process::{self, Session},
    runner::Call,
};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const BYTE_LIMIT: usize = 128 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(4);

pub fn read_text(reader: impl Read, limit: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取采集数据。")?;
    if bytes.len() > limit {
        return Err("采集数据超过大小限制。".into());
    }
    String::from_utf8(bytes).map_err(|_| "采集数据须为 UTF-8 文本。".into())
}

fn url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "请输入有效的 HTTP 或 HTTPS 地址。")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("请输入不含用户名、密码或片段的 HTTP 或 HTTPS 地址。".into());
    }
    Ok(url)
}

// A stalled mount can outlive the sampling deadline. Bound its workers separately
// so it cannot retain the broker (and any stdio sessions) or grow on each retry.
fn filesystem<T: Send + 'static>(
    deadline: Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
    }
    ACTIVE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
            (active < 4).then_some(active + 1)
        })
        .map_err(|_| "文件操作繁忙，请检查授权路径。")?;
    let slot = Slot;
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = work();
        drop(slot);
        let _ = send.send(result);
    });
    receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "授权路径访问超时。".to_string())?
}

pub(super) fn is_path_resource(kind: &str) -> bool {
    matches!(
        kind,
        "file" | "fileJson" | "directory" | "process" | "stdio"
    )
}

pub(super) fn check_paths<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    filesystem(Instant::now() + CALL_TIMEOUT, work)
}

pub(super) fn normalize_target(kind: &str, label: &str, value: &str) -> Result<String, String> {
    if matches!(kind, "http" | "httpJson") {
        return Ok(url(value)?.to_string());
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(format!("{}需要绝对路径。", label));
    }
    let path = path
        .canonicalize()
        .map_err(|_| format!("找不到{}。", label))?;
    if (kind == "directory" && !path.is_dir()) || (kind != "directory" && !path.is_file()) {
        return Err(format!("{}的路径类型不符。", label));
    }
    #[cfg(unix)]
    if matches!(kind, "process" | "stdio") {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .map_or(true, |metadata| metadata.permissions().mode() & 0o111 == 0)
        {
            return Err(format!("{}须为可执行文件。", label));
        }
    }
    #[cfg(windows)]
    if matches!(kind, "process" | "stdio")
        && !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err(format!("{}须为 .exe 可执行文件。", label));
    }
    path.into_os_string()
        .into_string()
        .map_err(|_| "路径须为 UTF-8。".into())
}

#[cfg(test)]
fn bind(definition: &Definition, values: &mut BTreeMap<String, String>) -> Result<(), String> {
    for (alias, permission) in &definition.permissions {
        if matches!(permission, Permission::System { .. }) {
            continue;
        }
        let value = values.get_mut(alias).ok_or("未授权")?;
        let scope = json!(permission);
        *value = normalize_target(scope["kind"].as_str().unwrap(), permission.label(), value)?;
    }
    Ok(())
}

pub fn settings(
    definition: &Definition,
    values: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let mut settings = Map::new();
    for (key, setting) in &definition.settings {
        let value = setting.value(values.get(key));
        if value.len() > 4096 {
            return Err(format!("{}的值过长。", setting.label));
        }
        settings.insert(key.clone(), json!(value));
    }
    Ok(Value::Object(settings))
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Http {
    path: String,
    method: Option<String>,
    headers: BTreeMap<String, String>,
    body: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePath {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rpc {
    method: String,
    params: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FindFiles {
    path: String,
    prefix: String,
    suffix: String,
    depth: usize,
    modified_since: u64,
    limit: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadWindow {
    path: String,
    #[serde(default)]
    head_bytes: u64,
    #[serde(default)]
    tail_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receive {
    method: String,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenUrl {
    url: String,
}

enum Operation {
    Http(Http),
    List(String),
    ReadText(String),
    FindFiles(FindFiles),
    ReadWindow(ReadWindow),
    Run(Vec<String>),
    Rpc(Rpc, bool),
    Receive(Receive),
    Browser(reqwest::Url),
    System,
}

impl Operation {
    fn check(call: &Call, permission: &Permission) -> Result<Self, String> {
        fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T, String> {
            serde_json::from_value(params.clone()).map_err(|_| "宿主调用参数无效。".into())
        }
        let denied = || format!("此调用超出「{}」的授权。", permission.label());
        match (call.operation.as_str(), permission) {
            ("http", Permission::Http { methods, .. }) => {
                let options: Http = parse(&call.params)?;
                let method = options.method.as_deref().unwrap_or("GET");
                let path = &options.path;
                if !methods.iter().any(|allowed| allowed == method)
                    || !(path.is_empty() || path.starts_with('/') && !path.starts_with("//"))
                    || path.contains(['\\', '#'])
                    || path.chars().any(char::is_control)
                    || options.headers.iter().any(|(header, value)| {
                        reqwest::header::HeaderName::from_bytes(header.as_bytes()).is_err()
                            || reqwest::header::HeaderValue::from_str(value).is_err()
                            || matches!(
                                header.to_ascii_lowercase().as_str(),
                                "host"
                                    | "connection"
                                    | "content-length"
                                    | "transfer-encoding"
                                    | "upgrade"
                                    | "proxy-authorization"
                            )
                    })
                {
                    return Err(denied());
                }
                Ok(Self::Http(options))
            }
            ("list" | "readText", Permission::Directory { .. })
            | ("readText", Permission::File { .. }) => {
                let path = parse::<FilePath>(&call.params)?.path;
                if !path.is_empty()
                    && (!path_valid(&path) || matches!(permission, Permission::File { .. }))
                {
                    return Err(denied());
                }
                if call.operation == "list" {
                    Ok(Self::List(path))
                } else {
                    Ok(Self::ReadText(path))
                }
            }
            ("run", Permission::Process { args, .. }) => {
                let requested = parse::<Args>(&call.params)?.args;
                if !args.contains(&requested) {
                    return Err(denied());
                }
                Ok(Self::Run(requested))
            }
            ("findFiles", Permission::Directory { .. }) => {
                let options: FindFiles = parse(&call.params)?;
                if !path_valid(&options.path)
                    || !(1..=6).contains(&options.depth)
                    || !(1..=32).contains(&options.limit)
                    || options.prefix.len() > 64
                    || options.suffix.len() > 64
                {
                    return Err(denied());
                }
                Ok(Self::FindFiles(options))
            }
            ("readWindow", Permission::Directory { .. }) => {
                let options: ReadWindow = parse(&call.params)?;
                if !path_valid(&options.path)
                    || options.head_bytes > 256 * 1024
                    || options.tail_bytes > 2 * 1024 * 1024
                {
                    return Err(denied());
                }
                Ok(Self::ReadWindow(options))
            }
            ("rpc" | "notify", Permission::Stdio { methods, .. }) => {
                let rpc: Rpc = parse(&call.params)?;
                if !methods.contains(&rpc.method)
                    || rpc
                        .params
                        .as_ref()
                        .is_some_and(|params| !params.is_object() && !params.is_array())
                {
                    return Err(denied());
                }
                Ok(Self::Rpc(rpc, call.operation == "notify"))
            }
            ("system", Permission::System { .. }) => Ok(Self::System),
            ("receive", Permission::Stdio { methods, .. }) => {
                let receive: Receive = parse(&call.params)?;
                if !methods.contains(&receive.method) || receive.timeout_ms > 1000 {
                    return Err(denied());
                }
                Ok(Self::Receive(receive))
            }
            ("openUrl", Permission::Browser { hosts, .. }) => {
                let url = url(&parse::<OpenUrl>(&call.params)?.url)?;
                if url.scheme() != "https"
                    || url.port_or_known_default() != Some(443)
                    || !url
                        .host_str()
                        .is_some_and(|host| hosts.iter().any(|value| value == host))
                {
                    return Err(denied());
                }
                Ok(Self::Browser(url))
            }
            _ => Err(denied()),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mock {
    #[serde(flatten)]
    call: Call,
    result: Value,
}

pub struct Broker {
    permissions: BTreeMap<String, Permission>,
    mode: Mode,
    deadline: Instant,
    cancelled: Option<Arc<AtomicBool>>,
}

enum Mode {
    Mock(Vec<Mock>),
    Live {
        bindings: BTreeMap<String, String>,
        client: reqwest::blocking::Client,
        sessions: BTreeMap<String, Session>,
    },
}

impl Broker {
    pub fn fixture(definition: &Definition, fixture: &Value) -> Result<(Value, Self), String> {
        let raw = fixture
            .get("settings")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let values = serde_json::from_value(raw).map_err(|_| "样例参数必须是文本值。")?;
        let mocks: Vec<Mock> =
            serde_json::from_value(fixture.get("calls").cloned().unwrap_or_else(|| json!([])))
                .map_err(|error| format!("样例调用：{error}"))?;
        if mocks.len() > 24 {
            return Err("样例调用超过 24 项。".into());
        }
        Ok((
            settings(definition, &values)?,
            Self {
                permissions: definition.permissions.clone(),
                mode: Mode::Mock(mocks),
                deadline: Instant::now() + Duration::from_secs(12),
                cancelled: None,
            },
        ))
    }

    pub fn live(
        definition: &Definition,
        bindings: BTreeMap<String, String>,
        client: reqwest::blocking::Client,
    ) -> Self {
        Self {
            permissions: definition.permissions.clone(),
            deadline: Instant::now() + Duration::from_secs(12),
            cancelled: None,
            mode: Mode::Live {
                bindings,
                client,
                sessions: BTreeMap::new(),
            },
        }
    }

    pub fn interactive(mut self, cancelled: Arc<AtomicBool>) -> Self {
        self.deadline = Instant::now() + Duration::from_secs(185);
        self.cancelled = Some(cancelled);
        self
    }

    pub fn call(&mut self, call: Call) -> Result<Value, String> {
        let now = Instant::now();
        if self
            .cancelled
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Relaxed))
        {
            return Err("操作已取消。".into());
        }
        if now >= self.deadline {
            return Err("采集已超时。".into());
        }
        let permission = self
            .permissions
            .get(&call.alias)
            .ok_or_else(|| format!("未声明资源授权「{}」。", call.alias))?;
        let operation = Operation::check(&call, permission)?;
        let Mode::Live {
            bindings,
            client,
            sessions,
        } = &mut self.mode
        else {
            let Mode::Mock(mocks) = &mut self.mode else {
                unreachable!()
            };
            return mocks
                .iter()
                .position(|mock| {
                    mock.call.operation == call.operation
                        && mock.call.alias == call.alias
                        && mock.call.params == call.params
                })
                .map(|index| mocks.remove(index).result)
                .ok_or_else(|| {
                    format!("样例缺少 {} 对「{}」的响应。", call.operation, call.alias)
                });
        };
        if let Operation::System = operation {
            return Ok(system_readings());
        }
        if let Operation::Browser(url) = operation {
            if self.cancelled.is_none() {
                return Err("打开浏览器需要用户发起插件操作。".into());
            }
            tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
                .map_err(|_| "无法打开默认浏览器。".to_string())?;
            return Ok(Value::Null);
        }
        let value = bindings.get(&call.alias).ok_or("资源尚未授权。")?;
        let deadline = self.deadline.min(now + CALL_TIMEOUT);
        match operation {
            Operation::Http(options) => http(client, value, options, deadline),
            Operation::List(path) => {
                let value = value.clone();
                filesystem(deadline, move || list(checked_path(&value)?, &path))
            }
            Operation::ReadText(path) => {
                let value = value.clone();
                let permission = permission.clone();
                filesystem(deadline, move || {
                    read(checked_path(&value)?, &path, &permission)
                })
            }
            Operation::FindFiles(options) => {
                let value = value.clone();
                filesystem(deadline, move || find_files(checked_path(&value)?, options))
            }
            Operation::ReadWindow(options) => {
                let value = value.clone();
                filesystem(deadline, move || {
                    read_window(checked_path(&value)?, options)
                })
            }
            Operation::Run(args) => {
                let program = executable(value, deadline)?;
                process::run(command(program).args(args), deadline)
            }
            Operation::Rpc(rpc, notify) => {
                if !sessions.contains_key(&call.alias) {
                    let Permission::Stdio {
                        args,
                        versionless,
                        environment,
                        ..
                    } = permission
                    else {
                        unreachable!()
                    };
                    let program = executable(value, deadline)?;
                    let mut command = command(program);
                    command.args(args);
                    for key in ["HOME", "USERPROFILE"] {
                        if let Some(value) = std::env::var_os(key) {
                            command.env(key, value);
                        }
                    }
                    let path = std::env::var_os("PATH").unwrap_or_default();
                    let search = std::env::split_paths(&path).filter(|path| path.is_absolute());
                    #[cfg(target_os = "macos")]
                    let search = search.chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
                    if let Ok(path) = std::env::join_paths(search) { command.env("PATH", path); }
                    for (key, alias) in environment {
                        if let Some(value) = bindings.get(alias) {
                            command.env(key, value);
                        }
                    }
                    let framing = if *versionless {
                        process::Framing::Versionless
                    } else {
                        process::Framing::JsonRpc
                    };
                    let session = Session::start(&mut command, framing)?;
                    sessions.insert(call.alias.clone(), session);
                }
                let session = sessions.get_mut(&call.alias).expect("opened session");
                if notify {
                    session.notify(&rpc.method, rpc.params, deadline)?;
                    Ok(Value::Null)
                } else {
                    session.request(&rpc.method, rpc.params, deadline)
                }
            }
            Operation::Receive(receive) => {
                if self.cancelled.is_none() {
                    return Err("等待通知需要用户发起插件操作。".into());
                }
                let session = sessions.get_mut(&call.alias).ok_or("授权程序尚未启动。")?;
                let deadline = deadline.min(now + Duration::from_millis(receive.timeout_ms));
                Ok(session
                    .receive(&receive.method, deadline)?
                    .unwrap_or(Value::Null))
            }
            Operation::System | Operation::Browser(_) => unreachable!(),
        }
    }
}

fn executable(value: &str, deadline: Instant) -> Result<PathBuf, String> {
    let value = value.to_owned();
    filesystem(deadline, move || checked_path(&value))
}

fn command(path: PathBuf) -> Command {
    let mut command = Command::new(&path);
    command.env_clear();
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    command
}

fn checked_path(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let canonical = path.canonicalize().map_err(|_| "授权路径已失效。")?;
    if canonical != path {
        return Err("授权路径已变化，请重新配置实例。".into());
    }
    Ok(canonical)
}

fn http(
    client: &reqwest::blocking::Client,
    value: &str,
    options: Http,
    deadline: Instant,
) -> Result<Value, String> {
    let base = url(value)?;
    let target = if options.path.is_empty() {
        base.clone()
    } else {
        base.join(&options.path).map_err(|_| "请求路径无效。")?
    };
    if target.origin() != base.origin() {
        return Err("请求超出已授权的网络来源。".into());
    }
    let method = options
        .method
        .as_deref()
        .unwrap_or("GET")
        .parse::<reqwest::Method>()
        .map_err(|_| "请求方法无效。")?;
    let mut request = client
        .request(method, target)
        .timeout(deadline.saturating_duration_since(Instant::now()));
    for (key, value) in options.headers {
        request = request.header(key, value);
    }
    if let Some(body) = options.body {
        request = request.body(body);
    }
    let response = request.send().map_err(|_| "授权 HTTP 请求失败。")?;
    let status = response.status().as_u16();
    let text = read_text(response, BYTE_LIMIT)?;
    Ok(json!({ "status": status, "text": text }))
}

fn dir(path: impl AsRef<Path>) -> Result<Dir, String> {
    Dir::open_ambient_dir(path, ambient_authority()).map_err(|_| "无法打开授权目录。".into())
}

fn list(root: PathBuf, path: &str) -> Result<Value, String> {
    let dir = dir(root)?;
    let entries = if path.is_empty() {
        dir.entries()
    } else {
        dir.read_dir(path)
    }
    .map_err(|_| "无法枚举授权目录。")?;
    let mut result = Vec::new();
    for entry in entries.take(513) {
        if result.len() >= 512 {
            return Err("目录超过 512 项。".into());
        }
        let entry = entry.map_err(|_| "无法读取目录项。")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "目录项名称须为 UTF-8。")?;
        let kind = entry.file_type().map_err(|_| "无法读取目录项类型。")?;
        result.push((
            name,
            if kind.is_dir() {
                "directory"
            } else if kind.is_file() {
                "file"
            } else {
                "other"
            },
        ));
    }
    result.sort_unstable();
    Ok(Value::Array(
        result
            .into_iter()
            .map(|(name, kind)| json!({ "name": name, "kind": kind }))
            .collect(),
    ))
}

fn open_file(root: PathBuf, path: &str, single_file: bool) -> Result<cap_std::fs::File, String> {
    let (dir, path) = if single_file {
        (
            dir(root.parent().ok_or("文件路径无效。")?)?,
            root.file_name().map(Path::new).ok_or("文件路径无效。")?,
        )
    } else {
        (dir(&root)?, Path::new(path))
    };
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | if single_file { libc::O_NOFOLLOW } else { 0 });
    }
    if !dir
        .symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err("请选择授权范围内的普通文件。".into());
    }
    let file = dir
        .open_with(path, &options)
        .map_err(|_| "无法打开授权文件。")?;
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err("请选择授权范围内的普通文件。".into());
    }
    Ok(file)
}
fn read(root: PathBuf, path: &str, permission: &Permission) -> Result<Value, String> {
    let file = open_file(root, path, matches!(permission, Permission::File { .. }))?;
    Ok(json!(read_text(file, BYTE_LIMIT)?))
}

fn read_window(root: PathBuf, options: ReadWindow) -> Result<Value, String> {
    let mut file = open_file(root, &options.path, false)?;
    let size = file.metadata().map_err(|_| "无法读取文件大小。")?.len();
    let mut head = Vec::new();
    BufReader::new((&mut file).take(options.head_bytes))
        .read_until(b'\n', &mut head)
        .map_err(|_| "无法读取文件首行。")?;
    let offset = size.saturating_sub(options.tail_bytes);
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "无法定位文件末尾。")?;
    let mut tail = Vec::new();
    file.take(options.tail_bytes)
        .read_to_end(&mut tail)
        .map_err(|_| "无法读取文件末尾。")?;
    let start = if offset == 0 {
        0
    } else {
        tail.iter()
            .position(|&byte| byte == b'\n')
            .map_or(tail.len(), |index| index + 1)
    };
    Ok(
        json!({ "head": String::from_utf8_lossy(&head), "tail": String::from_utf8_lossy(&tail[start..]) }),
    )
}

fn find_files(root: PathBuf, options: FindFiles) -> Result<Value, String> {
    let directory = dir(root)?;
    let mut pending = vec![(options.path.clone(), 1)];
    let mut found = Vec::new();
    let mut seen = 0;
    while let Some((path, depth)) = pending.pop() {
        let Ok(entries) = directory.read_dir(&path) else {
            continue;
        };
        for entry in entries {
            seen += 1;
            if seen > 20_000 {
                return Err("目录查询超过 20000 项。".into());
            }
            let Ok(entry) = entry else { continue };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let child = format!("{path}/{name}");
            if !path_valid(&child) {
                continue;
            }
            if kind.is_dir() && depth < options.depth {
                pending.push((child, depth + 1));
            } else if depth == options.depth
                && kind.is_file()
                && name.starts_with(&options.prefix)
                && name.ends_with(&options.suffix)
            {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                let Some(modified) = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.into_std().duration_since(std::time::UNIX_EPOCH).ok())
                else {
                    continue;
                };
                let modified = modified.as_millis() as u64;
                if modified >= options.modified_since {
                    found.push((modified, child, metadata.len()));
                }
            }
        }
    }
    found.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.truncate(options.limit);
    Ok(json!(found
        .into_iter()
        .map(|(modified, path, size)| {
            json!({ "path": path, "modified": modified, "size": size })
        })
        .collect::<Vec<_>>()))
}

fn system_readings() -> Value {
    struct Sampler {
        metrics: sysinfo::System,
        refreshed: Instant,
    }
    static SAMPLER: OnceLock<Mutex<Sampler>> = OnceLock::new();
    let sampler = SAMPLER.get_or_init(|| {
        let mut metrics = sysinfo::System::new();
        metrics.refresh_cpu_usage();
        thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        metrics.refresh_cpu_usage();
        metrics.refresh_memory();
        Mutex::new(Sampler {
            metrics,
            refreshed: Instant::now(),
        })
    });
    let Ok(mut sampler) = sampler.lock() else {
        return Value::Null;
    };
    if sampler.refreshed.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
        sampler.metrics.refresh_cpu_usage();
        sampler.metrics.refresh_memory();
        sampler.refreshed = Instant::now();
    }
    let metrics = &sampler.metrics;
    let total = metrics.total_memory();
    json!({
        "cpuUsagePercent": f64::from(metrics.global_cpu_usage()),
        "memoryUsedBytes": metrics.used_memory(),
        "memoryTotalBytes": total,
        "cpuPercent": (metrics.global_cpu_usage() * 10.0).round() / 10.0,
        "memoryPercent": (total != 0).then(|| {
            (1000.0 * metrics.used_memory() as f64 / total as f64).round() / 10.0
        }),
        "uptimeSeconds": sysinfo::System::uptime(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn definition(permission: Value) -> Definition {
        serde_json::from_value(json!({
            "name": "test", "permissions": { "source": permission },
            "outputs": {}, "widgets": {}
        }))
        .unwrap()
    }

    fn call(operation: &str, params: Value) -> Call {
        Call {
            operation: operation.into(),
            alias: "source".into(),
            params,
        }
    }

    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[test]
    fn http_uses_authorized_origin_and_does_not_follow_redirects() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut connection, _) = server.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&connection).read_line(&mut line).unwrap();
            assert_eq!(line.trim_end(), "GET /status?region=west HTTP/1.1");
            connection.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\n\r\n").unwrap();
        });
        let definition = definition(json!({ "kind": "http", "label": "test" }));
        let mut broker = Broker::live(
            &definition,
            BTreeMap::from([("source".into(), format!("http://{address}"))]),
            client(),
        );
        for params in [
            json!({ "path": "//127.0.0.1:1/" }),
            json!({ "method": "POST" }),
            json!({ "headers": { "Host": "other" } }),
        ] {
            assert!(broker.call(call("http", params)).is_err());
        }
        let result = broker
            .call(call("http", json!({ "path": "/status?region=west" })))
            .unwrap();
        assert_eq!(result["status"], 302);
        worker.join().unwrap();
    }

    #[test]
    fn fixture_matches_calls_and_enforces_the_same_policy() {
        let definition = definition(json!({ "kind": "http", "label": "test" }));
        let (_, mut broker) = Broker::fixture(
            &definition,
            &json!({ "calls": [
            { "operation": "http", "alias": "source", "params": { "path": "/ok" }, "result": 42 },
            { "operation": "http", "alias": "source", "params": { "path": "/ok" }, "result": 43 },
            { "operation": "http", "alias": "source", "params": { "method": "POST" }, "result": 42 },
            { "operation": "http", "alias": "source", "params": { "headers": { "X-Test": "a\nb" } }, "result": 42 }
        ] }),
        )
        .unwrap();
        assert_eq!(
            broker.call(call("http", json!({ "path": "/ok" }))).unwrap(),
            42
        );
        assert_eq!(
            broker.call(call("http", json!({ "path": "/ok" }))).unwrap(),
            43
        );
        assert!(broker
            .call(call("http", json!({ "headers": { "X-Test": "a\nb" } })))
            .is_err());
        assert!(broker
            .call(call("http", json!({ "path": "/missing" })))
            .is_err());
        assert!(broker
            .call(call("http", json!({ "method": "POST" })))
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_directory_grant_contains_reads_and_enumeration() {
        use std::os::unix::fs::symlink;
        let directory =
            std::env::temp_dir().join(format!("tdisplay-grant-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("granted")).unwrap();
        fs::write(directory.join("outside.txt"), "private").unwrap();
        fs::write(directory.join("granted/reading.txt"), "42").unwrap();
        symlink(&directory, directory.join("granted/link")).unwrap();
        let definition = definition(json!({ "kind": "directory", "label": "test" }));
        let mut bindings = BTreeMap::from([(
            "source".into(),
            directory.join("granted").display().to_string(),
        )]);
        bind(&definition, &mut bindings).unwrap();
        let mut broker = Broker::live(&definition, bindings, client());
        let result = broker.call(call("list", json!({ "path": "" }))).unwrap();
        assert_eq!(result[1]["name"], "reading.txt");
        assert_eq!(
            broker
                .call(call("readText", json!({ "path": "reading.txt" })))
                .unwrap(),
            "42"
        );
        assert!(broker
            .call(call("readText", json!({ "path": "../outside.txt" })))
            .is_err());
        assert!(broker
            .call(call("readText", json!({ "path": "link/outside.txt" })))
            .is_err());
        assert!(broker
            .call(call("list", json!({ "path": "link" })))
            .is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn process_requires_the_exact_approved_arguments() {
        let definition =
            definition(json!({ "kind": "process", "label": "test", "args": [["hello"]] }));
        let mut bindings = BTreeMap::from([("source".into(), "/bin/echo".into())]);
        bind(&definition, &mut bindings).unwrap();
        let mut broker = Broker::live(&definition, bindings, client());
        assert!(broker
            .call(call("run", json!({ "args": ["other"] })))
            .is_err());
        assert_eq!(
            broker
                .call(call("run", json!({ "args": ["hello"] })))
                .unwrap(),
            json!({ "code": 0, "stdout": "hello\n", "stderr": "" })
        );
    }
    #[cfg(unix)]
    #[test]
    fn stdio_broker_enforces_methods_and_uses_json_rpc_framing() {
        let program = r#"IFS= read -r notice
case "$notice" in *'"jsonrpc":"2.0","method":"ready"'*) ;; *) exit 1 ;; esac
IFS= read -r query
case "$query" in *'"id":1,"jsonrpc":"2.0","method":"reading"'*) ;; *) exit 2 ;; esac
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"value":73}}'"#;
        let definition = definition(json!({ "kind": "stdio", "label": "test",
            "args": ["-c", program], "methods": ["ready", "reading"]
        }));
        let mut bindings = BTreeMap::from([("source".into(), "/bin/sh".into())]);
        bind(&definition, &mut bindings).unwrap();
        let mut broker = Broker::live(&definition, bindings, client());
        assert!(broker
            .call(call("rpc", json!({ "method": "other" })))
            .is_err());
        broker
            .call(call("notify", json!({ "method": "ready" })))
            .unwrap();
        let result = broker
            .call(call("rpc", json!({ "method": "reading" })))
            .unwrap();
        assert_eq!(result["value"], 73);
    }
}
