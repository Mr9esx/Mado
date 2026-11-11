use crate::{device::Device, display_wire, wifi_display, SNAPSHOT_EVENT};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, ErrorKind},
    net::Ipv4Addr,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Listener, Manager, State};

const EVENT: &str = "display-connection-status";
const MAX_FRAME: usize = 65_536;
const SESSION_BUSY: &str = "设备正由另一个连接控制。";
struct Reply {
    sender: mpsc::Sender<Result<(), String>>,
    deadline: Instant,
}

impl Reply {
    fn send(self, result: Result<(), String>) {
        let _ = self.sender.send(result);
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}
enum Command {
    Page(String, Reply),
    Setting(&'static str, Value, Reply),
    Wifi {
        expected_id: String,
        kind: &'static str,
        payload: Value,
        deadline: Instant,
        reply: mpsc::Sender<Result<Value, String>>,
    },
    Stop,
}
struct Control {
    id: uuid::Uuid,
    tx: mpsc::Sender<Command>,
    worker: thread::JoinHandle<()>,
    #[cfg(target_os = "macos")]
    cancel: tokio_util::sync::CancellationToken,
}
impl Control {
    fn stop(self) {
        let _ = self.tx.send(Command::Stop);
        #[cfg(target_os = "macos")]
        self.cancel.cancel();
        let _ = self.worker.join();
    }
}

pub struct DisplayConnection {
    control: Mutex<Option<Control>>,
    port_operation: Mutex<()>,
    provisioning: Mutex<()>,
    status: Arc<Mutex<Value>>,
}
impl Default for DisplayConnection {
    fn default() -> Self {
        Self {
            control: Mutex::new(None),
            port_operation: Mutex::new(()),
            provisioning: Mutex::new(()),
            status: Arc::new(Mutex::new(json!({"phase":"waiting"}))),
        }
    }
}
impl DisplayConnection {
    pub(crate) fn with_firmware_port<T>(
        &self,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self
            .port_operation
            .try_lock()
            .map_err(|_| "设备正忙，请稍后重试。")?;
        self.stop()?;
        operation()
    }

    pub(crate) fn firmware_port_in_use(&self) -> bool {
        matches!(
            self.port_operation.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        )
    }

    fn stop(&self) -> Result<(), String> {
        let mut control = self.control.lock().map_err(|_| "设备控制锁不可用。")?;
        if let Some(task) = control.take() {
            task.stop();
        }
        Ok(())
    }
    fn send(&self, make: impl FnOnce(Reply) -> Command) -> Result<(), String> {
        let (sender, wait) = mpsc::channel();
        let deadline = Instant::now() + Duration::from_secs(12);
        let reply = Reply { sender, deadline };
        self.control
            .lock()
            .map_err(|_| "设备控制锁不可用。")?
            .as_ref()
            .ok_or("尚未连接设备。")?
            .tx
            .send(make(reply))
            .map_err(|_| "设备连接已关闭。")?;
        wait.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| "设备未确认操作。")?
    }

    fn control_id(&self) -> Result<uuid::Uuid, String> {
        self.control
            .lock()
            .map_err(|_| "设备控制锁不可用。")?
            .as_ref()
            .map(|task| task.id)
            .ok_or_else(|| "请先通过蓝牙或 USB 连接设备。".into())
    }

    fn wifi_request(
        &self,
        control_id: uuid::Uuid,
        expected_id: &str,
        kind: &'static str,
        payload: Value,
    ) -> Result<Value, String> {
        let (reply, answer) = mpsc::channel();
        let timeout = Duration::from_secs(12);
        self.control
            .lock()
            .map_err(|_| "设备控制锁不可用。")?
            .as_ref()
            .filter(|task| task.id == control_id)
            .ok_or("配网期间设备连接已变更。")?
            .tx
            .send(Command::Wifi {
                expected_id: expected_id.into(),
                kind,
                payload,
                deadline: Instant::now() + timeout,
                reply,
            })
            .map_err(|_| "设备连接已关闭。")?;
        answer
            .recv_timeout(timeout)
            .map_err(|_| "设备未确认配网操作。")?
    }
}

pub(crate) fn esp_usb_ports() -> impl Iterator<Item = (String, serialport::UsbPortInfo)> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(info)
                if info.vid == 0x303a
                    && info.pid == 0x1001
                    && !p.port_name.starts_with("/dev/tty.") =>
            {
                Some((p.port_name, info))
            }
            _ => None,
        })
}

