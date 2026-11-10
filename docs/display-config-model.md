# TDisplay 屏幕配置数据模型

本稿定义 TDisplay Studio 的采集模块 SDK、屏幕配置和设备协议。采集模块声明可输出的数据，组件定义声明怎么展示，卡片绑定已发布采集器的输出。Studio 把采样转换为按 `cardId` 索引的读数，发给桌面控件和实体屏。新建采集器使用 SDK v2，已有 SDK v1 模块继续运行。

桌面控件是独立于硬件的本机输出：读取同一份已应用布局和采集数据，不等待设备连接或设备确认。App 中「应用布局」更新本机布局；下文涉及实体设备确认、传输、供电和固件的字段仍属于硬件协议。桌面控件状态栏使用 App 版本与本机标识。当前本机快照以 `pages: {id, name, layout}[]`、`activePageId` 和 `rotationSeconds` 表示多页，`layout` 为当前页的兼容字段；应用操作校验并持久化全部页面，翻页仅更新当前页和帧序号，不增加配置版本。轮播默认关闭，可选 5/10/15/30/60 秒。相同组件允许放入不同页，单页仍不可重复；全部页面参与采集器引用保护。已应用配置保存在应用配置目录的 `display-pages.json`，读取数值不落盘。

## 数据分层

| 层 | 结构 | 职责 |
| --- | --- | --- |
| 模块 | `ModuleManifest`、草稿、发布版本 | 入口脚本的 `defineModule()` 声明参数、权限、输出和组件。发布版本保存经过试跑的定义和脚本快照，导出时生成 `manifest.json`。 |
| 采集 | `SourceInstance`、`OutputSample` | 发布时自动注册内部数据源，运行记录保存授权和调度状态。样本按 `(sourceInstanceId, outputKey)` 隔离。 |
| 屏幕编辑 | `WidgetDefinition`、`DisplayConfig` | 组件定义选择模块内的输出及呈现方式。屏幕配置选择页面、卡片、已发布的数据源和样式。 |
| 屏幕运行 | `DeviceDisplayConfig`、`ReadingFrame` | Studio 解析卡片并生成设备渲染配置。读数按 `cardId` 发送，设备按 `configRevision` 对齐配置。 |

`moduleId` 标识采集模块，`outputKey` 标识模块内的一路数据，`widgetId` 标识一种展示方式。`sourceInstanceId` 标识一次具体配置，`cardId` 标识屏幕上的一次放置。例如，一个 CPU 模块可以配置两台主机，每台主机的同一路 `cpuUsage` 可用于图表、趋势和区间三张卡。

## 用户操作与存储字段

| 步骤 | 用户填写或选择 | Studio 自动记录 |
| --- | --- | --- |
| 新建采集器 | 显示名称、模板 | 从模板写入 `sdkApiVersion` 和 `entry`，生成 `moduleId`。 |
| 编写与发布 | 编辑入口脚本，按需在试跑区填写样例数据 | 保存草稿，按文件树更新入口。发布时分配 `moduleRevision` 并冻结快照。 |
| 发布并启用 | 在入口脚本声明参数、资源和刷新周期。试跑或发布时确认需要的新授权。 | 自动注册数据源与卡片，保存稳定的 `sourceInstanceId` 和已授权的资源。 |
| 布局与同步 | 选择页面和数据源卡片，拖动、改规格、设置主题和样式 | 生成 `pageId`、`cardId`，记录组件和实例引用、格子坐标及尺寸。按屏幕规格填入 `profileId`，写入 `schemaVersion`，管理 `baseRevision`、`configRevision` 和帧 `sequence`。 |

## SDK

### 模块清单

在入口脚本的 `defineModule({ …, collect })` 中编写定义和采集逻辑。试跑时，Studio 在独立运行器中读取定义，校验权限和输出契约。导出时，Studio 合并自己管理的字段并生成 `manifest.json`。

