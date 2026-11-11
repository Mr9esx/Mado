# 采集器变量与内置集成实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Execute this plan task-by-task inline. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Mado Studio 增加 App 本机加密采集器变量配置、扩展系统指标，并内置 GitHub 与 B 站开播通知采集器。

**Architecture:** 扩展现有 `Definition.settings` 元数据和实例配置 API，变量值只以密文写入 `collectors.json`，由 App 自动管理本机密钥；前端从清单生成配置表单。系统采样由 Rust 宿主提供跨平台指标，GitHub 与 B 站仍是普通 MJS 插件；B 站的状态转换通过受控桌面通知能力触发原生通知，并沿用既有状态卡提醒。

**Tech Stack:** Rust/Tauri 2、React/TypeScript、MJS QuickJS collectors、AES-GCM、sysinfo、Node test runner、Cargo tests。

## Global Constraints

- 用户变量名匹配 `[A-Z][A-Z0-9_]*`；已有小写设置字段继续兼容。
- 中文变量名、描述、必填和敏感标志来自采集器清单；代码通过 `settings.VARIABLE_NAME` 读取。
- 不使用系统凭据库，不设置 App 解锁密码；变量值由 App 自动加密落盘。
- GitHub 只请求 `api.github.com` 的 GET 接口，令牌只放 Authorization 请求头。
- B 站只查询公开开播状态；首次采样不通知，只有离线转开播通知一次。
- 系统模块名为「系统」；卡片中文标题尽量不超过四个汉字；现有模块和卡片 ID 保持有效。
- 新模块 ID `github` 与 `bilibili` 保留；内置模块仍走统一 MJS 运行器和 SDK。
- 先写失败测试再写生产代码；不把已有用户改动纳入本功能提交。

---

## 文件地图

- `src-tauri/src/collector/model.rs`：变量元数据结构、名称校验、设置生成规则、组件声明和通知能力验证。
- `src-tauri/src/collector/vault.rs`：本机随机密钥、AES-GCM 密文格式、兼容明文配置迁移。
- `src-tauri/src/collector/workspace.rs`：Catalog 加载/保存加密、配置 API、实例状态脱敏、发布前后变量校验、通知派发。
- `src-tauri/src/collector/io.rs`：跨平台系统指标与网络计数器；系统/通知宿主能力。
- `src-tauri/src/collector/builtin.rs`：注册四个内置采集器并更新 fixtures。
- `shared/collectors/system/*`：短标题与新指标组件声明/投影。
- `shared/collectors/github/*`：仓库、PR、Issue、Actions API 查询。
- `shared/collectors/bilibili/*`：直播状态查询、初始状态与开播转换。
- `src-tauri/src/main.rs`、`src-tauri/Cargo.toml`、`src-tauri/capabilities/*.json`：通知插件初始化和 capability 权限。
- `src/collectorApi.ts`、`src/CollectorWorkspace.tsx`、`src/collector.css`：按变量定义生成配置表单并保存。
- `src/CollectorCode.tsx`：变量声明与读取的代码补全。
- `tests/*`、`src-tauri/src/collector/*`：脚本行为、模型、密文、配置流程和跨平台宿主能力测试。
- `README.md`、`docs/display-config-model.md`：用户和模块作者文档。

## Task 1: 变量定义与本机加密存储

**Interfaces:** `Setting { label, default, description?, required?, secret? }`; `settings` 键是脚本变量名；`vault::KeyStore` 提供 `load_or_create(config_dir)`, `encrypt(plain)`, `decrypt(cipher)`。`Catalog` 只在内存保存明文，序列化到盘时加密已声明变量；旧的明文变量值加载时迁移。