fn ports() -> Vec<String> {
    let mut ports: Vec<_> = esp_usb_ports().map(|(name, _)| name).collect();
    ports.sort();
    ports.dedup();
    ports
}

#[tauri::command]
pub fn list_usb_displays() -> Vec<String> {
    ports()
}

#[tauri::command]
pub fn get_display_connection_status(usb: State<'_, DisplayConnection>) -> Value {
    usb.status.lock().map(|s| s.clone()).unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn connect_usb_display(
    app: AppHandle,
    usb: State<'_, DisplayConnection>,
    port: String,
    interval_ms: u64,
    utc_offset_minutes: i16,
) -> Result<(), String> {
    if !ports().contains(&port) {
        return Err("请选择已识别的 ESP32 USB Serial/JTAG 设备。".into());
    }
    start(
        app,
        usb.inner(),
        Endpoint::Usb(port),
        interval_ms,
        utc_offset_minutes,
    )
}

#[tauri::command]
pub async fn connect_ble_display(
    app: AppHandle,
    usb: State<'_, DisplayConnection>,
    device_id: String,
    interval_ms: u64,
    utc_offset_minutes: i16,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        uuid::Uuid::parse_str(&device_id).map_err(|_| "蓝牙设备标识无效。")?;
        start(
            app,
            usb.inner(),
            Endpoint::Ble(device_id),
            interval_ms,
            utc_offset_minutes,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, usb, device_id, interval_ms, utc_offset_minutes);
        Err("此版本仅在 macOS 上支持蓝牙连接。".into())
    }
}

