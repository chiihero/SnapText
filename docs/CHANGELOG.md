# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/) 格式。

## [0.1.2] — 2026-07-02 · 结果窗口交互优化

### 改进
- **OCR 文字层改为 DOM 可拖选复制**（`Result.vue`）：原文/译文从 canvas 位图改为按 OCR 行 bbox 绝对定位的 DOM `<div>` 文字层（PDF.js 文字层同款做法）。鼠标可框选部分文字、松开自动复制到剪贴板（Ctrl+C 仍可用），单行点击翻转原文/译文（拖选时 `getSelection()` 非空则不翻转，避免误触）。背景改为按 bbox 贴同位置模糊原图区块弱化，描边用 `-webkit-text-stroke` + `paint-order:stroke fill` 复刻原 strokeText 视觉。

### 修复
- **结果窗口工具栏按钮位置跳动**：顶部状态条用 `v-if="statusText"` 控制，文字为空时整条消失，下方工具栏整体上移跳动（典型：点「原文」OCR 完成显示提示行 → 点回「原图」行消失 → 按钮上跳）。改为状态条始终渲染 + `min-height` 占位锁死高度；并补全 `statusText` 分支，原图态（OCR 已完成）显示「已识别，点「原文」查看」提示，消除空感。

---

## [0.1.1] — 2026-07-02 · 内存优化 + README 完善

### 修复
- **OCR 大图后内存飙升且不回落**：release 构建下框选大图 OCR 翻译后，内存飙到 2-3G 且不回落。根因在 ort 2.0.0-rc.12——默认开启的 `memory_pattern` 在动态 shape（oar-ocr Type0 resize，每张图尺寸不同）下按"见过的最大 shape"扩容 pattern buffer 并永久保留，且 ort 无"推理后释放 arena"的 API。4 处对症改动：
  - **ort session 配置**（`paddleocr.rs`）：经 oar-ocr 的 `ort_session()` 透传口子传 `OrtSessionConfig`——关 `memory_pattern`（ort 官方明确说动态尺寸应关）、`intra_threads` 封顶 4（默认用满全核，各线程临时 buffer 叠加抬高峰值）、`image_batch_size(2)` / `region_batch_size(16)`（默认 det=8 / rec=推荐，框选场景一次一张图，大 batch 无收益纯费内存）。
  - **接力缓存主动释放**（`ocr_translate.rs`）：`translate_region` 写历史后清空 `last_crop` / `last_ocr`（框选流程已结束，释放裁剪图 + OCR 行）。
  - **shot:// 协议免 clone**（`main.rs`）：改为持锁期间直接编码 BMP，免 clone 整张全屏图（4K RGBA 单份 30MB+），保持原有三态状态码语义。
  - 不改 OCR 默认档（保持 Medium）、不重建 session（oar-ocr 无 API 且成本高）。

### 改进
- **打包目标精简**：`tauri.conf.json` 的 `bundle.targets` 仅保留 nsis，移除 msi（个人工具无需企业分发，减少产物体积/构建时间）。

### 文档
- **README 视觉重构**：新增界面预览截图、功能特性区、下载入口；强化 PP-OCRv6 卖点（副标题点出 + 特性详述）；引用 PP-OCRv6 官方 benchmark 数字 + 参考链接区。

---

## [0.1.0] — 2026-07-01 · 首个 GitHub Release

P0 首个可用版本，由 `tauri-apps/tauri-action` 在 `windows-latest` runner 上自动构建。
触发方式：`git tag v0.1.0` → push tag → CI 产出 NSIS `*-setup.exe` 并自动发布到 Release。

### 发布物
- `SnapText_0.1.0_x64-setup.exe`（NSIS 安装器）

> 功能与改进清单继承自下列 [Unreleased] 段落。

## [Unreleased] — P0 首个可用版本

### 新增
- **截图（DU-02）**：WGC + DXGI 双后端，捕获所有显示器；实测 2560×1440 屏幕截图 97.8% 非空。
- **OCR（DU-04）**：oar-ocr + PP-OCRv6（medium/small）本地识别；实测识别屏幕文字准确。
- **翻译（DU-05）**：DeepSeek（OpenAI 兼容，默认 `deepseek-chat`）+ DeepL，共享 reqwest 连接池，超时/重试。
- **历史记录（DU-06）**：sqlite + r2d2 连接池，翻译后写入；启动按 retention/max 清理。
- **模型管理（DU-03）**：ModelScope 下载（`greatv/oar-ocr`），SHA256 校验。
- **UI（DU-07/08/09/11）**：Snipaste 风格——热键 Ctrl+Alt+Q → 截图 → 框选 overlay → OCR → 翻译 → 悬浮卡片。托盘 + 单实例保护。
- **Orchestrator（DU-10）**：状态机串联 capture→ocr→translate→history，mock 测试验证完整流程。
- **首次引导页 + 即时生效**：首次启动弹引导（热键/引擎/API Key/档位/目标语言）；设置/引导保存后翻译/语言/热键即时生效，无需重启。

