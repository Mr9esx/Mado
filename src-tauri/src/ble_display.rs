use serde::Serialize;

#[derive(Serialize)]
pub struct BleDisplayDevice {
    id: String,
    name: String,
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn scan_ble_displays() -> Result<Vec<BleDisplayDevice>, String> {
    Err("此版本仅在 macOS 上支持蓝牙连接。".into())
}

#[cfg(target_os = "macos")]
pub use macos::open;

#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn scan_ble_displays() -> Result<Vec<BleDisplayDevice>, String> {
    macos::scan().await
}

#[cfg(target_os = "macos")]
mod macos {
    use super::BleDisplayDevice;
    use btleplug::{
        api::{
            Central, CentralEvent, CentralState, Characteristic, Manager as _, Peripheral as _,
            ScanFilter, ValueNotification, WriteType,
        },
        platform::{Adapter, Manager, Peripheral, PeripheralId},
    };
    use futures_util::{stream::BoxStream, StreamExt};
    use std::{collections::HashSet, future::Future, io, time::Duration};
    use tokio::{
        sync::{Mutex, OnceCell},
        time::timeout,
    };
    use tokio_util::sync::CancellationToken;
    use uuid::{uuid, Uuid};

    const SERVICE: Uuid = uuid!("4e780001-704f-4fd7-a0dc-2c0fad1e48ad");
    const RX: Uuid = uuid!("4e780002-704f-4fd7-a0dc-2c0fad1e48ad");
    const TX: Uuid = uuid!("4e780003-704f-4fd7-a0dc-2c0fad1e48ad");
    const INFO: Uuid = uuid!("4e780004-704f-4fd7-a0dc-2c0fad1e48ad");
    static ADAPTER: OnceCell<Adapter> = OnceCell::const_new();
    static SCAN: Mutex<()> = Mutex::const_new(());

