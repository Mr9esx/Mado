mod metrics;
mod rpc;

use crate::{
    device::{Device, TaskReading, WidgetValue},
    SNAPSHOT_EVENT,
};
use metrics::{AccountLimits, RolloutCache, Speed};
use rpc::Failure;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};

const STATUS_EVENT: &str = "codex-status";
const ACCOUNT_INTERVAL: Duration = Duration::from_secs(120);
const LOCAL_INTERVAL: Duration = Duration::from_secs(15);
const MANUAL_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Settings {
    enabled: bool,
    cli_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Disabled,
    Checking,
    SigningIn,
    Connected,
    NeedsCli,
    NeedsLogin,
    UnsupportedLogin,
    Error,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStatus {
    sequence: u64,
    enabled: bool,
    cli_path: Option<PathBuf>,
    resolved_cli: Option<PathBuf>,
    phase: Phase,
    message: Option<&'static str>,
    plan: Option<String>,
    limits: AccountLimits,
    account_updated_at: Option<u64>,
    speed: Option<Speed>,
}

enum Action {
    Connect(Option<PathBuf>),
    Refresh,
    Pause,
    Login,
    CancelLogin,
}

pub struct Control {
    queue: mpsc::SyncSender<Action>,
    status: Arc<Mutex<CodexStatus>>,
}

impl Control {
    fn send(&self, action: Action) -> Result<(), String> {
        self.queue
            .try_send(action)
            .map_err(|_| "采集器正忙，请稍后重试。".into())
    }
}

pub fn start(app: AppHandle) -> Result<Control, Box<dyn std::error::Error>> {
    let home = app.path().home_dir()?;
    let settings_path = app.path().app_config_dir()?.join("codex-source.json");
    let settings: Settings = fs::read(&settings_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".codex"));
    let status = CodexStatus {
        sequence: 0,
        enabled: settings.enabled,
        cli_path: settings.cli_path.clone(),
        resolved_cli: rpc::find_cli(settings.cli_path.as_deref(), &home),
        phase: if settings.enabled {
            Phase::Checking
        } else {
            Phase::Disabled
        },
        message: None,
        plan: None,
        limits: AccountLimits::default(),
        account_updated_at: None,
        speed: None,
    };
    let (queue, receiver) = mpsc::sync_channel(4);
    let shared = Arc::new(Mutex::new(status.clone()));
    let now = Instant::now();
    let worker = Worker {
        app,
        home,
        codex_home,
        settings_path,
        settings,
        status,
        shared: Arc::clone(&shared),
        cache: RolloutCache::default(),
        task: None,
        next_account: now,
        next_local: now,
        last_attempt: None,
        login: None,
    };
    thread::spawn(move || worker.run(receiver));
    Ok(Control {
        queue,
        status: shared,
    })
}

struct Worker {
    app: AppHandle,
    home: PathBuf,
    codex_home: PathBuf,
    settings_path: PathBuf,
    settings: Settings,
    status: CodexStatus,
    shared: Arc<Mutex<CodexStatus>>,
    cache: RolloutCache,
    task: Option<TaskReading>,
    next_account: Instant,
    next_local: Instant,
    last_attempt: Option<Instant>,
    login: Option<rpc::Login>,
}