| `ModuleManifest` 字段 | 由谁管理 | 规则 |
| --- | --- | --- |
| `name` | 作者 | 模块显示名称，允许改名。 |
| `settings` | 作者 | 可选。普通文本参数，每项有 `label` 和可选的 `default`。 |
| `permissions` | 作者 | 可选。按别名声明宿主资源与允许的操作，在试跑或发布时确认实际资源和授权范围。 |
| `resources` | 作者 | 可选。以资源别名为键，填写 HTTP 地址或已知的本机路径。新项目在这里填写 HTTP 地址。省略本机路径时，授权流程会请用户选择资源。 |
| `schedule` | 作者 | 新项目使用 `{ refreshSeconds, enabled }`。采集间隔为 5–3600 秒，设 `enabled: false` 后发布可暂停采集。 |
| `outputs` | 作者 | 以 `outputKey` 为索引，至少声明一路。每路声明 `valueType`，按需声明单位、状态键和辅助字段。 |
| `widgets` | 作者 | 以模块内组件键为索引。`widgetKey` 由小写字母、数字和连字符组成。一个输出可供多个组件定义引用。Studio 用 `${moduleId}.${widgetKey}` 生成全局 `widgetId`。 |
| `moduleId` | Studio | 新建时生成 UUID。改名、编辑和发布新版均沿用原 ID。随包内置模块和外部工具编写的模块可以使用预先分配的 ID，支持用点分隔命名空间。 |
| `sdkApiVersion` | Studio | 新建时随模板写入 `2`，用于选择清单、脚本入口和返回值契约。编辑和发布沿用原值。导入按完整清单的版本校验，缺少版本或版本超出支持范围时给出错误。 |
| `entry` | Studio | 模板默认指向 `src/index.mjs`。用户可在文件树里选择其他项目内的 JavaScript ESM 文件作为入口。Studio 保存相对路径，重命名时同步更新。 |

Studio 在新建时为草稿保留 ID，并在注册或发布时原子检查唯一性。不同模块可以同名。导入时，Studio 根据清单中的 ID 选择处理方式。

- ID 未使用时，沿用该 ID 创建草稿。
- ID 已存在且与现有草稿或已发布版本内容一致时，提示已有相同内容，并打开对应版本。
- ID 已存在但内容不同时，可以导入为更新草稿，或者「作为副本导入」，由 Studio 分配新 ID。若已有编辑中的草稿，先提示处理该草稿。更新在测试和发布后才会生效，沿用原 SDK 版本。要使用不同 SDK 版本，请作为副本导入。内置模块的保留 ID 禁止通过导入更新。

SDK v1 的 `inputs` 保留兼容。Studio 按已有绑定或本次授权预读取 `httpJson` 或 `fileJson`，再把 JSON 传给旧脚本的 `collect(inputs, context)`。

发布时，Studio 自动建立内部运行记录。作者在 `defineModule()` 中提供普通参数默认值和采集计划。已有实例的 ID 和布局绑定会沿用。旧项目省略 `schedule` 时继续使用此前保存的参数和周期，新增 `schedule` 并发布即可采用项目里的参数与采集计划。

### 输出契约

采集器按输出键返回值。`valueType` 描述采样数据的形状，组件的 `kind` 描述显示方式。数值须为有限数，时间戳统一为 Unix 毫秒。

| `valueType` | 输出声明 | 非空采样值 | 可绑定的卡片 |
| --- | --- | --- | --- |
| `number` | 可声明 `unit`、`detailKeys` | `{ value, bounds?, details? }`。`bounds` 同时提供 `min` 和 `max`，且 `min < max`。 | `metric` 数值、`chart`、`trend`、`range` |
| `deadline` | 可声明 `detailKeys` | `{ deadlineUnixMs, details? }` | `metric` 倒计时 |
| `progress` | 可声明 `unit`、`detailKeys` | `{ value, ratio, details? }`。`ratio` 为 0–1 或 `null`。 | `progress` |
| `state` | 声明非空且去重的 `stateKeys`，可声明 `detailKeys` | `{ state, detail?, details? }`。`state` 必须来自 `stateKeys`。 | `state` |

`detailKeys` 声明输出可以提供的辅助字段键。采样中的 `details` 是以这些键为索引的字符串字典。组件从中选择最多两项，并设置展示顺序和标签。`null` 表示这次没有该项读数，数字 `0` 是有效值。

### 脚本 API

| API | 行为 |
| --- | --- |
| `defineModule({ …, collect })` | 从 `@tdisplay/sdk` 导入，作为入口脚本的默认导出。名称、参数、授权、输出和采集函数在同一个对象中定义。 |
| `collect(settings, context)` | 每次试跑或正式采集时调用。脚本自行发起 I/O，返回 `{ outputs, notifications? }` 或相应的 Promise。 |
| `context.sampledAtUnixMs` | Studio 为本次采集分配的时间戳。该批输出使用同一时间戳。 |
| `context.log(message)` | 写入本次执行的有限长度调试日志。日志只显示在 Studio。 |