### 修复
- **热键占用导致启动崩溃**：全局热键（默认 Ctrl+Alt+Q）被其他程序占用时（典型：上一次 snaptext 进程残留未释放），原行为是 setup panic 整个应用无法启动。改为优雅降级——应用照常启动，首页弹一次性提示、设置页快捷键项标红，用户改键保存后即时生效。与翻译缺 Key 降级同款"缺资源不崩、UI 引导修复"模式。
- **模型路径便携化**：模型从隐藏的 `%APPDATA%\SnapText\models\` 改为可执行文件同级的 `models\`（跟程序走，便于检查/分发）；开发运行时位于 `target\{debug,release}\models\`。⚠️ 安装目录须可写（勿装 `Program Files`，否则首启下载失败）。
- **环境耦合测试失败**：`is_models_ready` / `verify_models` 的 2 个测试断言"medium 档未下载"，在本机已下载 medium 时必然失败；改为自包含 tempdir 测试（`verify_files_detects_missing`）。
- **下载失败残留 `.part`**：模型下载中途失败时清理临时 `.part` 文件，避免磁盘碎片。
- **代码格式**：`main.rs` / `orchestrator.rs` 等跑 `cargo fmt`，恢复 `fmt --check` 通过。
- **缺 API Key 无法启动**：启动时不再因缺 Key `exit(1)`，改为降级启动（翻译暂不可用，其余功能正常），可在程序内设置/引导页填 Key。
- **ModelScope 模型下载 403**：ModelScope WAF 对 `.onnx` 模型文件拦截非浏览器 User-Agent（reqwest 默认 UA → 403，字典等小文件不受限），下载请求伪装浏览器 UA 绕过。
- **UI 配置项失效**：`overlay_dim_alpha` / `card_font_size` / `show_original` 此前在 UI 代码中写死，设置面板的滑块/开关无效；现已接入。`auto_copy_translation` 此前未实现，现已按配置在翻译完成后自动复制译文到剪贴板。
- **设置面板拖动闪烁/不跟手**：自绘无边框 + 手动 `OuterPosition` 拖动在 deferred viewport 下坐标正反馈，导致闪烁不跟手。改用 OS 原生标题栏（拖动/缩放交给 Windows），关闭检测 `ViewportEvent::Close`。
- **卡片手动拖动**：同源坐标问题，去掉卡片手动拖动，卡片固定定位在选区旁（近屏边翻向）。

### 改进（UI 美化重构 · 阶段 1/4：核心体验）
- **选区 overlay**：蒙版不透明度可配置；拖动时显示全屏十字辅助线；进入选区时顶部提示"拖动鼠标框选文字区域 · Esc 取消"。
- **主窗口**：新增「开始截图」按钮（与热键同路）+ 当前热键提示 +「设置」入口，取代原先空洞的状态行。
- **译文卡片**：字号、是否显示原文跟随设置生效。

### 改进（UI 美化重构 · 阶段 2/4：卡片悬浮）
- **译文卡片独立窗口化**：从主窗口内的固定面板，重写为独立 always-on-top 系统窗口，定位到选区右下角（近右/下屏边自动翻向），标题栏可拖动，Esc/按钮关闭。
- **卡片操作**：新增「复制原文」按钮（与「复制译文」「关闭」并列）。

### 改进（UI 美化重构 · 阶段 3/4：设置面板 + 浅色主题）
- **浅色主题系统**：新增 `ui/theme.rs`，统一配色（柔和蓝强调 / 灰白背景 / 白卡片 / 浅灰边框）+ `apply(ctx)` 设 visuals/style + `card_frame` 分组容器；`main.rs` 启动时应用。
- **设置面板独立窗口 + 左侧导航**：对标 PixPin/Umi-OCR，重写为独立 OS 窗口（可拖出主窗口、自绘标题栏可拖动），左侧 8 分类导航 + 右侧分组卡片。
- **API Key 密码框**：DeepSeek / DeepL / Microsoft 的 Key 改为密码框 + 显隐切换。
- **补齐设置项**：翻译目标语言下拉、DeepL Free/Pro 套餐、Microsoft 区域。
- **新增配置**：`ui.minimize_to_tray_on_close`（关闭主窗口最小化到托盘，默认开；关窗逻辑阶段 4 接入）。
- **卡片主题统一**：卡片配色改用 `theme` 常量，与全局一致。

### 关键决策
- OCR 模型：oar-ocr 0.7+ 原生支持 PP-OCRv6，回归 v6（DU-04 时 0.6.3 仅 v5 曾改用 v5）；下载源 ModelScope。
- DeepSeek 模型：`deepseek-v4-flash` 不存在，用 `deepseek-chat`（V3）。
- 依赖：rusqlite 0.32（0.40 与 r2d2_sqlite 冲突）；eframe 0.30 / global-hotkey 0.8 / tray-icon 0.24。

### 已知限制
- 多显示器选区：当前单屏（跨屏选区已永久砍除）。
- 首启下载 UI：同步 eprintln（完整进度窗口推迟）。
- 代码签名 / 自动更新：P2 DU-19（个人用可跳过）。
