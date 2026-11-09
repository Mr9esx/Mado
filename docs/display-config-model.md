# TDisplay 屏幕配置数据模型

这是一版配置协议提案。App、虚拟屏和实体屏共享组件类型、网格和渲染规则。App 编辑用户配置，数据源发布实时读数，设备根据已同步配置渲染读数。

## 数据分层

| 数据 | 内容 | 谁维护 |
| --- | --- | --- |
| 屏幕规格 `DisplayProfile` | 画布 320 × 170、4 列 × 2 行、间距、状态栏、受支持规格、字号和动效时序 | 渲染规范。App 与设备按规格 ID 使用同一套规则 |
| 组件定义 `WidgetDefinition` | 稳定的组件 ID、数据源绑定、通用类型、标签、单位、格式、支持规格、状态键和可用辅助字段 | 组件目录。数据源接入时注册 |
| 屏幕配置 `DisplayConfig` | 选中的主题、深浅主题色、每张卡片的组件引用、位置、规格、标记色和状态样式 | 用户在 App 编辑，点击「同步」后生效 |
| 读数帧 `ReadingFrame` | 当前配置版本、帧序号、卡片读数及状态 | App 采集和归一化后发布 |

状态栏独立于卡片网格。连接与供电来自设备运行状态，上次同步时间来自配置成功应用的记录。它们在各自发生变化时更新。

## 组件与卡片

组件定义的 `widgetId` 标识可选组件，例如 `codex.quota`。屏幕上的卡片有自己的 `cardId`，移动、改规格和改色时保持这个 ID。位置的 `x`、`y` 从零开始，`w` 为列数，`h` 为行数。原型标注按行 × 列，1 × 4 对应 `w: 4, h: 1`。

| 组件定义示例 | 通用类型 | 格式和读取字段 |
| --- | --- | --- |
| `codex.quota` | `progress` | `value` 与 `ratio`，单位 `%` |
| `codex.speed` | `metric` | `value`，长单位 `tok/s` |
| `codex.quota-reset` | `metric` | `deadlineUnixMs`，倒计时格式 |
| `codex.task` | `state` | `state` 与 `detail`，定义声明支持的状态键 |

组件定义声明 `supportedSpans`，至少包含 1 × 1。2 × 2 可展示该组件定义允许的辅助字段。额度与额度恢复各有独立的定义、卡片和读数。v1 沿用当前同一组件只能放一张的限制，卡片 ID 为后续扩展保留独立身份。

例如任务组件声明自己的状态键和可用规格。新增一个数据源时，只需按通用类型注册组件。

```json
{
  "widgetId": "codex.task",
  "binding": { "source": "codex", "key": "task" },
  "kind": "state",
  "label": "任务状态",
  "supportedSpans": [
    { "w": 1, "h": 1 }, { "w": 2, "h": 1 }, { "w": 2, "h": 2 }, { "w": 4, "h": 1 }
  ],
  "stateLabels": {
    "running": "运行中",
    "completed": "已完成",
    "interrupted": "已中断",
    "error": "运行异常",
    "unknown": "状态未知"
  }
}
```

## 配置示例

这份配置对应左侧一张 2 × 2 额度卡，右上独立倒计时，右下一张 1 × 2 任务卡。

```json
{
  "schemaVersion": 1,
  "profileId": "tdisplay-c5-v1",
  "theme": {
    "active": "dark",
    "dark": { "background": "#1E1E1E", "surface": "#4A4A4A", "text": "#F1F1F1" },
    "light": { "background": "#E3E3E3", "surface": "#FAFAFA", "text": "#242424" }
  },
  "cards": [
    { "cardId": "c1", "widgetId": "codex.quota", "x": 0, "y": 0, "w": 2, "h": 2, "accentColor": "#48B34A" },
    { "cardId": "c2", "widgetId": "codex.quota-reset", "x": 2, "y": 0, "w": 1, "h": 1 },
    {
      "cardId": "c3",
      "widgetId": "codex.task",
      "x": 2, "y": 1, "w": 2, "h": 1,
      "stateStyles": {
        "completed": { "color": "#48B34A", "alertDurationMs": 2000, "alertText": "auto" },
        "error": { "color": "#D63F41", "alertDurationMs": 3000, "alertText": "#FFFFFF" }
      }
    }
  ]
}
```

`accentColor` 用于普通卡片的色条或进度填充。`stateStyles` 按组件定义的状态键配置，未覆盖的键使用组件默认样式。提醒持续时间为 0 时直接显示新状态。淡入、淡出、文字渐隐等时序统一由屏幕规格定义。

实时读数另发一帧。倒计时使用绝对截止时间，由渲染端计算剩余时长。示例中的进度比例是独立字段，可以和读数使用不同单位。

```json
{
  "configRevision": 7,
  "sequence": 141,
  "readings": {
    "c1": { "kind": "progress", "value": 81, "ratio": 0.81 },
    "c2": { "kind": "metric", "deadlineUnixMs": 1794272400000 },
    "c3": { "kind": "state", "state": "running", "detail": "整理设备屏显示和同步状态" }
  }
}
```

v1 每帧覆盖当前配置的所有卡片。缺值用 `null`，渲染为「—」。数字 0 是有效值，状态键 `unknown` 表示数据源明确报告状态未知。

## 同步与渲染

1. App 保存可编辑的 `DisplayConfig`。点击同步后，App 将组件定义解析成设备用的通用描述，附上卡片类型、标签、单位、格式、状态标签及允许的辅助字段。设备只收到当前布局所需的描述，可以用同一个渲染器显示 Codex 或其他来源的组件。
2. 设备收到带 `baseRevision` 的完整配置后，一次性校验版本、规格、格子边界、重叠、样式与状态键。成功应用后返回新配置版本和同步时间。校验失败时继续使用上一次成功配置。同一份已应用配置重试时返回原确认结果。
3. 配置确认后，实时帧使用对应的 `configRevision`。设备按当前连接的帧序号接收，跳过旧版本或乱序帧。重新连接时先对齐配置，再发送完整读数。配置版本和数据帧序号分别递增。
4. 状态提醒比较同一张卡片前后两次的状态键。首次取得读数显示常态。同状态下更新标题或重发读数时，保持当前滚动与提醒进度。只改配色不会被识别为任务状态变化。

`schemaVersion` 用于配置结构迁移，`configRevision` 用于一次次同步提交。现有项目已具备布局版本和快照序号，可以沿用这两个概念扩展同步。当前 App 的组件目录仍只开放 1 × 1，跨格示例来自设计原型。