`outputs` 必须覆盖清单声明的全部输出键，缺值用 `null`。Studio 校验整批数据后提交，校验失败时整批丢弃，并在实例状态中记录失败原因。每路 `OutputSample` 保存 `sourceInstanceId`、`outputKey`、`sampledAtUnixMs` 和 `value`。采集失败时，最近成功值可保留到有效期结束。定时实例默认有效期为三个刷新周期。暂停采集或读数过期后，该实例的卡片显示空态。历史样本由 Studio 按所需图表窗口和趋势周期保留。

`notifications` 是本次请求提醒的状态输出键列表。每个键必须对应本批非空的 `state` 输出。提醒请求独立于可缓存的样本，Studio 只将它投递到本次更新中绑定该输出且允许提醒的卡片。下面的例子假设模块已声明 `task` 为状态输出，并把 `completed` 列入 `stateKeys`。

```js
import { defineModule } from '@tdisplay/sdk'

export default defineModule({
  collect() {
    return {
      outputs: { task: { state: 'completed', detail: '固件构建完成' } },
      notifications: ['task'],
    }
  },
})
```

正式采集应在收到需要提醒的事件时才返回 `notifications`。普通轮询可以持续返回同一个 `completed` 状态。

### 从草稿到数据源

1. 在「数据源 → 新建采集器」选模板、填写显示名称并打开 Micro IDE。模板创建一个 `src/index.mjs`，部分模板还预填离线样例。文件树列出实际的项目文件。CodeMirror 提供 JS 和 JSON 高亮。输入 SDK 字段、已声明的输入输出键或相对导入路径时，按 `Tab` 接受候选项。
2. 在文件树中新建、删除或重命名项目文件。Studio 自动保存草稿。重命名入口时更新 `entry`，删除入口前先选择替代文件。缺失的导入会在检查或试跑时定位到引用处，修复后才可发布。
3. 点「测试运行」。在试跑区的「样例数据」中可以按需填写离线响应，也可以用实际资源试跑。项目内显式创建的 `fixtures/sample.json` 优先用于离线试跑。测试结果包含日志、错误位置、读数和卡片预览，写入预览数据。测试使用和正式采集相同的运行器及 SDK 校验。
4. 点「发布」。Studio 校验测试结果对应当前文件快照，保存新的 `moduleRevision`，将该版本注册到模块目录。有新增或变化的资源时会先确认授权。发布成功后采集器自动注册，用户可以直接在布局中选择其卡片。
5. 可以导入或导出模块目录。目录包含清单、入口和项目内依赖。导入时先校验清单和文件结构，再以草稿打开。导出包含入口脚本和实际项目文件，Studio 生成模块清单。试跑面板中的样例、授权与本机选择的路径留在本机。打开旧项目时，Studio 创建引用原脚本的新入口，保留原有文件及屏幕绑定。内置模块从同一套清单注册。

QuickJS 在 Studio 随包提供的独立运行器内执行。模块加载器解析项目快照中的相对导入和 `@tdisplay/sdk`。兼容的纯 JS 库可以作为项目文件一起导入。运行器禁用原生模块、系统模块和任意文件加载，限制时间、内存和输出。支持执行陌生来源的模块之前，为运行器增加操作系统级沙箱。

## 由脚本发起采集

脚本可以分页、组合多个数据源，或根据上一次响应决定下一次请求。Studio 校验每次 I/O 调用。所有 I/O 方法均返回 Promise。

