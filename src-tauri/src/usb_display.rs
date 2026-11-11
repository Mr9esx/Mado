use crate::{device::Device, display_wire, SNAPSHOT_EVENT};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, ErrorKind},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Listener, Manager, State};

const EVENT: &str = "usb-display-status";
const MAX_FRAME: usize = 65_536;
type Reply = mpsc::Sender<Result<(), String>>;
enum Command {
    Page(String, Reply),
    Setting(&'static str, Value, Reply),
    Stop,
}
struct Control {
    tx: mpsc::Sender<Command>,
    worker: thread::JoinHandle<()>,
}
pub struct UsbDisplay {
    control: Mutex<Option<Control>>,
    status: Arc<Mutex<Value>>,
}
impl Default for UsbDisplay {
    fn default() -> Self {
        Self {
            control: Mutex::new(None),
            status: Arc::new(Mutex::new(json!({"phase":"waiting"}))),
        }
    }
}
impl UsbDisplay {
    fn stop(&self) -> Result<(), String> {
        let mut control = self.control.lock().map_err(|_| "设备控制锁不可用。")?;
        if let Some(task) = control.take() {
            let _ = task.tx.send(Command::Stop);
            let _ = task.worker.join();
        }
        Ok(())
    }
    fn send(&self, make: impl FnOnce(Reply) -> Command) -> Result<(), String> {
        let (reply, wait) = mpsc::channel();
        self.control
            .lock()
            .map_err(|_| "设备控制锁不可用。")?
            .as_ref()
            .ok_or("尚未连接设备。")?
            .tx
            .send(make(reply))
            .map_err(|_| "设备连接已关闭。")?;
        wait.recv_timeout(Duration::from_secs(12))
            .map_err(|_| "设备未确认操作。")?
    }
}

fn ports() -> Vec<String> {
    let mut ports: Vec<_> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(info)
                if info.vid == 0x303a && info.pid == 0x1001 =>
            {
                Some(p.port_name)
            }
            _ => None,
        })
        .filter(|name| !name.starts_with("/dev/tty."))
        .collect();
    ports.sort();
    ports.dedup();
    ports
}

#[tauri::command]
pub fn list_usb_displays() -> Vec<String> {
    ports()
}

#[tauri::command]
pub fn get_usb_display_status(usb: State<'_, UsbDisplay>) -> Value {
    usb.status.lock().map(|s| s.clone()).unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn connect_usb_display(
    app: AppHandle,
    usb: State<'_, UsbDisplay>,
    port: String,
    interval_ms: u64,
    utc_offset_minutes: i16,
) -> Result<(), String> {
    if !ports().contains(&port) {
        return Err("请选择已识别的 ESP32 USB Serial/JTAG 设备。".into());
    }
    if !(500..=60_000).contains(&interval_ms) || !(-840..=840).contains(&utc_offset_minutes) {
        return Err("推送周期须在 500–60000 ms 之间。".into());
    }
    usb.stop()?;
    let mut control = usb.control.lock().map_err(|_| "设备控制锁不可用。")?;
    if control.is_some() {
        return Err("另一个连接正在建立。".into());
    }
    let (tx, rx) = mpsc::channel();
    let status = usb.status.clone();
    let worker =
        thread::spawn(move || worker(app, status, port, interval_ms, utc_offset_minutes, rx));
    *control = Some(Control { tx, worker });
    Ok(())
}

#[tauri::command]
pub async fn disconnect_usb_display(usb: State<'_, UsbDisplay>) -> Result<(), String> {
    usb.stop()
}

#[tauri::command]
pub async fn select_usb_display_page(
    usb: State<'_, UsbDisplay>,
    page_id: String,
) -> Result<(), String> {
    usb.send(|reply| Command::Page(page_id, reply))
}

#[tauri::command]
pub async fn set_usb_display_brightness(
    usb: State<'_, UsbDisplay>,
    percent: u8,
) -> Result<(), String> {
    if percent > 100 {
        return Err("亮度须在 0–100 之间。".into());
    }
    usb.send(|reply| Command::Setting("brightnessPercent", json!(percent), reply))
}

#[tauri::command]
pub async fn set_usb_display_rotation(usb: State<'_, UsbDisplay>, rotated: bool) -> Result<(), String> {
    usb.send(|reply| Command::Setting("screenRotated", json!(rotated), reply))
}

fn emit(app: &AppHandle, status: &Arc<Mutex<Value>>, value: Value) {
    if let Ok(mut current) = status.lock() {
        *current = value.clone();
    }
    let _ = app.emit(EVENT, &value);
}

struct Link<T> {
    port: T,
    session: Value,
    id: u64,
    input: Vec<u8>,
    discard: bool,
    status: Value,
}
impl Link<Box<dyn serialport::SerialPort>> {
    fn open(path: &str) -> Result<Self, String> {
        let port = serialport::new(path, 115_200)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| e.to_string())?;
        port.clear(serialport::ClearBuffer::Input)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            port,
            session: Value::Null,
            id: 0,
            input: Vec::new(),
            discard: false,
            status: json!({}),
        })
    }
}