fn wifi_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("wifi-devices"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scan_wifi_displays(app: AppHandle) -> Result<Vec<wifi_display::Choice>, String> {
    let directory = wifi_directory(&app)?;
    tauri::async_runtime::spawn_blocking(move || wifi_display::scan(&directory))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn connect_wifi_display(
    app: AppHandle,
    device_id: String,
    address: Option<String>,
    interval_ms: u64,
    utc_offset_minutes: i16,
) -> Result<(), String> {
    let address = address
        .map(|value| {
            value
                .parse::<Ipv4Addr>()
                .ok()
                .filter(|ip| !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast())
                .ok_or("请输入设备屏幕上显示的 IPv4 地址。")
        })
        .transpose()?;
    let credentials = wifi_display::Stored::load(&wifi_directory(&app)?, &device_id)?.candidates();
    if credentials.is_empty() {
        return Err("请先通过蓝牙或 USB 配置这块屏幕的 Wi-Fi。".into());
    }
    let display = app.state::<DisplayConnection>();
    start(
        app.clone(),
        display.inner(),
        Endpoint::Wifi(wifi_display::Target {
            id: device_id,
            address,
            credentials,
        }),
        interval_ms,
        utc_offset_minutes,
    )
}

#[tauri::command]
pub async fn provision_wifi_display(
    app: AppHandle,
    ssid: String,
    password: String,
    interval_ms: u64,
    utc_offset_minutes: i16,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        provision_wifi(&app, ssid, password, interval_ms, utc_offset_minutes)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn provision_wifi(
    app: &AppHandle,
    ssid: String,
    password: String,
    interval: u64,
    offset: i16,
) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 || ssid.contains('\0') {
        return Err("Wi-Fi 网络名须为 1–32 字节。".into());
    }
    if !password.is_empty()
        && !((8..=63).contains(&password.len())
            && password.bytes().all(|b| (b' '..=b'~').contains(&b))
            || password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("密码须为 8–63 个可打印 ASCII 字符，或 64 位十六进制。开放网络可留空。".into());
    }
    if !(500..=60_000).contains(&interval) || !(-840..=840).contains(&offset) {
        return Err("推送周期须在 500–60000 ms 之间。".into());
    }
    let display = app.state::<DisplayConnection>();
    let _provisioning = display
        .provisioning
        .try_lock()
        .map_err(|_| "已有 Wi-Fi 配网正在进行。")?;
    let status = display
        .status
        .lock()
        .map_err(|_| "设备状态不可用。")?
        .clone();
    if status["phase"] != "connected"
        || !matches!(status["transport"].as_str(), Some("usb" | "ble"))
    {
        return Err("请先通过蓝牙或 USB 连接设备。".into());
    }
    if status["wifiProvisioning"] != true {
        return Err("请更新设备固件后再配置 Wi-Fi。".into());
    }
    let id = status["deviceId"].as_str().ok_or("设备标识无效。")?;
    let directory = wifi_directory(app)?;
    let control_id = display.control_id()?;
    let saved = display.wifi_request(control_id, id, "wifi.status", json!({}))?;
    if matches!(saved["phase"].as_str(), Some("joining" | "forgetting")) {
        return Err("设备正在处理上一次 Wi-Fi 操作，请稍后再试。".into());
    }
    let mut previous = wifi_display::Stored::load(&directory, id)?;
    let saved_id = saved["savedProfileId"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok());
    previous.reconcile(saved["saved"] == true, saved_id);
    let material = wifi_display::Provisioning::new()?;
    let request_id = uuid::Uuid::new_v4();
    let stored = wifi_display::Stored {
        active: previous.active.clone(),
        pending: Some(material.client.clone()),
        pending_id: Some(request_id),
    };
    stored.save(&directory, id)?;
    let request = material.request(request_id, ssid, password);
    let request_id = request_id.to_string();
    let mut reply = display.wifi_request(control_id, id, "wifi.provision", request)?;
    let deadline = Instant::now() + Duration::from_secs(28);
    let address = loop {
        if reply["requestId"] != request_id {
            return Err("配网状态已变更，请检查设备的 Wi-Fi 页面。".into());
        }
        match reply["phase"].as_str() {
            Some("ready") => {
                break reply["address"]
                    .as_str()
                    .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
                    .ok_or("设备已入网，但没有返回可用的 IPv4 地址。")?
            }
            Some("failed") => {
                previous.save(&directory, id)?;
                return Err(match reply["error"].as_str() {
                    Some("AUTH_FAILED") => "Wi-Fi 认证失败，请核对密码。",
                    Some("NETWORK_NOT_FOUND") => "设备找不到该 Wi-Fi，请核对网络名和信号。",
                    Some("STORAGE_FAILED") => "设备无法保存 Wi-Fi 配置。",
                    Some("RADIO_FAILED") => "设备无法启动 Wi-Fi。",
                    _ => "设备未能从 Wi-Fi 获得 IPv4 地址。",
                }
                .into());
            }
            Some("joining") if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(500))
            }
            _ => return Err("等待设备入网超时，请检查设备上的 Wi-Fi 状态。".into()),
        }
        reply = display.wifi_request(control_id, id, "wifi.status", json!({}))?;
    };

    // Keep the current display session until the new network is reachable from this Mac.
    thread::sleep(Duration::from_millis(300));
    if wifi_display::verify_at(address, &material.client).is_err() {
        return Err(format!("设备已入网（{address}），但 Studio 无法通过局域网连接。请让电脑加入同一网络，然后选择 Wi-Fi 连接。"));
    }
    wifi_display::Stored {
        active: Some(material.client.clone()),
        pending: None,
        pending_id: None,
    }
    .save(&directory, id)?;
    start_if_current(
        app.clone(),
        display.inner(),
        Endpoint::Wifi(wifi_display::Target {
            id: id.to_owned(),
            address: Some(address),
            credentials: vec![material.client],
        }),
        interval,
        offset,
        Some(control_id),
    )
}

fn start(
    app: AppHandle,
    display: &DisplayConnection,
    endpoint: Endpoint,
    interval: u64,
    offset: i16,
) -> Result<(), String> {
    start_if_current(app, display, endpoint, interval, offset, None)
}