| `permissions[alias].kind` | 调用 | 规则 |
| --- | --- | --- |
| `http` | `context.http.request(alias, { method?, path?, headers?, body? })` → `{ status, text }` | 用户填写 HTTP(S) 地址，`methods` 默认为 `["GET"]`，也可声明 HEAD、POST、PUT、PATCH 或 DELETE。省略路径时请求原地址，指定路径时使用同源的 `/path?query`。跳转响应直接返回给脚本。正文为字符串。 |
| `http` | `context.http.getJson(alias, path?)` → JSON | GET 的简写，非 2xx 响应或无效 JSON 会抛出异常。 |
| `file` / `directory` | `context.files.readText(alias, path?)` → UTF-8 文本 | 文件授权下省略路径。目录授权下传相对文件路径，宿主将读取限制在授权目录内。 |
| `directory` | `context.files.list(alias, path?)` → `{ name, kind }[]` | 枚举授权目录或相对的子目录。 |
| `process` | `context.process.run(alias, args?)` → `{ code, stdout, stderr }` | 用户选择可执行文件。清单的 `args` 为可选用的完整参数组合，例如 `[["--summary"], ["--details"]]`。每次调用必须精确匹配一组。 |
| `stdio` | `context.process.request(alias, method, params?)`、`context.process.notify(alias, method, params?)` | 用户选择可执行文件，清单声明固定的启动 `args` 和允许的 `methods`。同一次采集复用一个逐行 JSON-RPC 会话，采集结束即关闭。 |
| `system` | `context.system.read(alias)` → `{ cpuPercent, memoryPercent, uptimeSeconds }` | 用户确认授权读取本机指标。 |

例如，作者声明名为 `metrics` 的 HTTP 权限和名为 `region` 的普通参数。用户把 `metrics` 绑定到服务地址，脚本根据当前状态决定是否继续取详情。

```js
import { defineModule } from '@tdisplay/sdk'

export default defineModule({
  async collect({ region }, context) {
    const query = '?region=' + encodeURIComponent(region)
    const status = await context.http.getJson('metrics', '/status' + query)
    if (status.paused) return { outputs: { latency: null } }

    const metrics = await context.http.getJson('metrics', '/latency' + query)
    return { outputs: { latency: { value: metrics.p95Ms } } }
  },
})
```

自定义脚本在独立 QuickJS 进程中运行。网络、文件和进程调用由 Studio 校验授权后代为执行。每次采集最多 24 次宿主调用，I/O 总时长和单次结果大小均受限。主动授权启动的程序以当前用户权限运行。

「测试运行」默认离线。样例数据用 `settings` 提供参数，用 `calls` 提供 `{ operation, alias, params, result }`。样例按实际调用的操作、别名和参数匹配。同一个调用重复出现时，依次消耗对应的样例响应。`operation` 可为 `http`、`readText`、`list`、`run`、`rpc`、`notify` 或 `system`。「用实际资源试跑」打开授权表单，本次测试和定时采集走同一套检查。

内置系统模块也使用 `context.system.read()`。Codex 脚本通过 `context.process.request()` 查询账号与额度，并在 JS 中选择额度窗口、汇总速度和判断显示哪个任务。Studio 管理 Codex 登录、App Server 握手，以及本地会话日志的读取和事件解析。

## 屏幕配置与设备协议

| 结构 | 必填字段 | 规则 |
| --- | --- | --- |
| `WidgetDefinition` | `outputKey`、`kind`、`label`、`supportedSpans: { w, h }[]` | `outputKey` 来自同一模块，输出类型必须兼容 `kind`。规格至少包含 `{ w: 1, h: 1 }`。定义可带渲染参数和默认样式。 |
| `DisplayConfig` | `schemaVersion: 1`、`profileId: string`、`theme`、`pages: PageConfig[]` | `profileId` 引用屏幕规格，例如 `tdisplay-c5-v1`。主题保存 `active: "dark" 或 "light"` 及两套 `{ background, surface, text }`。颜色统一使用 `#RRGGBB`。 |
| `PageConfig` | `pageId: string`、`name: string`、`cards: CardConfig[]` | 1 至 5 页，数组顺序即显示顺序，`pageId` 唯一。每页有独立的 4 列 × 2 行网格，可以是空页。 |
| `CardConfig` | `cardId: string`、`widgetId: string`、`sourceInstanceId: string`、`x`、`y`、`w`、`h` | 来源实例须属于组件所在模块。`cardId` 跨页唯一。坐标为从 0 开始的整数，`w` 是列数，`h` 是行数。卡片不能越界或重叠。v1 在同一份配置中，每组 `(widgetId, sourceInstanceId)` 放置一次。 |
| `DeviceDisplayConfig` | `schemaVersion`、`profileId`、`theme`、`pages` | 页和布局字段沿用 `DisplayConfig`。每张卡带 `cardId`、位置、规格和解析后的 `render`。`render` 包含 `kind`、`label` 及适用的渲染字段。 |
| `ReadingFrame` | `configRevision: integer`、`sequence: integer`、`readings` | `readings` 以 `cardId` 为键，包含已配置的全部卡片。每项为对应类型的读数对象或 `null`。读数对象必带 `kind`，且与该卡渲染配置一致。 |