- [ ] **Step 1: 写模型失败测试**：在 `model.rs` 测试非空 label、description、uppercase variable key、secret/required JSON round-trip、拒绝小写新变量名，同时确认旧小写设置不带变量元数据仍有效。
- [ ] **Step 2: 运行模型目标测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::model::tests::variable -- --nocapture`；预期因新字段/校验缺失失败。
- [ ] **Step 3: 实现最小变量元数据与验证**：扩展 Setting；只有含变量元数据的新定义才要求 uppercase key；限制说明文本长度；fixture/mock `io::settings` 将变量名和值作为 JS 对象属性。
- [ ] **Step 4: 写 vault 行为测试**：断言随机 nonce 下同一明文生成不同密文、密文可还原、篡改/错误 key 解密失败、已有明文值能迁移；测试临时目录真实文件行为。
- [ ] **Step 5: 运行 vault 目标测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::vault -- --nocapture`。
- [ ] **Step 6: 实现本机 AES-256-GCM vault**：使用 OS CSPRNG 生成 32-byte key 和 96-bit nonce；密文含版本前缀；将 key 写入 App 配置目录独立文件并限制 Unix 权限；发现已存在密文但 key 缺失/损坏时返回清楚错误，不重置 key。
- [ ] **Step 7: 写 Catalog 加密/脱敏测试**：保存含变量的 catalog，检查原变量字符串不出现在磁盘字节与 `instance_status` JSON，重载后采集可得到原值；迁移明文并再次启动仍有效；去掉变量后清理密文。
- [ ] **Step 8: 运行集成测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::workspace::tests::encrypted_variables -- --nocapture`。
- [ ] **Step 9: 实现 Catalog 持久化边界**：按模块 release 的 `settings` 定义仅加密变量键；路径/程序授权保持当前工作方式；旧变量明文在 load 时解密并在下一次保存迁移；错误、日志和状态响应不得回显敏感值。
- [ ] **Step 10: 运行 Rust 定向测试**：运行 `cargo test --manifest-path src-tauri/Cargo.toml collector::model` 和 `collector::workspace::tests::encrypted_variables`，全部通过。

## Task 2: 自动生成的变量配置表单

**Consumes:** Task 1 的 Setting 元数据、加密持久化与不回显状态 API。

**Produces:** `get_collector_variables(id) -> { fields, configured }`、`save_collector_variables(id, values) -> status`。

- [ ] **Step 1: 写 Rust 配置流程测试**：缺必填变量时拒绝启用/采集配置；配置合法值后已保存，敏感值只返回 configured 布尔值；空值可清除可选变量；未知键被拒绝。
- [ ] **Step 2: 运行目标测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::workspace::tests::configure_variables -- --nocapture`。
- [ ] **Step 3: 实现变量读取/保存 Tauri commands**：通过模块 ID 找到已发布 Definition；验证 keys 与 required；写入实例内存 settings 后复用 Catalog vault 保存，触发该实例立即刷新并发布新状态。
- [ ] **Step 4: 写前端表单行为测试**：使用已配置、未配置、必填、可选敏感字段的 definition，验证中文名/说明显示、密码字段遮蔽、未填必填项不可保存、敏感值不回显、清除按钮能清除可选变量。
- [ ] **Step 5: 运行前端定向测试确认失败**：通过 `npm test -- tests/collector-variables.test.mjs`（若 JSX 需浏览器则使用现有 fixture/Vite 测试模式）。
- [ ] **Step 6: 实现工作区入口与变量表单**：在选中的已发布模块页显示「配置变量」；字段按清单顺序生成；成功保存刷新 catalog；错误使用现有工作区反馈区域；不影响源码编辑与模块发布操作。
- [ ] **Step 7: 增加 CodeMirror 补全**：补全 uppercase settings keys 和 `settings.KEY` 属性读取；完整旧模块行为保留。
- [ ] **Step 8: 运行构建和定向测试**：`npm test -- tests/collector-variables.test.mjs`、`npm run build`、变量 Tauri tests。

## Task 3: 系统指标扩展

**Interfaces:** `context.system.read('system')` 增加 `memoryAvailableBytes`、`systemDiskUsedPercent`、`networkReceivedBytes`、`networkTransmittedBytes`；采样序列由宿主维护，MJS 负责转成短期速率。CPU 当前数值与现有 CPU 历史输出兼容。