fn start_if_current(
    app: AppHandle,
    display: &DisplayConnection,
    endpoint: Endpoint,
    interval: u64,
    offset: i16,
    expected: Option<uuid::Uuid>,
) -> Result<(), String> {
    if !(500..=60_000).contains(&interval) || !(-840..=840).contains(&offset) {
        return Err("推送周期须在 500–60000 ms 之间。".into());
    }
    let _guard = display
        .port_operation
        .try_lock()
        .map_err(|_| "设备正忙，请稍后重试。")?;
    let mut control = display.control.lock().map_err(|_| "设备控制锁不可用。")?;
    if expected.is_some() && control.as_ref().map(|task| task.id) != expected {
        return Err("Wi-Fi 配对已保存。当前连接已变更，请从设备面板选择 Wi-Fi。".into());
    }
    if let Some(task) = control.take() {
        task.stop();
    }
    let (tx, rx) = mpsc::channel();
    let status = display.status.clone();
    #[cfg(target_os = "macos")]
    let (cancel, worker_cancel) = {
        let token = tokio_util::sync::CancellationToken::new();
        (token.clone(), token)
    };
    let worker = thread::spawn(move || {
        worker(
            app,
            status,
            endpoint,
            interval,
            offset,
            rx,
            #[cfg(target_os = "macos")]
            worker_cancel,
        );
    });
    *control = Some(Control {
        id: uuid::Uuid::new_v4(),
        tx,
        worker,
        #[cfg(target_os = "macos")]
        cancel,
    });
    Ok(())
}

#[tauri::command]
pub async fn disconnect_display(usb: State<'_, DisplayConnection>) -> Result<(), String> {
    usb.stop()
}

#[tauri::command]
pub async fn select_display_page(
    usb: State<'_, DisplayConnection>,
    page_id: String,
) -> Result<(), String> {
    usb.send(|reply| Command::Page(page_id, reply))
}

#[tauri::command]
pub async fn set_display_brightness(
    usb: State<'_, DisplayConnection>,
    percent: u8,
) -> Result<(), String> {
    if percent > 100 {
        return Err("亮度须在 0–100 之间。".into());
    }
    usb.send(|reply| Command::Setting("brightnessPercent", json!(percent), reply))
}

#[tauri::command]
pub async fn set_display_rotation(
    usb: State<'_, DisplayConnection>,
    rotated: bool,
) -> Result<(), String> {
    usb.send(|reply| Command::Setting("screenRotated", json!(rotated), reply))
}

fn emit(app: &AppHandle, status: &Arc<Mutex<Value>>, value: Value) {
    if let Ok(mut current) = status.lock() {
        *current = value.clone();
    }
    let _ = app.emit(EVENT, &value);
}

enum Endpoint {
    Usb(String),
    Wifi(wifi_display::Target),
    #[cfg(target_os = "macos")]
    Ble(String),
}

trait DisplayIo: io::Read + io::Write {}
impl<T: io::Read + io::Write> DisplayIo for T {}

impl Endpoint {
    fn transport(&self) -> &'static str {
        match self {
            Self::Usb(_) => "usb",
            Self::Wifi(_) => "wifi",
            #[cfg(target_os = "macos")]
            Self::Ble(_) => "ble",
        }
    }

    fn port(&self) -> Option<&str> {
        match self {
            Self::Usb(path) => Some(path),
            Self::Wifi(_) => None,
            #[cfg(target_os = "macos")]
            Self::Ble(_) => None,
        }
    }

    fn open(
        &self,
        #[cfg(target_os = "macos")] cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<Box<dyn DisplayIo>, String> {
        match self {
            Self::Usb(path) => {
                let port = serialport::new(path, 115_200)
                    .timeout(Duration::from_millis(100))
                    .open()
                    .map_err(|e| e.to_string())?;
                port.clear(serialport::ClearBuffer::Input)
                    .map_err(|e| e.to_string())?;
                Ok(Box::new(port))
            }
            Self::Wifi(target) => Ok(Box::new(target.open()?)),
            #[cfg(target_os = "macos")]
            Self::Ble(id) => Ok(Box::new(crate::ble_display::open(id, cancel.clone())?)),
        }
    }
}