`CardConfig.appearance` 可覆盖组件默认的 `accentColor` 和 `stateStyles`。同步时按状态键和属性合并，卡片设置优先。主数值的 `unit` 取自输出定义。状态组件的 `stateLabels` 为该输出的每个状态键提供标签，`detailFields` 选择输出已声明的辅助字段。Studio 在生成设备配置时完成这些解析。设备只需保存渲染配置和按 `cardId` 索引的读数。

状态栏使用设备运行状态，包括当前页 ID、连接方式、供电、电量、固件版本和最近成功同步时间。画布、字号、边距、滚动和动画曲线由 `DisplayProfile` 所引用的[渲染规范](display-ui.md)统一决定。

## 各 kind 字段

下表列出各类型的渲染字段和设备读数。**粗体为必填**，其余为可选。`kind` 只出现在组件定义、设备的 `render` 和非空设备读数中。`unit` 来自输出契约，其他渲染字段来自组件定义及卡片样式。

| `kind` | 定义字段 | 非空读数的字段 |
| --- | --- | --- |
| `metric` 普通指标 | `format: "number"`（默认）、`unit`、`numberFormat`、`numberAnimation`、`accentColor`、`detailFields` | **`value: number`**、`details` |
| `metric` 倒计时 | **`format: "countdown"`**、`expiredText`、`accentColor`、`detailFields` | **`deadlineUnixMs: integer`**、`details` |
| `progress` 进度 | `unit`、`numberFormat`、`numberAnimation`、`accentColor`、`detailFields` | **`value: number`**、**`ratio: number 或 null`**、`details` |
| `state` 状态 | **`stateLabels`**、`supportsNotify`、`stateStyles`、`accentColor`、`detailFields` | **`state: string`**、`detail: string 或 null`、`notify: boolean`、`details` |
| `chart` 图表 | **`chart`**、`unit` | **`windowEndUnixMs: integer`**、**`values: (number 或 null)[]`** |
| `trend` 趋势 | **`trend`**、`unit`、`numberFormat`、`numberAnimation`、`accentColor`、`detailFields` | **`value: number`**、**`delta: number 或 null`**、`details` |
| `range` 区间 | **`range`**、`unit`、`numberFormat`、`numberAnimation`、`accentColor`、`detailFields` | **`value: number`**、`bounds: { min, max }`、`details` |

`null` 读数表示当前没有数据，显示「—」或该类型的空态。数字 0 是有效读数。所有数值均为有限数，时间戳统一为 Unix 毫秒。表中未列出的字段不属于该类型。

### 通用可选字段

| 字段 | 类型及默认值 | 用途 |
| --- | --- | --- |
| `unit` | 字符串，默认空 | 主数值或图表最新值的单位，例如 `tok/s`。数值卡的单位区由渲染器按可用宽度自动折行。 |
| `numberFormat` | v1 为 `"compact"`，缺省同值 | 数值按量级和精度缩写，单位位置固定。 |
| `numberAnimation` | `none`、`digitRoll`、`count`，缺省为 `none` | 仅作用于主数值。位数滚动只更新变化的数位，计数动画从当前显示值过渡到新目标。 |
| `accentColor` | HEX 颜色，缺省用主题字色 | 普通色条、进度填充或状态卡常态标记色。图表柱体和趋势差值标签由渲染规范取色。 |
| `detailFields` | `{ key: string, label: string }[]`，默认空，最多两项且键唯一 | 声明 2 × 2 卡可展示的本组件辅助字段及顺序。只有定义支持 2 × 2 时适用。 |
| `details` | `Record<string, string>`，默认空 | 实时辅助文字。键须来自该组件的 `detailFields`，2 × 2 按声明顺序展示。 |

### 指标与进度

- **普通指标**使用 `value`。倒计时使用 `deadlineUnixMs`，由渲染端计算剩余时长。到期显示 `expiredText`，默认「待刷新」。
- **进度**将 `value` 作为主数值，将 `ratio` 作为 0–1 的进度比例。填充宽度为 `round(卡片宽度 × ratio)`。`ratio: null` 时保留底轨，主数值照常显示。

### 状态与提醒

