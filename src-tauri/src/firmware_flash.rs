use crate::display_connection::{esp_usb_ports, DisplayConnection};
use aws_lc_rs::digest::{digest, SHA256};
use esp_idf_part::{AppType, DataType, Flags, Partition, PartitionTable, SubType, Type};
use espflash::{
    connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
    flasher::{FlashSize, Flasher},
    image_format::Segment,
    target::{Chip, ProgressCallbacks},
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use tauri::{ipc::Channel, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;
use zip::ZipArchive;

const BOARD: &str = "LILYGO T-Display-C5 V1.0";
const BOOTLOADER_OFFSET: u32 = 0x2000;
const TABLE_OFFSET: u32 = 0x8000;
const TABLE_SIZE: usize = 0xC00;
const APP_OFFSET: u32 = 0x10000;
const APP_SIZE: usize = 0x4C0000;
const FONTS_OFFSET: u32 = 0x990000;
const FONTS_SIZE: usize = 0x640000;
const MAX_PACKAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum InstallMode {
    Install,
    Upgrade,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    board: String,
    chip: String,
    version: String,
    mode: InstallMode,
    images: Vec<Image>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Image {
    file: String,
    offset: u32,
    sha256: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    id: String,
    file_name: String,
    version: String,
    mode: InstallMode,
}

struct Package {
    info: PackageInfo,
    segments: Vec<Segment<'static>>,
}

#[derive(Default)]
pub struct FlashSelection(Mutex<Option<Arc<Package>>>);

fn read_entry(archive: &mut ZipArchive<File>, name: &str, limit: usize) -> Result<Vec<u8>, String> {
    let file = archive
        .by_name(name)
        .map_err(|_| format!("无法读取烧录包中的 {name}。"))?;
    if !file.is_file() || file.size() == 0 || file.size() > limit as u64 {
        return Err(format!("{name} 的大小不符合要求。"));
    }
    let size = file.size() as usize;
    let mut bytes = Vec::with_capacity(size);
    file.take(size as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("无法解压 {name}。"))?;
    if bytes.len() != size {
        return Err(format!("{name} 的大小与 ZIP 记录不一致。"));
    }
    Ok(bytes)
}

fn check_partition_table(bytes: &[u8]) -> Result<(), String> {
    // Match firmware/partitions.csv. Construct the custom data subtype directly
    // because esp-idf-part's CSV reader only accepts built-in data subtypes.
    let flags = Flags::empty();
    let nvs = SubType::Data(DataType::Nvs);
    let expected = PartitionTable::new(vec![
        Partition::new("nvs", Type::Data, nvs, 0x9000, 0x6000, flags),
        Partition::new(
            "factory",
            Type::App,
            SubType::App(AppType::Factory),
            APP_OFFSET,
            APP_SIZE as u32,
            flags,
        ),
        Partition::new("displaycfg", Type::Data, nvs, 0x4D0000, 0x40000, flags),
        Partition::new(
            "fonts",
            Type::Data,
            SubType::Custom(0x40),
            FONTS_OFFSET,
            FONTS_SIZE as u32,
            flags,
        ),
    ])
    .to_bin()
    .map_err(|error| format!("无法读取 Mado 分区定义：{error}"))?;
    if bytes != expected {
        return Err("分区表与 Mado 的布局不一致。".into());
    }
    Ok(())
}

fn check_image(offset: u32, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() % 4 != 0 {
        return Err("镜像长度必须为 4 字节的倍数。".into());
    }
    match offset {
        BOOTLOADER_OFFSET | APP_OFFSET => {
            if bytes.len() < 24
                || bytes[0] != 0xe9
                || u16::from_le_bytes([bytes[12], bytes[13]]) != Chip::Esp32c5.id()
            {
                return Err("镜像不是有效的 ESP32-C5 固件。".into());
            }
            let is_app = bytes.get(32..36) == Some(&[0x32, 0x54, 0xcd, 0xab]);
            if is_app != (offset == APP_OFFSET) {
                return Err("应用与启动程序镜像的写入地址不匹配。".into());
            }
        }
        TABLE_OFFSET => check_partition_table(bytes)?,
        FONTS_OFFSET => {
            if bytes.len() < 16
                || !matches!(&bytes[..4], b"TDF1" | b"TDF2")
                || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize != bytes.len()
            {
                return Err("字库镜像的文件头或长度无效。".into());
            }
        }
        _ => return Err("镜像地址不受支持。".into()),
    }
    Ok(())
}

fn load_package(path: &Path) -> Result<Package, String> {
    let file = File::open(path).map_err(|_| "无法打开所选烧录包。")?;
    let metadata = file.metadata().map_err(|_| "无法读取烧录包的大小。")?;
    if !metadata.is_file() || metadata.len() > MAX_PACKAGE_BYTES {
        return Err("请选择不超过 16 MiB 的 ZIP 烧录包。".into());
    }
    let mut archive = ZipArchive::new(file).map_err(|_| "烧录包不是有效的 ZIP 文件。")?;
    let manifest: Manifest =
        serde_json::from_slice(&read_entry(&mut archive, "manifest.json", 4096)?)
            .map_err(|error| format!("烧录包清单格式无效：{error}"))?;
    if manifest.schema_version != 1 || manifest.board != BOARD || manifest.chip != "esp32c5" {
        return Err("烧录包必须适用于 LILYGO T-Display-C5 V1.0（ESP32-C5）。".into());
    }
    if manifest.version.trim().is_empty()
        || manifest.version.len() > 64
        || manifest.version.chars().any(char::is_control)
    {
        return Err("烧录包的版本字段无效。".into());
    }
    let required: &[(u32, usize)] = match manifest.mode {
        InstallMode::Install => &[
            (
                BOOTLOADER_OFFSET,
                (TABLE_OFFSET - BOOTLOADER_OFFSET) as usize,
            ),
            (TABLE_OFFSET, TABLE_SIZE),
            (APP_OFFSET, APP_SIZE),
            (FONTS_OFFSET, FONTS_SIZE),
        ],
        InstallMode::Upgrade => &[(APP_OFFSET, APP_SIZE)],
    };
    if manifest.images.len() != required.len() || archive.len() != required.len() + 1 {
        return Err("烧录包的文件数量与烧录方式不符。".into());
    }
    let mut names = std::collections::BTreeSet::new();
    let mut segments = Vec::with_capacity(required.len());
    for &(addr, limit) in required {
        let image = manifest
            .images
            .iter()
            .find(|image| image.offset == addr)
            .ok_or_else(|| format!("烧录包缺少地址 0x{addr:X} 的镜像。"))?;
        if !image.file.ends_with(".bin")
            || image.file.len() > 100
            || !image
                .file
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
            || !names.insert(&image.file)
        {
            return Err("每个镜像必须在 ZIP 根目录使用不同的 .bin 文件名。".into());
        }
        let bytes = read_entry(&mut archive, &image.file, limit)?;
        let hash: String = digest(&SHA256, &bytes)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !image.sha256.eq_ignore_ascii_case(&hash) {
            return Err(format!("{} 的 SHA-256 校验失败。", image.file));
        }
        check_image(addr, &bytes).map_err(|error| format!("{}：{error}", image.file))?;
        // espflash pads uncompressed writes to a full stub block. Narrow regions
        // must use compressed writes so the padding cannot reach the next partition.
        if bytes
            .len()
            .next_multiple_of(Chip::Esp32c5.stub_flash_write_size())
            > limit
        {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
            encoder
                .write_all(&bytes)
                .map_err(|error| error.to_string())?;
            let compressed = encoder.finish().map_err(|error| error.to_string())?;
            if compressed.len() >= bytes.len() {
                return Err(format!("{} 无法在对应的 Flash 区域内写入。", image.file));
            }
        }
        segments.push(Segment {
            addr,
            data: Cow::Owned(bytes),
        });
    }
    Ok(Package {
        info: PackageInfo {
            id: Uuid::new_v4().to_string(),
            file_name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            version: manifest.version,
            mode: manifest.mode,
        },
        segments,
    })
}

#[tauri::command]
pub async fn choose_firmware_package(window: WebviewWindow) -> Result<Option<PackageInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .add_filter("Mado 烧录包", &["zip"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|_| "无法读取所选文件路径。")?;
        let package = load_package(&path)?;
        let info = package.info.clone();
        *window
            .state::<FlashSelection>()
            .0
            .lock()
            .map_err(|_| "烧录包选择不可用。")? = Some(Arc::new(package));
        Ok(Some(info))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashProgress {
    phase: &'static str,
    percent: u8,
}

struct Progress<'a> {
    channel: Channel<FlashProgress>,
    segments: &'a [Segment<'static>],
    total_bytes: usize,
    completed_bytes: usize,
    current_bytes: usize,
    chunks: usize,
    percent: u8,
}

impl Progress<'_> {
    fn report(&self, phase: &'static str) {
        let _ = self.channel.send(FlashProgress {
            phase,
            percent: self.percent,
        });
    }
}

impl ProgressCallbacks for Progress<'_> {
    fn init(&mut self, addr: u32, total: usize) {
        self.current_bytes = self
            .segments
            .iter()
            .find(|segment| segment.addr == addr)
            .map_or(0, |segment| segment.data.len());
        self.chunks = total;
        self.report("writing");
    }

    fn update(&mut self, current: usize) {
        let written = self.completed_bytes + self.current_bytes * current / self.chunks.max(1);
        let percent = (written * 99 / self.total_bytes).min(99) as u8;
        if percent > self.percent {
            self.percent = percent;
            self.report("writing");
        }
    }

    fn verifying(&mut self) {
        self.report("verifying");
    }

    fn finish(&mut self, _skipped: bool) {
        self.completed_bytes += self.current_bytes;
        if self.completed_bytes == self.total_bytes {
            self.report("restarting");
        }
    }
}