- [ ] **Step 1: 写系统指标投影测试**：CPU 当前值、内存 GB 详情、系统卷比例、上/下行速率、首次采样和计数器回退均用手算 fixture 验证。
- [ ] **Step 2: 运行目标测试确认失败**：`node --test tests/system-collector.test.mjs`。
- [ ] **Step 3: 实现 MJS 输出与清单**：模块名改「系统」；新增 CPU 数值、系统盘进度、上传/下载速率和内存已用/总量 details；旧 `system.*` widget ID 保持。
- [ ] **Step 4: 写 Rust 系统读取测试**：固定网络计数与磁盘容量输入，通过可注入的 snapshot helper 验证 API 投影；生产 sampler 在目标 OS 下读取 sysinfo 的系统卷和网络数据。
- [ ] **Step 5: 运行目标测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::io::tests::system -- --nocapture`。
- [ ] **Step 6: 扩展 sysinfo sampler**：刷新系统卷和网络计数器，过滤 loopback 接口，合计其余接口；计数器使用饱和处理；首次无增量信息由 MJS 返回 null。
- [ ] **Step 7: 运行系统采集器与 Rust 测试**：`node --test tests/system-collector.test.mjs` 与 system 读取测试。

## Task 4: GitHub 内置采集器

**Interfaces:** module `github`, settings `GITHUB_REPOSITORY` required and `GITHUB_TOKEN` optional secret; outputs open PR count, open Issue count, latest Actions state; widgets `PR数`, `Issue`, `CI状态`。

- [ ] **Step 1: 写 GitHub fixture tests**：mock 完整的 REST 响应，确认 repo parsing、Issue 排除 PR、Actions 状态 mapping、Authorization Bearer header、空 token 不带 header、API error/limit returns error rather than zero。
- [ ] **Step 2: 运行目标测试确认失败**：`node --test tests/github-collector.test.mjs`。
- [ ] **Step 3: 实现 GitHub collector 和 manifest**：三路 GET 绑定同一 host，采样周期 60 秒且初始停用；不记录 token；分别输出 metric 和 state。
- [ ] **Step 4: 注册并验证 built-in module**：将模块装配到 `builtin::projects()`，加入保留 ID、fixtures、布局目录和作者工作区目录；测试三项 widget 都进入 runtime catalog。
- [ ] **Step 5: 运行 GitHub 与内置目录测试**：`node --test tests/github-collector.test.mjs`、`cargo test --manifest-path src-tauri/Cargo.toml collector::builtin`。

## Task 5: B 站开播状态与桌面通知

**Interfaces:** module `bilibili`, required `BILIBILI_UID`, output state `live` with title/URL details; offline-to-live requests state notification and native desktop notification capability.

- [ ] **Step 1: 写 B 站状态机测试**：完整响应 fixture 覆盖首次已开播不通知、离线保持不通知、离线转开播通知一次、开播持续不重复通知、错误采样保留状态、回到离线后再次开播可再次通知。
- [ ] **Step 2: 运行目标测试确认失败**：`node --test tests/bilibili-collector.test.mjs`。
- [ ] **Step 3: 实现 B 站 MJS collector**：读取 `BILIBILI_UID`，请求公开房间信息接口，校验 code/liveStatus/room ID，缓存上次状态，生成 title 和房间链接；设置周期 30 秒。
- [ ] **Step 4: 写宿主通知 capability 测试**：collector 声明通知能力时可派发仅限安全长度 title/body 的桌面通知；无能力不可派发；状态卡通知仍是独立的一次性帧行为。
- [ ] **Step 5: 运行通知测试确认失败**：`cargo test --manifest-path src-tauri/Cargo.toml collector::workspace::tests::desktop_notification -- --nocapture`。
- [ ] **Step 6: 实现 Tauri 通知集成**：添加 notification plugin 和 capability；将一次性 notification 请求限定为声明的模块输出键，发出跨平台通知；权限拒绝或 Linux notification service 缺失时保留采集读数并记录非敏感状态。
- [ ] **Step 7: 注册 B 站内置模块**：加入目录、保留 ID 和 fixture；配置表单显示主播 UID。
- [ ] **Step 8: 运行 B 站与通知测试**：`node --test tests/bilibili-collector.test.mjs` 和相应 Cargo tests。

## Task 6: 文档、完整验证与跨平台边界

- [ ] **Step 1: 更新模块作者文档**：解释变量键名规则、label/description/required/secret、`settings.KEY`、默认值、离线 fixture、安全存储及本机密钥文件的保护边界。
- [ ] **Step 2: 更新用户文档**：列出系统卡、GitHub 配置、B 站 UID 与通知行为；注明 B 站公开接口可能变更。
- [ ] **Step 3: 检查差异**：运行 `git diff --check` 并确认以前已存在的未提交改动未被重置或暂存。
- [ ] **Step 4: 运行全部测试与构建**：`npm test`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- [ ] **Step 5: 构建 Tauri App**：运行 macOS 本机 release build；检查 Windows/Linux 可交叉编译条件并在代码可执行的 target 下运行相应 build/tests。
- [ ] **Step 6: 验收交互**：打开 App，验证 GitHub/UID 表单保存、遮蔽与清除，系统指标卡预览，B 站 fixture 通知与禁用行为。
