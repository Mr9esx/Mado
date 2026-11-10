# Studio 控件与主题统一实施计划

**目标：** 使用 ReUI 统一 App 控件与视觉，并提供可保存的 App/桌面控件主题设置。
**实现：** ReUI 官方组件 + 统一 CSS tokens；Rust AppearanceStore 为偏好唯一来源，前端 Provider 监听跨窗口变更并应用系统主题。

- [x] 添加 appearance.rs 的默认、旧字段兼容、保存恢复和保存失败回滚测试，再实现 get/set 命令和事件。
- [x] 安装 ReUI Select/Tabs/Dialog 官方源码和 Radix 依赖，将 PageManager 与设置改用共享控件；保留原布局草稿行为。
- [x] AppearanceProvider 统一加载、更新、跨窗口事件、系统主题监听；Editor、DesktopWidget、CollectorWorkspace 使用共享显示主题。
- [x] App 使用系统字体与完整浅深 token；删除透明叠层和局部控件覆盖，统一字号、边框、圆角、间距；设备像素样式保留。
- [x] 前端/Rust 测试、类型与打包；真实 App 验证 ReUI 下拉、设置标签、浅深主题、独立画布主题、跨窗口同步、重启持久化，截图记录。

## 验证记录

- 前端 18 项、Rust 51 项测试通过；最后的原生主题更新条件调整后，外观专项 3 项测试再次通过。
- TypeScript、Vite、Tauri debug macOS bundle 和 git diff --check 通过。
- 原生 App：ReUI Select 选择轮播间隔生成草稿，撤销后恢复手动；设置 Tabs 可切到原 Codex 连接信息。
- 深色 App + 浅色画布同时显示，库缩略图与桌面控件遵从控件主题；退出重启后，两项设置分别恢复。
- 已打开的采集器窗口随 App 从深色切到跟随系统（本机浅色），无需重开；代码区和控件共享主题变量。
- 最终保留 App 跟随系统、控件深色；恢复置顶，原有两页布局及手动切页不变。
- UI 检查发现采集器两行列表项受到 Button 固定高度约束，模块与模板按钮改用 ReUI autoHeight。
- 验收页面：tests/fixtures/appearance.html；截图：reui-theme-settings.png、reui-editor.png（聊天可视化目录）。