fn write_package(
    port: &str,
    usb_info: serialport::UsbPortInfo,
    package: &Package,
    manual_boot: bool,
    channel: Channel<FlashProgress>,
) -> Result<(), String> {
    let segments = &package.segments;
    let mut progress = Progress {
        channel,
        segments,
        total_bytes: segments.iter().map(|segment| segment.data.len()).sum(),
        completed_bytes: 0,
        current_bytes: 0,
        chunks: 0,
        percent: 0,
    };
    progress.report("connecting");
    let serial = serialport::new(port, 115_200)
        .timeout(std::time::Duration::from_secs(3))
        .open_native()
        .map_err(|error| format!("无法打开 USB 串口：{error}"))?;
    let before = if manual_boot {
        ResetBeforeOperation::NoReset
    } else {
        ResetBeforeOperation::UsbReset
    };
    let connection = Connection::new(
        serial,
        usb_info,
        ResetAfterOperation::WatchdogReset,
        before,
        115_200,
    );
    let mut flasher = Flasher::connect(connection, true, true, false, Some(Chip::Esp32c5), None)
        .map_err(|error| format!("无法连接 ESP32-C5 下载模式：{error}"))?;
    if flasher.secure_download_mode() {
        return Err("设备处于受限下载模式，无法校验写入结果。".into());
    }
    let size = flasher
        .device_info()
        .map_err(|error| format!("无法读取设备信息：{error}"))?
        .flash_size;
    if size != FlashSize::_16Mb {
        return Err("设备的 Flash 容量与目标板卡不符（要求 16 MiB）。".into());
    }
    if matches!(package.info.mode, InstallMode::Upgrade) {
        progress.report("checking");
        let table =
            tempfile::NamedTempFile::new().map_err(|error| format!("无法创建临时文件：{error}"))?;
        flasher
            .read_flash(
                TABLE_OFFSET,
                TABLE_SIZE as u32,
                1024,
                1,
                table.path().into(),
            )
            .map_err(|error| format!("无法读取设备分区表：{error}"))?;
        let bytes =
            std::fs::read(table.path()).map_err(|error| format!("无法读取分区表：{error}"))?;
        check_partition_table(&bytes)
            .map_err(|_| "设备分区表与 Mado 不兼容。请使用首次安装包。")?;
    }
    flasher
        .write_bins_to_flash(segments, &mut progress)
        .map_err(|error| format!("烧录未完成：{error}"))?;
    Ok(())
}

#[tauri::command]
pub async fn flash_firmware_package(
    window: WebviewWindow,
    package_id: String,
    port: String,
    manual_boot: bool,
    progress: Channel<FlashProgress>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let package = window
            .state::<FlashSelection>()
            .0
            .lock()
            .map_err(|_| "烧录包选择不可用。")?
            .as_ref()
            .filter(|package| package.info.id == package_id)
            .cloned()
            .ok_or("请重新选择烧录包。")?;
        let (_, info) = esp_usb_ports()
            .find(|(name, _)| name == &port)
            .ok_or("未找到选中的 USB 设备，请刷新端口。")?;
        window
            .state::<DisplayConnection>()
            .with_firmware_port(|| write_package(&port, info, &package, manual_boot, progress))
    })
    .await
    .map_err(|error| error.to_string())?
}
