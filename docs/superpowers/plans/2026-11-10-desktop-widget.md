# Desktop Widget Implementation Plan

**Goal:** 将虚拟屏变为不依赖硬件、可以置顶的桌面控件。
**Architecture:** 复用本机 Device 快照；Rust 管理独立窗口和置顶偏好；React 提供悬停工具栏及本机状态栏。
**Tech Stack:** Tauri 2 / Rust / React 19 / TypeScript。

## Constraints

保留当前工作区已有改动，不另建丢失未提交功能的工作树。不增加新的依赖。画布保持 320 × 170。使用原生 always_on_top。配置只保存置顶偏好。

## Tasks

- [x] 在 `src-tauri/src/desktop_widget.rs` 建立偏好存储测试：缺失配置默认置顶、false 保存后重新加载仍为 false、保存失败不改变内存状态。先运行失败测试，再实现 JSON 原子保存与默认值恢复。
- [x] 在上述模块实现 `open_desktop_widget`、`get_desktop_widget_pinned`、`set_desktop_widget_pinned`、`show_editor`，在 `main.rs` 注册。窗口仍用 label `display`。现有窗口先 show/unminimize/focus，新窗口从已保存的置顶偏好创建；编辑器不存在时从 main 配置重建。
- [x] 新增 `src/DesktopWidget.tsx`，接入本机快照；将拖动、编辑入口、置顶切换、关闭及失败提示放在控件窗口。将 `main.tsx` 的虚拟屏组件替换为此组件。
- [x] 在 `Screen.tsx` 增加 desktop 输出模式，首次打开显示布局；状态栏使用 App 版本和本机标识。编辑器与控件都使用此模式，原设备模式保持不变。
- [x] 更新编辑器文案、权限描述、采集器返回编辑器逻辑和 README。添加本机模式的视觉验收场景。
- [x] 运行 `npm test`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`、`git diff --check`。
- [x] 打包并实际检查：无硬件直接显示、默认置顶、取消/恢复置顶、关闭后重开保留偏好、关闭编辑器后数据持续更新、从控件返回编辑器。保存截图，保持可用 App 打开。

## 验证结果

- 前端 12 项测试通过；Rust 44 项测试通过；生产前端构建与 macOS App 打包通过。
- 实际 App 在未应用布局时直接显示本机读数；原生置顶默认开启，可取消并在关闭/重开控件后恢复偏好。
- 关闭主窗口后，控件继续显示且速度读数持续变化；编辑布局入口可重建主窗口。最终保留控件打开并启用置顶。
- 重启前发现用户的未应用 8 格草稿，经用户明确选择「丢弃草稿，直接重启验证」后才退出旧版本。