impl Worker {
    fn run(mut self, receiver: mpsc::Receiver<Action>) {
        loop {
            let timeout = if self.login.is_some() {
                Duration::from_millis(400)
            } else if self.settings.enabled {
                self.next_account
                    .min(self.next_local)
                    .saturating_duration_since(Instant::now())
            } else {
                Duration::from_secs(3600)
            };
            match receiver.recv_timeout(timeout) {
                Ok(Action::Connect(cli_path)) => self.configure(Settings {
                    enabled: true,
                    cli_path,
                }),
                Ok(Action::Pause) => self.configure(Settings {
                    enabled: false,
                    ..self.settings.clone()
                }),
                Ok(Action::Login) => self.start_login(),
                Ok(Action::CancelLogin) if self.login.take().is_some() => {
                    self.finish_login(false, "已取消登录。")
                }
                Ok(Action::Refresh) if self.settings.enabled && self.login.is_none() => {
                    self.refresh_requested()
                }
                Ok(Action::Refresh | Action::CancelLogin)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if let Some(success) = self.login.as_mut().and_then(rpc::Login::poll) {
                self.login.take();
                self.finish_login(
                    success,
                    "浏览器登录未完成，请重试或在终端运行 codex login。",
                );
            }
            if !self.settings.enabled || self.login.is_some() {
                continue;
            }
            let now = Instant::now();
            if self.next_account <= now {
                self.refresh_account();
            }
            if self.next_local <= now {
                let readings = self.cache.read(&self.codex_home, SystemTime::now());
                if self.status.speed != readings.speed || self.task != readings.task {
                    self.status.speed = readings.speed;
                    self.task = readings.task;
                    self.publish();
                }
                self.next_local = Instant::now() + LOCAL_INTERVAL;
            }
        }
    }

    fn configure(&mut self, settings: Settings) {
        if save_settings(&self.settings_path, &settings).is_err() {
            self.status.message = Some("无法保存设置，请检查应用配置目录的权限。");
            self.status.phase = Phase::Error;
            self.publish();
            return;
        }
        self.login.take();
        self.settings = settings;
        self.status.enabled = self.settings.enabled;
        self.status.cli_path = self.settings.cli_path.clone();
        self.status.resolved_cli = rpc::find_cli(self.settings.cli_path.as_deref(), &self.home);
        self.clear_account();
        self.status.speed = None;
        self.task = None;
        self.status.message = None;
        self.status.phase = if self.settings.enabled {
            Phase::Checking
        } else {
            Phase::Disabled
        };
        self.next_account = Instant::now();
        self.next_local = Instant::now();
        self.publish();
    }

    fn start_login(&mut self) {
        if !self.settings.enabled
            || !matches!(
                self.status.phase,
                Phase::NeedsLogin | Phase::UnsupportedLogin
            )
        {
            return;
        }
        let Some(path) = self.status.resolved_cli.clone() else {
            return;
        };
        self.status.phase = Phase::SigningIn;
        self.status.message = Some("请在默认浏览器完成 ChatGPT 登录。");
        self.publish();
        match rpc::begin_login(&path, &self.home) {
            Ok(login) => self.login = Some(login),
            Err(Failure::Unavailable(message)) => self.finish_login(false, message),
            Err(_) => self.finish_login(false, "无法发起浏览器登录，请在终端运行 codex login。"),
        }
    }

    fn finish_login(&mut self, success: bool, message: &'static str) {
        if success {
            self.status.phase = Phase::Checking;
            self.status.message = None;
            self.next_account = Instant::now();
            self.next_local = Instant::now();
        } else {
            self.status.phase = Phase::NeedsLogin;
            self.status.message = Some(message);
            self.next_account = Instant::now() + ACCOUNT_INTERVAL;
        }
        self.publish();
    }

    fn refresh_requested(&mut self) {
        self.next_local = Instant::now();
        if self
            .last_attempt
            .is_some_and(|attempt| attempt.elapsed() < MANUAL_INTERVAL)
        {
            self.status.message = Some("刚刚读取过账号额度，稍后可再次刷新。");
            self.publish();
            return;
        }
        self.next_account = Instant::now();
        self.status.phase = Phase::Checking;
        self.status.message = None;
        self.publish();
    }

    fn refresh_account(&mut self) {
        self.status.resolved_cli = rpc::find_cli(self.settings.cli_path.as_deref(), &self.home);
        self.last_attempt = Some(Instant::now());
        let account = match &self.status.resolved_cli {
            Some(path) => rpc::read_account(path, &self.home),
            None => Err(Failure::MissingCli),
        };
        self.next_account = Instant::now() + ACCOUNT_INTERVAL;
        match account {
            Ok(account) => {
                if let Some(home) = account.codex_home {
                    self.codex_home = home;
                }
                self.status.phase = Phase::Connected;
                self.status.message = None;
                self.status.plan = account.plan;
                self.status.limits = account.limits;
                self.status.account_updated_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|elapsed| elapsed.as_secs());
            }
            Err(error) => {
                self.clear_account();
                (self.status.phase, self.status.message) = match error {
                    Failure::MissingCli => (
                        Phase::NeedsCli,
                        Some("找不到 Codex CLI。安装后重新检测，或填入可执行文件的绝对路径。"),
                    ),
                    Failure::NeedsLogin => (Phase::NeedsLogin, None),
                    Failure::UnsupportedLogin => (
                        Phase::UnsupportedLogin,
                        Some("读取订阅额度需要用 ChatGPT 账号登录 Codex CLI。"),
                    ),
                    Failure::Unavailable(reason) => (Phase::Error, Some(reason)),
                };
            }
        }
        self.publish();
    }

    fn clear_account(&mut self) {
        self.status.plan = None;
        self.status.limits = AccountLimits::default();
        self.status.account_updated_at = None;
    }

    fn publish(&mut self) {
        self.status.sequence += 1;
        if let Ok(mut status) = self.shared.lock() {
            *status = self.status.clone();
        }
        let _ = self.app.emit_to("main", STATUS_EVENT, &self.status);

        let mut values = BTreeMap::new();
        if let Some(window) = self.status.limits.limiting_window() {
            values.insert(
                "quota".into(),
                WidgetValue::Number(window.remaining_percent),
            );
            if let Some(resets_at) = window.resets_at {
                values.insert("quota-reset".into(), WidgetValue::Number(resets_at));
            }
        }
        if let Some(count) = self.status.limits.reset_count {
            values.insert("resets".into(), WidgetValue::Number(count));
        }
        if let Some(speed) = &self.status.speed {
            values.insert("speed".into(), WidgetValue::Number(speed.tokens_per_second));
        }
        if let Some(task) = &self.task {
            values.insert("task".into(), WidgetValue::Task(task.clone()));
        }
        let device = self.app.state::<Mutex<Device>>();
        let snapshot = device
            .lock()
            .ok()
            .and_then(|mut device| device.update_values(values));
        if let Some(snapshot) = snapshot {
            let _ = self.app.emit(SNAPSHOT_EVENT, snapshot);
        }
    }
}

fn save_settings(path: &Path, settings: &Settings) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().expect("settings have a parent directory"))?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec(settings)?)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn get_codex_status(control: State<'_, Control>) -> Result<CodexStatus, String> {
    control
        .status
        .lock()
        .map(|status| status.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn connect_codex(control: State<'_, Control>, cli_path: Option<String>) -> Result<(), String> {
    let cli_path = cli_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    control.send(Action::Connect(cli_path))
}

#[tauri::command]
pub fn refresh_codex(control: State<'_, Control>) -> Result<(), String> {
    control.send(Action::Refresh)
}

#[tauri::command]
pub fn pause_codex(control: State<'_, Control>) -> Result<(), String> {
    control.send(Action::Pause)
}

#[tauri::command]
pub fn login_codex(control: State<'_, Control>) -> Result<(), String> {
    control.send(Action::Login)
}

#[tauri::command]
pub fn cancel_codex_login(control: State<'_, Control>) -> Result<(), String> {
    control.send(Action::CancelLogin)
}