    async fn adapter() -> Result<&'static Adapter, String> {
        let adapter = ADAPTER
            .get_or_try_init(|| async {
                let adapters = timeout(Duration::from_secs(5), async {
                    Manager::new().await?.adapters().await
                })
                .await
                .map_err(|_| "系统蓝牙未响应。请检查蓝牙权限，或通过 USB 配网。".to_owned())?
                .map_err(|e| e.to_string())?;
                adapters
                    .into_iter()
                    .next()
                    .ok_or_else(|| "未找到蓝牙适配器。".to_owned())
            })
            .await?;
        let state = timeout(Duration::from_secs(3), adapter.adapter_state())
            .await
            .map_err(|_| "无法读取系统蓝牙状态。".to_owned())?
            .map_err(|e| e.to_string())?;
        if state != CentralState::PoweredOn {
            return Err("请打开系统蓝牙，并允许 Mado Studio 使用蓝牙。".into());
        }
        Ok(adapter)
    }

    async fn discover(adapter: &Adapter, target: Option<&str>) -> Result<Vec<Peripheral>, String> {
        let _guard = SCAN.lock().await;
        let mut events = adapter.events().await.map_err(|e| e.to_string())?;
        adapter
            .start_scan(ScanFilter {
                services: vec![SERVICE],
            })
            .await
            .map_err(|e| e.to_string())?;
        let mut seen = HashSet::new();
        let _ = timeout(Duration::from_secs(3), async {
            while let Some(event) = events.next().await {
                if let CentralEvent::ServicesAdvertisement { id, services } = event {
                    if services.contains(&SERVICE) {
                        let found = target.is_some_and(|target| id.to_string() == target);
                        seen.insert(id);
                        if found {
                            break;
                        }
                    }
                }
            }
        })
        .await;
        adapter.stop_scan().await.map_err(|e| e.to_string())?;
        Ok(adapter
            .peripherals()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|device| seen.contains(&device.id()))
            .collect())
    }

    pub async fn scan() -> Result<Vec<BleDisplayDevice>, String> {
        let mut devices = Vec::new();
        for peripheral in discover(adapter().await?, None).await? {
            if let Some(info) = peripheral.properties().await.map_err(|e| e.to_string())? {
                let id = peripheral.id().to_string();
                let name = info
                    .advertisement_name
                    .or(info.local_name)
                    .unwrap_or_else(|| format!("Mado · {}", &id[id.len().saturating_sub(4)..]));
                devices.push(BleDisplayDevice { id, name });
            }
        }
        devices.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        Ok(devices)
    }

    async fn wait<T, E: std::fmt::Display>(
        cancel: &CancellationToken,
        duration: Duration,
        operation: impl Future<Output = Result<T, E>>,
    ) -> io::Result<T> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(io::Error::new(io::ErrorKind::ConnectionAborted, "蓝牙连接已关闭。")),
            result = timeout(duration, operation) => match result {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(error)) => Err(io::Error::other(error.to_string())),
                Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "蓝牙操作超时。")),
            }
        }
    }

    pub fn open(id: &str, cancel: CancellationToken) -> Result<BleIo, String> {
        tauri::async_runtime::block_on(async {
            let adapter = adapter().await?;
            let matches_id = |device: &Peripheral| device.id().to_string() == id;
            let cached = adapter
                .peripherals()
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(matches_id);
            // CoreBluetooth's adapter cache drops the peripheral on disconnect.
            let peripheral = match cached {
                Some(device) => device,
                None => discover(adapter, Some(id))
                    .await?
                    .into_iter()
                    .find(matches_id)
                    .ok_or("找不到已配对的 Mado，请确认设备已开机并在附近。")?,
            };
            if cancel.is_cancelled() {
                return Err("蓝牙连接已关闭。".into());
            }
            let events = adapter.events().await.map_err(|e| e.to_string())?;
            let connection = Connection(peripheral);
            let (rx, tx, info) = wait(&cancel, Duration::from_secs(12), async {
                connection.0.connect().await?;
                connection.0.discover_services().await?;
                let characteristics = connection.0.characteristics();
                let find = |uuid| {
                    characteristics
                        .iter()
                        .find(|c| c.service_uuid == SERVICE && c.uuid == uuid)
                        .cloned()
                        .ok_or(btleplug::Error::NotSupported(
                            "Mado 蓝牙服务不完整。".into(),
                        ))
                };
                Ok::<_, btleplug::Error>((find(RX)?, find(TX)?, find(INFO)?))
            })
            .await
            .map_err(|e| e.to_string())?;

            // The protected read starts system pairing before any display data is sent.
            let mtu = wait(&cancel, Duration::from_secs(45), connection.0.read(&info))
                .await
                .map_err(|e| format!("蓝牙配对未完成。请在设备上开启配对并确认验证码。{e}"))?;
            let mtu: [u8; 2] = mtu.try_into().map_err(|_| "设备返回的蓝牙分片大小无效。")?;
            let chunk = (u16::from_le_bytes(mtu) as usize)
                .saturating_sub(3)
                .clamp(20, 244);
            let notifications = wait(&cancel, Duration::from_secs(5), async {
                let notifications = connection.0.notifications().await?;
                connection.0.subscribe(&tx).await?;
                Ok::<_, btleplug::Error>(notifications)
            })
            .await
            .map_err(|e| e.to_string())?;
            Ok(BleIo {
                connection,
                rx,
                notifications,
                events,
                pending: Vec::new(),
                chunk,
                cancel,
            })
        })
    }

    struct Connection(Peripheral);

    impl Drop for Connection {
        fn drop(&mut self) {
            let peripheral = self.0.clone();
            // Drop can also run inside the async connection setup.
            tauri::async_runtime::spawn(async move {
                let _ = timeout(Duration::from_secs(3), peripheral.disconnect()).await;
            });
        }
    }

    async fn while_connected<T>(
        events: &mut BoxStream<'static, CentralEvent>,
        id: PeripheralId,
        operation: impl Future<Output = io::Result<T>>,
    ) -> io::Result<T> {
        tokio::select! {
            biased;
            _ = async {
                while let Some(event) = events.next().await {
                    if matches!(event, CentralEvent::DeviceDisconnected(peer) if peer == id) {
                        break;
                    }
                }
            } => Err(io::Error::new(io::ErrorKind::ConnectionAborted, "蓝牙已断开。")),
            result = operation => result,
        }
    }

    pub struct BleIo {
        connection: Connection,
        rx: Characteristic,
        notifications: BoxStream<'static, ValueNotification>,
        events: BoxStream<'static, CentralEvent>,
        pending: Vec<u8>,
        chunk: usize,
        cancel: CancellationToken,
    }

    impl io::Read for BleIo {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if buffer.is_empty() {
                return Ok(0);
            }
            if self.pending.is_empty() {
                let notifications = &mut self.notifications;
                self.pending = tauri::async_runtime::block_on(while_connected(
                    &mut self.events,
                    self.connection.0.id(),
                    wait(&self.cancel, Duration::from_millis(100), async {
                        loop {
                            match notifications.next().await {
                                Some(value) if value.uuid == TX && !value.value.is_empty() => {
                                    return Ok(value.value)
                                }
                                Some(_) => {}
                                None => return Err("蓝牙已断开。"),
                            }
                        }
                    }),
                ))?;
            }
            let size = self.pending.len().min(buffer.len());
            buffer[..size].copy_from_slice(&self.pending[..size]);
            self.pending.drain(..size);
            Ok(size)
        }
    }

    impl io::Write for BleIo {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let peripheral = &self.connection.0;
            let rx = &self.rx;
            let chunk_size = self.chunk;
            tauri::async_runtime::block_on(while_connected(
                &mut self.events,
                peripheral.id(),
                wait(&self.cancel, Duration::from_secs(45), async {
                    for chunk in buffer.chunks(chunk_size) {
                        timeout(
                            Duration::from_secs(5),
                            peripheral.write(rx, chunk, WriteType::WithResponse),
                        )
                        .await
                        .map_err(|_| btleplug::Error::RuntimeError("蓝牙写入超时。".into()))??;
                    }
                    Ok::<_, btleplug::Error>(buffer.len())
                }),
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