| 字段 | 类型及规则 |
| --- | --- |
| `stateLabels` | 定义中非空的 `Record<string, string>`，将状态键映射到显示标签。 |
| `state` | 读数中的状态键，须存在于 `stateLabels`。如果定义声明了 `unknown`，它代表明确的未知状态。 |
| `detail` | 当前状态的详情文字，例如完整任务标题。缺省或 `null` 时显示占位。 |
| `supportsNotify` | 定义中的布尔值，默认 `false`。启用时才允许配置 `stateStyles` 和发送 `notify`。 |
| `stateStyles` | `Record<stateKey, { color?, alertText?, alertDurationMs? }>`。键须存在于 `stateLabels`。颜色为 HEX，`alertText` 也可为 `auto`，默认 `auto`。时长为 0 至 10000 毫秒的整秒，默认 2000 毫秒。 |
| `notify` | 读数中的布尔值，默认 `false`。写入方按本次更新决定是否请求播放。 |

状态标签取 `stateLabels[state]`。没有状态读数时使用组件标题。没有定义状态色时用常态标记色。顶部色条随状态更新，提醒期间保持新状态色。底色和字色按屏幕规格渐变。

### 写入方请求状态提醒

`notify: true` 请求播放本次提醒。即使状态没变也可以请求，目标状态的时长为 0 时直接显示。重发同一帧沿用原 `sequence`，接收端按连接内的帧序号去重。后续全量帧和重连快照省略该次请求的 `notify`。

新的提醒请求从最新状态重新计时。普通更新维持同状态的播放进度。状态变更且本次没有请求提醒时，结束旧提醒并直接显示新状态。

### 图表、趋势与区间

| 定义字段 | 必填子字段 | 读数与计算规则 |
| --- | --- | --- |
| `chart` | `series: "bar"`、`rangeSeconds: integer`、`sampleCount: integer`、`min: number`、`max: number` | 窗口长度和槽数均为正数，窗口须可按毫秒等分，`min < max`。`values` 长度等于 `sampleCount`，从旧到新。每槽取最后一个有效样本，时间区间左开右闭，以 `windowEndUnixMs` 为锚点。`null` 留空位，最新值标签取末槽。超出绘图值域时贴边，标签保留原值。1 × 1 最多 12 槽。 |
| `trend` | `comparisonSeconds: integer`，可选 `deltaUnit: string`，缺省取 `unit` | 比较周期为正整数。App 用同一口径计算当前值减上一周期值，写入带符号的 `delta`。方向由显示精度下的差值推导。基准缺失时用 `delta: null`，当前值仍可展示。CPU 从 64% 到 68% 为 `delta: 4`，单位是 `pp`。 |
| `range` | `mode: "fixed"` 时必带 `min`、`max`。`mode: "reading"` 时只声明模式 | 固定模式使用定义边界。动态模式允许读数附 `bounds`，上下限须成对且 `min < max`。指针按 `(value - min) / (max - min)` 定位，越界贴边，读数保留原值。缺少有效动态边界时只显示读数。 |

三种卡可共用一个输出，例如 `cpuUsage`。App 从同一数据源生成不同读数，卡片各自占位。

## 采集器完整示例

下面的模块通过 HTTP 读取 CPU 占用。同一路 `cpuUsage` 输出可显示为历史图表、趋势和区间。试跑区的「样例数据」可以提供离线响应。实际采集时脚本从项目声明并经用户授权的地址请求 `/cpu`。

### 入口脚本

把定义和采集函数放在同一个 `defineModule()` 调用中。这里把数值校验拆到项目内的辅助文件。

`src/index.mjs`

```js
import { defineModule } from '@tdisplay/sdk'
import { readUsage } from './read-usage.mjs'

export default defineModule({
  "name": "主机 CPU",
  "schedule": { "refreshSeconds": 60, "enabled": true },
  "resources": { "cpu": "https://host-a.example.com" },
  "permissions": {
    "cpu": { "kind": "http", "label": "主机地址" }
  },
  "outputs": {
    "cpuUsage": { "valueType": "number", "unit": "%" }
  },
  "widgets": {
    "cpu-usage-history": {
      "outputKey": "cpuUsage", "kind": "chart", "label": "CPU 历史",
      "supportedSpans": [{ "w": 1, "h": 1 }],
      "chart": { "series": "bar", "rangeSeconds": 3600, "sampleCount": 12, "min": 0, "max": 100 }
    },
    "cpu-trend": {
      "outputKey": "cpuUsage", "kind": "trend", "label": "CPU 趋势",
      "supportedSpans": [{ "w": 1, "h": 1 }],
      "trend": { "comparisonSeconds": 300, "deltaUnit": "pp" }
    },
    "cpu-range": {
      "outputKey": "cpuUsage", "kind": "range", "label": "CPU 区间",
      "supportedSpans": [{ "w": 1, "h": 1 }],
      "range": { "mode": "fixed", "min": 0, "max": 100 }
    }
  },

  async collect(_, context) {
    const value = readUsage(await context.http.getJson('cpu', '/cpu'))
    context.log(`CPU 占用 ${value}%`)
    return { outputs: { cpuUsage: { value } } }
  },
})
```

