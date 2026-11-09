# TDisplay Studio

T-Display-C5 的桌面布局编辑器。编辑器预览和独立虚拟屏均按 320×170 原尺寸渲染，包含 4×2 网格和固定的底部 Status Bar。界面使用 React 和 ReUI，模拟设备状态与布局校验由 Rust 后端管理。

通用按钮和输入框取自 [ReUI v1](https://v1.reui.io/docs)，源码保存在 `src/components/ui`。

## 启动

准备 Node.js 22.18 或以上、Rust 和平台所需的 [Tauri 2 构建工具](https://v2.tauri.app/start/prerequisites/)。在项目目录运行：

```sh
npm ci
npm run tauri dev
```

点击「虚拟屏」。在编辑器中把「重置卡」拖到右下角的空格，再点「同步」。虚拟屏收到同步后移动卡片。也可以用方向键移动已聚焦的卡片。

左侧组件库按来源分组，每个组件占一格（1 × 1），缩略图复用卡片的样式和当前读数。点击未放置的组件可添加到空格，也可拖动缩略图到指定位置。

点卡片右上角的「×」即可从草稿移除，点击同步后才会更新虚拟屏。

虚拟屏关闭后可以重新打开，并恢复本次 App 运行期间最近同步的布局。Status Bar 显示连接图标、最近一次布局同步时间和供电状态。默认模拟直接供电，使用 `TDISPLAY_SIM_BATTERY_PERCENT=67 npm run tauri dev` 可预览电池模式。

## 连接 Codex

1. 在左侧点「设置」，或点 Codex 分组旁的连接状态。
2. 点「连接 Codex」，App 会自动查找本机 CLI。如需指定可执行文件，先展开「CLI 路径」并填入绝对路径。首次使用需先安装 [Codex CLI](https://developers.openai.com/codex/cli)。
3. 已用 ChatGPT 登录时会直接开始读取。需要登录时，点「浏览器登录」，在 OpenAI 官方页面完成后 App 会继续读取。也可以在终端运行 `codex login`，再回到设置点「重新检测」。
4. 设置中能看到实际返回的额度窗口、可用重置卡和速度样本。设备上的额度卡取剩余比例最低的窗口。数值会自动更新，布局由「同步」提交。

采集启用后每两分钟读一次账号额度，每 15 秒检查一次本地会话日志。速度取最近七天内最多十个可估算的模型响应，以输出 token 总数除以这些响应的总耗时。「暂停采集」会清空显示值并保存停用设置，重新打开 App 后仍保持停用。

- 「额度恢复倒计时」跟随剩余比例最低的额度窗口，到期后显示「待刷新」。
- 「任务状态」优先显示最近开始且仍在运行的本机 Codex 对话，状态来自回合日志，标题来自会话索引。标题超宽时单行循环滚动。未结束的回合三分钟没有更新时显示「状态未知」。

## 实体屏固件设计

### Flash 与 USB 更新

T-Display-C5 V1.0 采用单固件槽，通过 USB 烧录和更新。字库和设备配置分别放在独立分区。下表是规划中的 16 MiB Flash 布局。

| 区域 | 起始地址 | 容量 | 用途 |
| --- | --- | ---: | --- |
| 系统区 | `0x000000` | 64 KiB | 启动程序、分区表和 NVS |
| 应用分区 | `0x010000` | 4.75 MiB | 单份固件，应用分区类型为 `factory` |
| 资源预留 | `0x4D0000` | 4.75 MiB | 原第二固件槽空间，供后续资源使用 |
| 字库分区 | `0x990000` | 6.25 MiB | 字库索引及 WOFF2 分片 |
| 尾部预留 | `0xFD0000` | 192 KiB | 可用于崩溃转储等系统数据 |

亮度、布局和 Wi-Fi 设置等小型配置存入 NVS。首次应用新布局时写入分区表、固件和字库。后续 USB 更新只写应用分区，以保留字库和 NVS。执行全片擦除前需备份配置，擦除后需重新写入字库。 [Espressif 默认分区表](https://github.com/espressif/arduino-esp32/blob/master/tools/partitions/default_16MB.csv)

### 字体渲染

只存一套 Maple Mono CN Regular，保留 v7 CN 的全部 22,307 个码位。字库拆成 23 个 WOFF2 分片，实测共 5.91 MiB，计入索引后字库分区约余 349 KiB。FreeType 按需读取分片，运行时选择字号，需要加粗时在栅格化前调用 `FT_Outline_Embolden()`。固件需启用 Brotli，把分片解压缓冲和字形缓存分配到 PSRAM。 [FreeType 加粗接口](https://freetype.org/freetype2/docs/reference/ft2-outline_processing.html#ft_outline_embolden) · [Espressif FreeType 组件](https://components.espressif.com/components/espressif/freetype)

电脑端已验证字库覆盖，以及同一套 Regular 在 14 px 和 28 px 的普通与模拟粗体渲染。单分片测试中 FreeType 和 Brotli 的分配峰值约为 3.15 MiB，LVGL 缓存另计。固件集成后需验证应用镜像能装进 4.75 MiB 分区，以及实板上的 PSRAM 占用、首次出字延迟和 USB 更新后的数据保留。

## 检查

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```