struct Link<T> {
    port: T,
    session: Value,
    id: u64,
    input: Vec<u8>,
    discard: bool,
    status: Value,
}

impl<T: io::Read + io::Write> Link<T> {
    fn new(port: T) -> Self {
        Self {
            port,
            session: Value::Null,
            id: 0,
            input: Vec::new(),
            discard: false,
            status: json!({}),
        }
    }

    fn request(&mut self, kind: &str, payload: Value) -> Result<Value, String> {
        self.id += 1;
        let mut wire = serde_json::to_vec(&json!({
            "protocolVersion":1,"sessionId":self.session,"id":self.id,"type":kind,"payload":payload
        }))
        .map_err(|e| e.to_string())?;
        if wire.len() > MAX_FRAME {
            return Err("显示数据超过设备消息上限。".into());
        }
        wire.insert(0, b'\n');
        wire.push(b'\n');
        for _ in 0..3 {
            self.port.write_all(&wire).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                let Some(message) = self.receive().map_err(|e| e.to_string())? else {
                    continue;
                };
                if message["protocolVersion"] != 1 {
                    continue;
                }
                if message["sessionId"] != self.session
                    && kind != "hello"
                    && !message["sessionId"].is_null()
                {
                    continue;
                }
                let payload = &message["payload"];
                if message["type"] == "device.status" && message["sessionId"] == self.session {
                    if payload["statusSequence"].as_u64() > self.status["statusSequence"].as_u64() {
                        self.status = payload.clone();
                    }
                    continue;
                }
                if payload["replyTo"] != self.id {
                    continue;
                }
                if kind == "hello"
                    && message["type"] == "welcome"
                    && message["sessionId"].is_string()
                {
                    self.session = message["sessionId"].clone();
                    return Ok(payload.clone());
                }
                if message["type"] != "result" {
                    continue;
                }
                if payload["ok"] == true {
                    return Ok(payload["data"].clone());
                }
                let code = payload["error"]["code"].as_str().unwrap_or("UNKNOWN");
                if kind == "hello" && code == "SESSION_BUSY" {
                    return Err(SESSION_BUSY.into());
                }
                return Err(format!("设备拒绝 {kind}（{code}）。"));
            }
        }
        Err(format!("设备未确认 {kind}。"))
    }
    fn receive(&mut self) -> io::Result<Option<Value>> {
        loop {
            if let Some(end) = self.input.iter().position(|&b| b == b'\n') {
                let line: Vec<_> = self.input.drain(..=end).collect();
                if std::mem::take(&mut self.discard) || line.len() > MAX_FRAME + 1 {
                    continue;
                }
                if let Ok(value) = serde_json::from_slice(&line) {
                    return Ok(Some(value));
                }
                continue;
            }
            if self.input.len() > MAX_FRAME {
                self.input.clear();
                self.discard = true;
                return Ok(None);
            }
            let mut chunk = [0; 1024];
            match self.port.read(&mut chunk) {
                Ok(0) => return Ok(None),
                Ok(count) => self.input.extend_from_slice(&chunk[..count]),
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    return Ok(None)
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn worker(
    app: AppHandle,
    status: Arc<Mutex<Value>>,
    endpoint: Endpoint,
    interval: u64,
    offset: i16,
    rx: mpsc::Receiver<Command>,
    #[cfg(target_os = "macos")] cancel: tokio_util::sync::CancellationToken,
) {
    let pending = Arc::new(Mutex::new(BTreeSet::new()));
    let queued = pending.clone();
    let listener = app.listen(SNAPSHOT_EVENT, move |event| {
        let Ok(snapshot) = serde_json::from_str::<Value>(event.payload()) else {
            return;
        };
        let Some(values) = snapshot["values"].as_object() else {
            return;
        };
        if let Ok(mut events) = queued.lock() {
            for (id, reading) in values {
                if reading["notify"] == true {
                    if let Some(state) = reading["state"].as_str() {
                        events.insert((id.clone(), state.to_owned()));
                    }
                }
            }
        }
    });
    let mut last =
        json!({"phase":"syncing","port":endpoint.port(),"transport":endpoint.transport()});
    emit(&app, &status, last.clone());
    let mut failed = false;
    loop {
        let result = endpoint
            .open(
                #[cfg(target_os = "macos")]
                &cancel,
            )
            .and_then(|port| {
                stream(
                    &app,
                    &status,
                    &endpoint,
                    Link::new(port),
                    interval,
                    offset,
                    &rx,
                    &pending,
                    &mut last,
                )
            });
        match result {
            Ok(()) => break,
            Err(reason) => {
                #[cfg(target_os = "macos")]
                if cancel.is_cancelled() {
                    break;
                }
                let had_layout = last["hasLayout"] == true;
                last["phase"] = json!(if had_layout { "disconnected" } else { "failed" });
                let busy = reason == SESSION_BUSY;
                last["reason"] = json!(reason);
                if !had_layout && !busy && matches!(endpoint.transport(), "ble" | "wifi") {
                    failed = true;
                    break;
                }
                last["reconnecting"] = json!(true);
                emit(&app, &status, last.clone());
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Command::Page(_, reply) | Command::Setting(_, _, reply)) => {
                        reply.send(Err("设备正在重连。".into()));
                    }
                    Ok(Command::Wifi { reply, .. }) => {
                        let _ = reply.send(Err("设备正在重连。".into()));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }
    app.unlisten(listener);
    if !failed {
        last["phase"] = json!(if last["hasLayout"] == true {
            "disconnected"
        } else {
            "waiting"
        });
        last["reason"] = Value::Null;
    }
    last["reconnecting"] = json!(false);
    emit(&app, &status, last);
}

fn stream(
    app: &AppHandle,
    status: &Arc<Mutex<Value>>,
    endpoint: &Endpoint,
    mut link: Link<Box<dyn DisplayIo>>,
    interval: u64,
    offset: i16,
    rx: &mpsc::Receiver<Command>,
    pending: &Mutex<BTreeSet<(String, String)>>,
    last: &mut Value,
) -> Result<(), String> {
    let welcome = link.request(
        "hello",
        json!({"supportedProtocolVersions":[1],
        "appVersion":env!("CARGO_PKG_VERSION"),"clientNonce":uuid::Uuid::new_v4()}),
    )?;
    if welcome["profileId"] != "tdisplay-c5-v1" {
        return Err("设备屏幕规格不匹配。".into());
    }
    if let Endpoint::Wifi(target) = endpoint {
        if welcome["deviceId"] != target.id {
            return Err("Wi-Fi 设备身份与配对记录不匹配。".into());
        }
    }
    let supports_wifi = welcome["capabilities"]["wifiProvisioning"] == true;
    let supports_setting = |key: &str| {
        welcome["capabilities"]["settings"]
            .as_array()
            .is_some_and(|settings| settings.iter().any(|setting| setting.as_str() == Some(key)))
    };
    let supports_rotation = supports_setting("screenRotated");
    let mut revision = welcome["configRevision"]
        .as_u64()
        .ok_or("设备没有返回配置版本。")?;
    let mut sequence = 0;
    let mut applied = Value::Null;
    let mut updated_at = Value::Null;
    let mut preview = last["preview"].clone();
    let mut clock = Instant::now();
    let mut ping = Instant::now();
    let interval = Duration::from_millis(interval);
    let mut next = Instant::now();
    link.request(
        "time.sync",
        json!({"unixMs":now_ms(),"utcOffsetMinutes":offset}),
    )?;
    if let Ok(mut events) = pending.lock() {
        events.clear();
    }
    loop {
        match rx.recv_timeout(
            next.saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        ) {
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if endpoint.transport() == "usb" {
                    let _ = link.request("session.close", json!({}));
                }
                return Ok(());
            }
            Ok(Command::Page(_, ref reply) | Command::Setting(_, _, ref reply))
                if reply.expired() => {}
            Ok(Command::Page(page, reply)) => {
                let value =
                    link.request("page.set", json!({"configRevision":revision,"pageId":page}));
                if let Ok(data) = &value {
                    link.status["activePageId"] = data["activePageId"].clone();
                }
                reply.send(value.map(|_| ()));
            }
            Ok(Command::Setting(key, value, reply)) => {
                let result = if supports_setting(key) {
                    link.request("settings.set", json!({(key):value}))
                        .and_then(|data| {
                            if data[key] != value {
                                return Err("设备未确认设置值。".into());
                            }
                            link.status[key] = value;
                            Ok(())
                        })
                } else {
                    Err("当前固件不支持此屏幕设置。".into())
                };
                reply.send(result);
            }
            Ok(Command::Wifi {
                expected_id,
                kind,
                payload,
                deadline,
                reply,
            }) => {
                let result = if Instant::now() >= deadline {
                    Err("配网请求已超时。".into())
                } else if !supports_wifi
                    || endpoint.transport() == "wifi"
                    || welcome["deviceId"] != expected_id
                {
                    Err("请通过蓝牙或 USB 连接要配网的设备。".into())
                } else {
                    link.request(kind, payload)
                };
                let _ = reply.send(result);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if clock.elapsed() >= Duration::from_secs(300) {
            link.request(
                "time.sync",
                json!({"unixMs":now_ms(),"utcOffsetMinutes":offset}),
            )?;
            clock = Instant::now();
        }
        if ping.elapsed() >= Duration::from_secs(5) {
            link.request("ping", json!({}))?;
            ping = Instant::now();
        }
        if Instant::now() >= next {
            let snapshot = app
                .state::<Mutex<Device>>()
                .lock()
                .map_err(|_| "布局状态不可用。")?
                .snapshot();
            let snapshot = serde_json::to_value(snapshot).map_err(|e| e.to_string())?;
            let mut frame = display_wire::project(&snapshot)?;
            let initial = applied != frame.config;
            if initial {
                let data = link.request(
                    "config.set",
                    json!({"baseRevision":revision,"config":frame.config}),
                )?;
                revision = data["configRevision"]
                    .as_u64()
                    .ok_or("设备没有确认配置版本。")?;
                applied = frame.config.clone();
            }
            if let Ok(mut queue) = pending.lock() {
                let events = std::mem::take(&mut *queue);
                if !initial {
                    display_wire::add_alerts(&mut frame, &events);
                }
            }
            sequence += 1;
            let data = link.request(
                "readings.set",
                json!({"configRevision":revision,"sequence":sequence,
                "mode":if initial {"snapshot"} else {"update"},"readings":frame.readings}),
            )?;
            if data["acceptedSequence"] != sequence || data["configRevision"] != revision {
                return Err("设备确认的读数版本不匹配。".into());
            }
            updated_at = json!(now_ms());
            preview = snapshot;
            preview["displayFrame"] = json!({
                "config":frame.config,"readings":frame.readings,"sourceIds":frame.source_ids,
                "configRevision":revision,"sequence":sequence
            });
            preview["sequence"] = json!(sequence);
            next = Instant::now() + interval;
        }
        let value = json!({
            "phase":if applied.is_null() {"syncing"} else {"connected"},
            "transport":endpoint.transport(),"port":endpoint.port(),"firmwareVersion":welcome["firmwareVersion"],
            "deviceId":welcome["deviceId"],"wifiProvisioning":supports_wifi,"activePageId":link.status["activePageId"],
            "brightness":link.status["brightnessPercent"],"power":link.status["power"],
            "screenRotated":supports_rotation.then(|| link.status["screenRotated"].as_bool()).flatten(),
            "lastReadingsAppliedAtUnixMs":updated_at,"hasLayout":!applied.is_null(),
            "preview":preview,"reconnecting":false
        });
        if *last != value {
            *last = value;
            emit(app, status, last.clone());
        }
    }
}