`src/read-usage.mjs`

```js
export function readUsage(cpu) {
  const value = cpu?.usagePercent
  if (!Number.isFinite(value) || value < 0 || value > 100) {
    throw new Error('usagePercent 必须是 0–100 的有限数')
  }
  return value
}
```

在试跑区的「样例数据」填写脚本请求 `/cpu` 时的响应。

```json
{
  "calls": [{
    "operation": "http",
    "alias": "cpu",
    "params": { "path": "/cpu" },
    "result": { "status": 200, "text": "{\"usagePercent\":68}" }
  }]
}
```

点「测试运行」后，输出面板显示 `cpuUsage = 68%`，区间卡和趋势卡可预览当前值。只有一次采样时，趋势的 `delta` 为 `null`。发布时确认 `https://host-a.example.com` 的访问范围，之后直接在屏幕布局选择 CPU 卡片。两台主机可以在同一个项目声明两个资源和对应输出，也可以各有一个独立发布的采集器。

### 屏幕配置

下面是 Studio 根据拖拽布局和主题设置保存的配置。主机 A 和主机 B 各发布一个采集器。布局从主机 A 选择三种组件，从主机 B 选择趋势组件并单独设置标记色。Studio 用模块 ID 和组件键生成卡片引用中的 `widgetId`，并为发布的数据源保存稳定 ID。

```json
{
  "schemaVersion": 1,
  "profileId": "tdisplay-c5-v1",
  "theme": {
    "active": "dark",
    "dark": { "background": "#1E1E1E", "surface": "#4A4A4A", "text": "#F1F1F1" },
    "light": { "background": "#E3E3E3", "surface": "#FAFAFA", "text": "#242424" }
  },
  "pages": [
    {
      "pageId": "hosts",
      "name": "主机监控",
      "cards": [
        {
          "cardId": "a1", "widgetId": "9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa.cpu-usage-history",
          "sourceInstanceId": "collector:9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa", "x": 0, "y": 0, "w": 1, "h": 1
        },
        {
          "cardId": "a2", "widgetId": "9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa.cpu-trend",
          "sourceInstanceId": "collector:9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa", "x": 1, "y": 0, "w": 1, "h": 1
        },
        {
          "cardId": "a3", "widgetId": "9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa.cpu-range",
          "sourceInstanceId": "collector:9b0f4ec8-7ee2-4edc-9bfa-52dd5f9cf4aa", "x": 2, "y": 0, "w": 1, "h": 1
        },
        {
          "cardId": "b1", "widgetId": "32ba9c20-786e-45bb-aeb5-50702d4c28ba.cpu-trend",
          "sourceInstanceId": "collector:32ba9c20-786e-45bb-aeb5-50702d4c28ba",
          "x": 0, "y": 1, "w": 1, "h": 1,
          "appearance": { "accentColor": "#D88A36" }
        }
      ]
    }
  ]
}
```

### 设备渲染配置与读数帧

同步时，Studio 把布局中的引用解析为设备能直接渲染的描述。下面是 `DeviceDisplayConfig` 中的主机 B 趋势卡。

```json
{
  "cardId": "b1",
  "x": 0,
  "y": 1,
  "w": 1,
  "h": 1,
  "render": {
    "kind": "trend",
    "label": "CPU 趋势",
    "unit": "%",
    "accentColor": "#D88A36",
    "trend": { "comparisonSeconds": 300, "deltaUnit": "pp" }
  }
}
```

继续运行后，主机 A 当前值为 68%，五分钟前为 64%。主机 B 当前值为 32%，缺少可用的比较基准。下面是一次发往设备的全量帧。