impl<T: io::Read + io::Write> Link<T> {
    fn request(&mut self, kind: &str, payload: Value) -> Result<Value, String> {
        self.id += 1;
        let mut wire = serde_json::to_vec(&json!({
            "protocolVersion":1,"sessionId":self.session,"id":self.id,"type":kind,"payload":payload
        }))
        .map_err(|e| e.to_string())?;
        if wire.len() > MAX_FRAME {
            return Err("显示数据超过 USB 消息上限。".into());
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
                return Err(format!(
                    "设备拒绝 {kind}（{}）。",
                    payload["error"]["code"].as_str().unwrap_or("UNKNOWN")
                ));
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
    port: String,
    interval: u64,
    offset: i16,
    rx: mpsc::Receiver<Command>,
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
    let mut last = json!({"phase":"syncing","port":port,"transport":"usb"});
    emit(&app, &status, last.clone());
    loop {
        match stream(
            &app, &status, &port, interval, offset, &rx, &pending, &mut last,
        ) {
            Ok(()) => break,
            Err(reason) => {
                let had_layout = last["hasLayout"] == true;
                last["phase"] = json!(if had_layout { "disconnected" } else { "failed" });
                last["reason"] = json!(reason);
                last["reconnecting"] = json!(true);
                emit(&app, &status, last.clone());
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Command::Page(_, reply) | Command::Setting(_, _, reply)) => {
                        let _ = reply.send(Err("设备正在重连。".into()));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    }
    app.unlisten(listener);
    last["phase"] = json!(if last["hasLayout"] == true {
        "disconnected"
    } else {
        "waiting"
    });
    last["reason"] = Value::Null;
    last["reconnecting"] = json!(false);
    emit(&app, &status, last);
}

fn stream(
    app: &AppHandle,
    status: &Arc<Mutex<Value>>,
    port: &str,
    interval: u64,
    offset: i16,
    rx: &mpsc::Receiver<Command>,
    pending: &Mutex<BTreeSet<(String, String)>>,
    last: &mut Value,
) -> Result<(), String> {
    let mut link = Link::open(port)?;
    let welcome = link.request(
        "hello",
        json!({"supportedProtocolVersions":[1],
        "appVersion":env!("CARGO_PKG_VERSION"),"clientNonce":uuid::Uuid::new_v4()}),
    )?;
    if welcome["profileId"] != "tdisplay-c5-v1" {
        return Err("设备屏幕规格不匹配。".into());
    }
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
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Command::Page(page, reply)) => {
                let value =
                    link.request("page.set", json!({"configRevision":revision,"pageId":page}));
                if let Ok(data) = &value {
                    link.status["activePageId"] = data["activePageId"].clone();
                }
                let _ = reply.send(value.map(|_| ()));
            }
            Ok(Command::Setting(key, value, reply)) => {
                let result = if supports_setting(key) {
                    link.request("settings.set", json!({(key):value})).and_then(|data| {
                        if data[key] != value {
                            return Err("设备未确认设置值。".into());
                        }
                        link.status[key] = value;
                        Ok(())
                    })
                } else {
                    Err("当前固件不支持此屏幕设置。".into())
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
            "transport":"usb","port":port,"firmwareVersion":welcome["firmwareVersion"],
            "deviceId":welcome["deviceId"], "activePageId":link.status["activePageId"],
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