```json
{
  "configRevision": 7,
  "sequence": 141,
  "readings": {
    "a1": { "kind": "chart", "windowEndUnixMs": 1794280800000, "values": [18, 26, null, 42, 35, 61, 0, 73, 54, 88, 64, 68] },
    "a2": { "kind": "trend", "value": 68, "delta": 4 },
    "a3": { "kind": "range", "value": 68 },
    "b1": { "kind": "trend", "value": 32, "delta": null }
  }
}
```

### 其他输出示例

| 场景 | SDK 的单路输出值 | 设备读数 |
| --- | --- | --- |
| 室温 | `{ value: 23.5 }` | `{ kind: "metric", value: 23.5 }` |
| 专注计时 | `{ deadlineUnixMs: 1794282600000 }` | `{ kind: "metric", deadlineUnixMs: 1794282600000 }` |
| 下载进度 | `{ value: 512, ratio: 0.5 }`，单位为 MiB | `{ kind: "progress", value: 512, ratio: 0.5 }` |
| 动态温度量程 | `{ value: 42, bounds: { min: -10, max: 70 } }` | `{ kind: "range", value: 42, bounds: { min: -10, max: 70 } }` |
| 构建完成并提醒 | `{ state: "completed", detail: "固件构建完成" }`，本次请求提醒该输出 | `{ kind: "state", state: "completed", detail: "固件构建完成", notify: true }` |

## 同步与校验

1. Studio 保存可编辑的 `DisplayConfig`。点击「同步」时解析组件和实例引用，校验组件类型、输出契约、卡片规格、位置和样式，生成 `DeviceDisplayConfig` 并连同 `baseRevision` 提交。
2. 设备一次性校验配置结构、渲染字段、规格、位置和元数据。成功后应用配置并返回新版本和同步时间，失败时继续使用上一次成功配置。同一份已应用配置重试返回原确认结果。
3. 配置确认后，Studio 用已确认的卡片绑定及渲染参数生成该 `configRevision` 的全量帧。接收端跳过旧版本和乱序帧。切页直接使用当前有效读数，重连先对齐配置再补发快照。
4. 发布新版模块前，Studio 检查现有实例设置和屏幕引用，包括设备仍在使用的已同步布局。删除被引用的组件或输出、改变输出类型或单位、移除仍使用的状态键，以及移除卡片正在使用的规格，都需要先处理受影响的绑定。兼容版本发布后可继续更新现有卡片。标题和样式等渲染变化随下一次「同步」应用。正在运行的旧版本采集结果在实例切换后丢弃。
5. Studio 保留上一个发布版本。恢复时执行同样的引用和输入配置校验。模块导入、发布或恢复都通过模块目录操作，屏幕配置按设备确认单独生效。

`sdkApiVersion` 是 SDK 契约版本，`moduleRevision` 是模块发布版本，`schemaVersion` 是屏幕配置的结构版本，`configRevision` 是设备成功应用的配置版本，`sequence` 是连接内的帧顺序。

## 内置 Codex 模块迁移

Codex 模块随 Studio 发布。现有 Rust 适配器继续负责登录、CLI 和本地会话读取，并通过 `native` 输入提供额度窗口、重置卡数量、速度和任务状态。模块的 JavaScript 入口通过 `defineModule()` 生成下表中的读数，由脚本选择剩余比例最低的额度窗口，供额度和倒计时卡共同使用。额度和日志保留各自的刷新节奏，来源读数失效时由适配器提供空值。

| 现有布局 ID | 新 `widgetId` | `outputKey` | SDK 输出 |
| --- | --- | --- | --- |
| `quota` | `codex.quota` | `quota` | `progress`，主值为剩余百分比，`ratio` 为主值除以 100 |
| `speed` | `codex.speed` | `speed` | `number`，单位为 `tok/s` |
| `resets` | `codex.resets` | `resets` | `number`，单位为「张」 |
| `quota-reset` | `codex.quota-reset` | `quotaReset` | `deadline`，将现有秒级时间戳转换为 `deadlineUnixMs` |
| `task` | `codex.task` | `task` | `state`，将会话标题映射到 `detail` |

迁移旧布局时将原 ID 映射到新组件和默认 Codex 实例，保留格子位置。编辑器、桌面控件和 Rust 布局校验使用同一份运行时组件目录。读数按实例更新，再按设备已经确认的布局生成帧。桌面控件已支持 1–5 个独立页面、页面管理、手动翻页及可选轮播；跨格布局仍由设计原型展示。
