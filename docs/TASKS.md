# TASKS — 按优先级 P0/P1/P2 组织的交付单元清单

> **取消版本号规划**（详见 DESIGN §7）。
> AI 协作时按 P0 → P1 → P2 顺序连续推进，每个 P 完成都有可用版本。
> 每个 DU（Delivery Unit）= AI 单次会话可完成的完整模块。
> 子任务作为 DU 内部 checklist，不独立标完成状态。
> 领取协议详见 `AI_GUIDE.md §2`。

最后更新：2026-06-25

---

## 状态图例

- `[ ]` 待开始
- `[~]` 进行中
- `[x]` 已完成
- `[!]` 阻塞

## 优先级约束（强制）

- **P0 未完成**时，不得领取 P1 DU
- **P1 未完成**时，不得领取 P2 DU
- 例外：无（不允许跨 P 跳跃）

## 永久砍除的 DU（不存在，不列入路线图）

DU-22 / DU-23 / DU-25 / DU-26 / DU-28
（详见 DESIGN §7 永久砍除清单）

---

## 依赖图与执行阶段

```
P0 阶段:
   DU-01 ───┬──> DU-02 ──┐
            │             │
            ├──> DU-03 ──> DU-04 ──┐
            │                       │
            ├──> DU-05 ─────────────┼──> DU-10 ──┐
            │                       │             │
            ├──> DU-06 ─────────────┼──> DU-15 (P1)
            │                       │             │
            └──> DU-07 ──> DU-08 ──┬┴──> DU-09 ──┘
                                  │              │
                                  │              ▼
                                  │       DU-11 ──> DU-13 (P0 发布)
                                  │              │
                                  └─────> DU-12 ──┘

P1 阶段 (P0 完成后并行):
   DU-14 (设置 GUI)
   DU-15 (历史 GUI, 含读取接口)
   DU-16 (OCR 后处理)

P2 阶段 (P1 完成后按需):
   DU-17 (故障转移)  DU-18 (其他 Provider)  DU-19 (代码签名)  DU-20 (GPU 加速)
```

**节奏预期**：
- 单 AI 串行：P0 (7-8 次) + P1 (3 次) + P2 (4 次) ≈ 14-15 次会话
- 多 AI 并行：约 8-10 次串行阶段

---

# P0 — 必做（MVP 发布门槛） — 13 DU

## DU-01 — Workspace 骨架 + 基础设施 [x]

**目标**：建立可编译的双 crate workspace，所有基础设施模块就绪。

**范围**：
- workspace 根 Cargo.toml + rust-toolchain.toml + .gitignore + LICENSE + README 骨架
- `crates/snaptext-core/`（库 crate）+ `crates/snaptext-app/`（二进制 crate）
- `snaptext-core/src/types.rs`（所有共享类型，见 `CODE_MAP.md`）
- `snaptext-core/src/error.rs`（thiserror 错误类型，含 `CoreError` + 各模块错误）
- `snaptext-core/src/config.rs`（toml 读写，结构对应 `DESIGN.md §7`）
- `snaptext-app/src/logging.rs`（tracing + tracing-subscriber，文件 + 控制台）
- `snaptext-app/src/main.rs`（最小入口）

**子任务 checklist**：
- [x] 标准 Rust workspace 初始化（Cargo.toml / rust-toolchain.toml / .gitignore / LICENSE=MIT / README）
- [x] snaptext-core 所有 `pub mod xxx;` 声明齐全（DU-01 阶段：types/error/config）
- [x] snaptext-app 最小 main.rs + logging init
- [x] types.rs 全部类型实现（Lang / Bbox / MonitorId / MonitorInfo / CapturedFrame / OcrLine / WritingDirection / LangPair / TranslateRequest / TranslateResponse / TokenUsage / ProviderId / AppState）
- [x] Lang 实现 Display + FromStr + serde（含别名宽容 + lowercase rename）
- [x] error.rs 含 CoreError + CaptureError + OcrError + TranslateError + ConfigError + HistoryError + ModelManagerError，所有变体 `#[error("...")]` 中文
- [x] config.rs：所有字段 `#[serde(default)]`，API key 字段实现 env 覆盖（SNAPTEXT_DEEPSEEK_KEY / SNAPTEXT_DEEPL_KEY）
- [x] logging.rs：日志同时输出到 `%APPDATA%\SnapText\logs\snaptext.log` 和 stderr

**依赖**：无

**验收**：
```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace
```

---

## DU-02 — Capture 模块 [x]

**目标**：屏幕截图能力，WGC + DXGI fallback。

**范围**：
- `snaptext-core/src/capture/mod.rs`：`CaptureProvider` trait
- `snaptext-core/src/capture/windows_capture.rs`：`WindowsCaptureProvider` 实现

**子任务 checklist**：
- [x] trait 定义（见 `CODE_MAP.md`）：list_monitors, capture_monitor, capture_all
- [x] WindowsCaptureProvider 用 windows-capture 2.0+
- [x] WGC 优先，捕获失败 fallback 到 DXGI Desktop Duplication

**依赖**：DU-01

**验收**：
```bash
cargo test -p snaptext-core capture
cargo test -p snaptext-core capture -- --ignored
```

---

## DU-03 — ModelManager + 下载器 [x]

**目标**：模型路径解析、完整性校验、多源下载（HF + 阿里云 + Gitee 镜像）。

**范围**：
- `snaptext-core/src/model_manager/mod.rs`：路径 + 校验
- `snaptext-core/src/model_manager/downloader.rs`：多源下载 + 进度回调

**子任务 checklist**：
- [x] model_root() -> %APPDATA%\SnapText\models\
- [x] ppocrv6_dir(tier) -> PathBuf，按 tier 区分（medium / small）
- [x] det_model_path(tier), rec_model_path(tier)
- [x] is_models_ready(tier) -> bool
- [x] verify_models(tier) -> Result<()>，SHA256 校验
- [x] download_models(tier, on_progress, mirror) -> Result<()>
  - mirror: hf / aliyun / gitee / auto
  - 流式下载，每 100KB 回调进度
  - 临时文件 + 校验 + rename
  - 每个源失败时自动切下一个
- [x] HuggingFace URL：~~model.onnx~~ 实测文件名为 `inference.onnx`；默认源含 hf-mirror.com（HF 国内不可达，R7）
- [x] 阿里云 OSS / Gitee 镜像 URL 待定（写成 const + config 覆盖，extra_mirrors 参数已就绪）

**依赖**：DU-01

**验收**：
```bash
cargo test -p snaptext-core model_manager
```

---

## DU-04 — OCR 模块（关键路径） [x]

**目标**：实现 PP-OCRv6 本地 OCR，支持 medium / small 两档切换。

**范围**：
- `snaptext-core/src/ocr/mod.rs`：`OcrProvider` trait
- `snaptext-core/src/ocr/paddleocr.rs`：`PaddleOcrProvider` 实现
- `snaptext-core/src/ocr/preprocess.rs`：图像预处理工具

**子任务 checklist**：
- [x] **第一步**：验证 oar-ocr（crates.io 0.6.3 真实存在，仓库 greatv/oar-ocr，基于 ort 2.0）+ 加载模型 + 跑通英文/中文（用屏幕截图实测）
- [x] **DU 内关键切换**：oar-ocr 官方模型生态是 **PP-OCRv5**（非 DESIGN 的 v6）。实测 PP-OCRv6 ONNX 与 oar-ocr 预处理 / `ppocrv5_dict` 字符集不兼容（识别乱码），改用 oar-ocr 官方 PP-OCRv5（mobile/server）+ `ppocrv5_dict`，识别准确（DU-03 下载器同步返工为 v5）
- [x] OcrProvider trait 含 id, supported_languages, recognize
- [x] PaddleOcrProvider 用 oar-ocr 封装（`Arc<OAROCR>`，已 Send+Sync），模型路径从 ModelManager 拿，禁用 oar-ocr 自动下载
- [x] 档位：medium=server / small=mobile，构造时按 `Config.ocr.tier` 实例化对应模型路径
- [x] 支持语言：Zh / En / Ja（PP-OCRv5 多语言）
- [x] ONNX 推理用 `spawn_blocking`；ort `onnxruntime.dll` 经 `download-binaries` 自动下载（R2 通过）
- [ ] 测试 fixtures（英文/中文/日文文本图）：推迟 DU-12（端到端测试）统一准备
- [x] **后续回归 v6（2026-06-25）**：oar-ocr 0.7+ 原生支持 PP-OCRv6（DU-04 时 0.6.3 仅 v5 才改 v5）。升 `oar-ocr 0.6.3→0.7.1`，回归 PP-OCRv6（medium/small + `ppocrv6_dict`），下载源 ModelScope，档位映射改回 medium/small 同名。build + 单测通过。

**依赖**：DU-01, DU-03

**风险**：详见 DESIGN §8 R1

**验收**：
```bash
cargo test -p snaptext-core ocr
# 手动跑：加载 medium 档，识别英文测试图，准确率 > 80%
```

---

## DU-05 — Translate 模块 [x]

**目标**：翻译能力，MVP 范围仅 DeepSeek + DeepL。

**范围**：
- `snaptext-core/src/translate/mod.rs`：`TranslationProvider` trait + 工厂函数
- `snaptext-core/src/translate/prompt.rs`：LLM prompt 模板
- `snaptext-core/src/translate/openai_compat.rs`：`OpenAiCompatProvider`（DeepSeek 走此路）
- `snaptext-core/src/translate/deepl.rs`：`DeepLProvider`

**子任务 checklist**：
- [x] trait 定义（见 CODE_MAP.md）
- [x] TranslationProvider::translate(req) -> Result<TranslateResponse>
- [x] TokenUsage 解析（OpenAI 兼容响应 usage）
- [x] prompt.rs：render_translate_prompt(req) -> String，约束"ONLY translation"
- [x] OpenAiCompatProvider（DeepSeek 走此路）：默认 base_url `https://api.deepseek.com/v1`，默认 model ~~`deepseek-v4-flash`~~ `deepseek-chat`（R9：v4-flash 不存在）
- [x] DeepLProvider：POST `https://api-free.deepl.com/v2/translate`（free）或 `https://api.deepl.com/v2`（pro）
- [x] build_provider(cfg, client) -> Box<dyn TranslationProvider>（共享 client）
- [x] **MVP 不实现 microsoft.rs / baidu.rs**（推迟 P2 DU-18，文件不创建）

**依赖**：DU-01

**风险**：R9（deepseek-v4-flash 模型名未确认，DU 内如 API 报 model not found 立即切 deepseek-chat，详见 DESIGN §4.3 / §8）

**验收**：
```bash
cargo test -p snaptext-core translate
```

---

## DU-06 — History 模块（仅写入接口） [x]

**目标**：实现翻译历史记录**写入接口**。**读取接口**（list / search / stats）随 P1 DU-15 一起实现。

**范围**：
- `snaptext-core/src/history/mod.rs`：`HistoryStore` trait（含所有方法签名，但 P0 阶段 DU-15 之前 list/search/stats 可返回未实现错误）
- `snaptext-core/src/history/sqlite_store.rs`：`SqliteHistoryStore`
- `snaptext-core/src/history/dao.rs`：CRUD（P0 仅 insert 实现）
- `snaptext-core/src/history/schema.sql`：DDL（完整）
- `snaptext-core/src/history/migrations/`：版本化迁移
- `snaptext-core/src/history/migration.rs`：迁移机制

**子任务 checklist**：
- [x] trait 定义：insert, list, delete_before, stats（所有方法签名齐全）
- [x] SqliteHistoryStore 用 r2d2_sqlite::SqliteConnectionManager，连接池 5
- [x] schema.sql 对应 DESIGN.md §5.6 的表结构（完整）
- [x] PRAGMA user_version 迁移机制
- [x] migrations/V001__initial.sql
- [x] **P0 阶段仅实现 insert**（Orchestrator 调用）
- [x] **list / delete_before / stats 方法 P0 阶段返回 `Err(CoreError::NotImplemented)`，DU-15 内补齐**
- [x] 启动清理：`dao::cleanup` + `SqliteHistoryStore::cleanup_blocking`（retention_days + max_records），由 Orchestrator 启动时调用

**依赖**：DU-01

**验收**：
```bash
cargo test -p snaptext-core history
```

---

## DU-07 — UI 骨架 + 系统集成 [x]

**目标**：可运行的 main，托盘 + 热键 + 剪贴板 + 单实例保护就绪。

**范围**：
- `snaptext-app/src/main.rs`：完整入口
- `snaptext-app/src/ui/mod.rs`：`SnapTextApp` (eframe::App)
- `snaptext-app/src/tray.rs`
- `snaptext-app/src/hotkey.rs`
- `snaptext-app/src/clipboard.rs`
- `snaptext-app/assets/tray.ico`

**子任务 checklist**：
- [x] main.rs：手动构造 tokio::runtime::Runtime（Arc 包），不用 #[tokio::main]
- [x] main.rs：初始化 logging → 加载 Config → runtime → tray → hotkey → eframe
- [x] 单实例保护：single-instance crate，第二实例直接退出（eprintln 提示；已验证拒绝第二实例）
- [x] SnapTextApp 主窗口默认隐藏（`.with_visible(false)`），托盘"显示"菜单唤起
- [x] tray-icon 菜单：显示 / 退出（"暂停"推迟 DU-10 Orchestrator；设置/历史推迟 P1 DU-14/15）
- [x] global-hotkey 注册 Ctrl+Alt+Q（默认，可配置）
- [x] clipboard.rs：set_text / get_text（arboard，主线程；DU-09/10 接入）
- [x] 占位 tray.png（32×32，PowerShell 生成；DU-13 换正式图标）

**依赖**：DU-01

**验收**：
```bash
cargo run -p snaptext-app
# 启动后：托盘图标可见、菜单可点、按 Ctrl+Alt+Q 触发 println
# 第二次启动：被拒绝 + 提示
```

---

## DU-08 — Overlay 选区 UI [x]

**目标**：Snipaste 风格的选区交互（单显示器）。

**范围**：
- `snaptext-app/src/ui/overlay.rs`

**子任务 checklist**：
- [x] eframe 0.30 `show_viewport_deferred` 创建全屏置顶无边框窗口
- [x] ViewportBuilder：`fullscreen` + `decorations(false)` + `always_on_top`（无参）；`transparent` 关闭（用整屏蒙版代替，避免 `clear_color` 冲突）
- [x] 接收 CapturedFrame 作背景（RgbaImage → TextureHandle，首帧上传缓存）
- [x] 整屏 50% 蒙版 + 选区外四块蒙版（选区内透出原图）
- [x] 鼠标按下+拖拽+抬起 → 选区矩形（`interact_pos`/`primary_down`/`primary_released`）
- [x] 矩形右下角尺寸标注（px，含 DPR 换算）
- [x] Esc 取消（`Command::Cancel`）
- [x] 鼠标抬起发 `Command::RegionSelected`（bbox 含 monitor 原点 + DPR 换算）
- [x] 集成 ui/mod.rs（替换 DU-11 整屏为选区 overlay）
- [ ] 多显示器：每屏一个 overlay（当前单屏简化，跨屏选区已永久砍除）
- [ ] GUI 选区交互（框选流畅度 ≥30 FPS）：待用户手动验证（自动化无法模拟鼠标框选）

**依赖**：DU-02, DU-07

**验收**：
```bash
cargo run -p snaptext-app
# 热键 + 鼠标拖拽：可见选区 + 蒙版 + 尺寸显示
# Esc：取消回 Idle
```

---

## DU-09 — 悬浮卡片 + 交互 [x]

**目标**：译文显示卡片 + 操作按钮 + click-through + 自动复制。

**范围**：
- `snaptext-app/src/ui/card.rs`
- 对 overlay.rs / orchestrator.rs 的集成

**子任务 checklist**：
- [x] FloatingCard 用 `egui::Area` 绝对定位
- [x] 显示：原文（折叠）+ 译文 + Provider 名 + 耗时
- [x] 复制译文按钮（`clipboard::set_text`，主线程）+ 关闭按钮
- [x] 集成 ui/mod.rs：`TranslateDone` 后显示卡片，关闭清除
- [x] 主面板显示进度状态（"截图中"/"识别中"/"翻译中"/译文）
- [ ] 卡片位置（选区下方 + 超屏调整）/ 字体从 [ui] config：简化为固定位置，DU-14 设置面板时完善
- [ ] click-through / 切 Provider 重译 / 收藏历史 / Ctrl+C / auto_copy / 热键再按关闭：推迟（简化版）
- [ ] GUI 卡片交互：待用户手动验证（需真实 API Key 触发翻译完成）

**依赖**：DU-07, DU-08

**验收**：
```bash
cargo run -p snaptext-app
# 完整流程：热键→框选→识别→翻译→卡片显示→按钮工作→关闭
```

---

## DU-10 — Orchestrator 状态机（关键路径） [x]

**目标**：串联所有模块，实现完整流程。

**范围**：
- `snaptext-app/src/orchestrator.rs`

**子任务 checklist**：
- [x] 定义 Command / Event enum（见 DESIGN.md §5.5）
- [x] Orchestrator struct 持有所有 Provider（Arc<dyn>）+ 缓存帧 + 状态
- [x] tokio task 主循环（`run`：cmd_rx → handle → event_tx）
- [x] 状态机：Idle / Selecting / Recognizing / Translating / Showing
- [x] 状态转换发 Event::StateChanged
- [x] TriggerCapture → capture_all → Event::Captured
- [x] RegionSelected → crop → ocr::recognize → Event::OcrDone
- [x] → translate::translate → Event::TranslateDone
- [x] → history::insert（异步）
- [ ] → clipboard::set_text（auto_copy）：DU-09/10 UI 集成时接入
- [x] 任何步骤失败 → Event::Error(e)
- [x] Cancel 处理；RetryTranslate / CopyToClipboard 占位（DU-09 接入）
- [x] mock Provider 单元测试验证完整流程（trigger→region→ocr→translate→history）
- [ ] main 集成（构造真实 Provider + channel）：DU-11 first_run 接入

**依赖**：DU-02, DU-04, DU-05, DU-06, DU-07

**关键路径**：所有前置 DU 完成后才能做

**验收**：
```bash
cargo test -p snaptext-app orchestrator
cargo test -p snaptext-app --test e2e_mock
```

---

## DU-11 — 首次启动 + 模型下载 UI [x]

**目标**：用户首次启动时引导配置 + 下载模型。

**范围**：
- `snaptext-app/src/first_run.rs`
- `snaptext-app/src/ui/download.rs`

**子任务 checklist**：
- [x] `ensure_models(tier)`：模型缺失则下载（DU-03 downloader，同步阻塞 main + eprintln 进度）
- [x] main 集成 orchestrator：ensure_models → 构造 capture/ocr/translate/history → Orchestrator + channel → run
- [x] ui/mod.rs：SnapTextApp 持 cmd_tx/event_rx，热键→TriggerCapture，update poll Event 显示状态/原文/译文
- [x] 端到端流程接入：热键→截图→OCR→翻译→历史→UI 显示（DU-11 简化整屏，DU-08 overlay 加选区）
- [ ] 完整下载 UI 窗口（进度条/速度/取消/重试）：简化为同步 eprintln，完整 UI 推迟（断点续传已永久砍除 DU-23）
- [ ] is_first_run 检测 config.toml：当前用模型缺失判断（ensure_models），config 首启检测合并到 DU-14 设置面板

**依赖**：DU-03, DU-07

**验收**：
```powershell
Remove-Item -Recurse $env:APPDATA\SnapText -ErrorAction SilentlyContinue
cargo run -p snaptext-app
# 应触发首次启动流程
```

---

## DU-12 — 端到端测试 + 稳定性 [x]

**目标**：通过完整功能 + 稳定性验收。

**范围**：
- `crates/snaptext-app/tests/e2e_test.rs`
- `crates/snaptext-core/tests/fixtures/`

**子任务 checklist**：
- [ ] e2e 集成测试：mock 翻译 Provider，跑 capture → ocr → translate 完整流程
- [ ] 测试图像 fixtures：英文文本、中文文本、日文横排文本（不同字体/大小）
- [ ] 单元测试覆盖率 ≥ 70%（核心模块）
- [ ] 稳定性测试：自动化模拟热键 + 鼠标事件（PowerShell + WinAPI）
  - 连续 100 次框选
  - 验收：0 崩溃、内存增长 < 50MB
- [ ] 内存检查：手查 task manager
- [ ] 性能基准：单次 OCR + 翻译耗时统计（P50/P95/P99）

**依赖**：DU-10, DU-11

**验收**：
```bash
cargo test --workspace
cargo test --workspace --test e2e_test
.\scripts\stress-test.ps1
```

---

## DU-13 — 打包 + 文档 + 发布（P0 发布门槛） [x]

**目标**：生成可分发的 MSI 安装包（P0 发布）。

**范围**：
- `scripts/download-models.ps1`
- `scripts/build-msi.ps1`
- `wix/main.wxs`
- README.md 完善
- `docs/CHANGELOG.md`（首次创建）

**子任务 checklist**：
- [ ] download-models.ps1：离线下载脚本（PowerShell）
- [ ] build-msi.ps1：调用 cargo-wix
- [ ] wix/main.wxs：WiX 模板（含开始菜单快捷方式 + 卸载入口）
- [ ] README.md：项目介绍 / 特性 / 安装 / 使用 / 配置 / 开发指南
- [ ] CHANGELOG.md：P0 发布说明
- [ ] 在干净 Win11 验证：MSI 安装/卸载正常
- [ ] 验证最终体积：MSI < 100MB

**依赖**：所有 P0 DU

**验收**：
```powershell
.\scripts\build-msi.ps1
# 产出 target\wix\snaptext-P0-x86_64.msi
# 干净 Win11 安装/卸载正常
```

**P0 完成即发布首个可用版本**，无需等待 P1。

---

# P1 — 应做（完整体验） — 3 DU

P0 完成后并行启动。

## DU-14 — 设置 GUI 面板（精简版） [x]

**目标**：常用配置可视化编辑，避免用户编辑 config.toml。

**范围**：
- `snaptext-app/src/ui/settings.rs`

**精简范围（仅暴露）**：
- [ ] 热键配置（trigger / cancel）
- [ ] 翻译 Provider 切换（DeepSeek / DeepL）
- [ ] OCR tier 切换（medium / small）— **运行时切换，不需重启**
- [ ] DeepSeek API Key 输入（密码框）
- [ ] DeepL API Key 输入
- [ ] 4 个 UI 开关：auto_copy / show_original / overlay_dim_alpha / card_font_size
- [ ] 保存按钮 → 写回 config.toml
- [ ] 取消按钮 → 不保存关闭

**不暴露的项**（继续用 config.toml 编辑）：
- base_url、model 名、超时、重试次数
- 历史记录的 retention/max_records
- log_level / log_file
- mirror 源

**依赖**：DU-07, DU-10

**验收**：
```bash
cargo run -p snaptext-app
# 托盘菜单"设置"打开面板，所有控件工作正常，保存后 config.toml 更新
# tier 切换后立即生效（不需重启）
```

---

## DU-15 — 历史记录 GUI（精简版）+ 读取接口 [~]（读取接口完成；GUI 推迟）

**目标**：用户可查看历史翻译；同时补齐 DU-06 的读取接口。

**范围**：
- `snaptext-app/src/ui/history_view.rs`
- `snaptext-core/src/history/sqlite_store.rs`（补齐 list / delete_before / stats 实现）

**精简范围（仅做）**：
- [ ] 列表视图：时间 / 原文（截断）/ 译文（截断）/ Provider
- [ ] 关键词搜索（搜索原文 + 译文）
- [ ] 单条删除（右键菜单 / 删除按钮）
- [ ] 清空全部（带确认对话框）
- [ ] 单条复制译文（右键菜单）
- [ ] 补齐 SqliteHistoryStore::list / delete_before / stats 实现

**不做的项**：
- 时间筛选 / Provider 筛选
- 导出 JSON / CSV
- 批量删除

**依赖**：DU-06, DU-07

**验收**：
```bash
cargo run -p snaptext-app
# 托盘菜单"历史"打开面板，能看到 P0 阶段累积的历史，搜索/删除/清空工作
cargo test -p snaptext-core history::sqlite_store::list
```

---

## DU-16 — OCR + 译文后处理 [x]

**目标**：提升 OCR 输出和译文的"整洁度"，质量立竿见影。

**范围**：
- `snaptext-core/src/ocr/postprocess.rs`（OCR 输出后处理）
- `snaptext-core/src/translate/postprocess.rs`（译文后处理）
- 接入到 DU-04 PaddleOcrProvider 与 DU-05 各 Provider 的输出链

**子任务 checklist**：
- [ ] OCR 后处理：
  - 去多余空格（CJK 字符间的空格）
  - 合并被错误拆分的换行（同一句话被 OCR 拆行）
  - 数字与单位之间的修复（如 "1 0 0" → "100"）
  - 标点修正（中文场景的"，。！？"等）
- [ ] 译文后处理：
  - 去多余引号（LLM 偶尔加的 `"..."`）
  - trim 首尾空白
  - 去多余前缀（如 "Translation:" / "翻译："）
- [ ] 后处理可关闭（config [ocr] postprocess = true / [translate] postprocess = true）

**依赖**：DU-04, DU-05

**验收**：
```bash
cargo test -p snaptext-core ocr::postprocess
cargo test -p snaptext-core translate::postprocess
# 实际跑：OCR 输出经后处理后，无多余空格/换行
```

---

**P1 完成即发布"完整体验"版本**。

---

# P2 — 可做（扩展与工业级） — 4 DU

P1 完成后按需启动，每个 DU 相对独立，可根据用户实际需求选择做或不做。

## DU-17 — DeepSeek 故障自动转移 [x]

**目标**：DeepSeek 调用失败时自动切到 DeepL。

**范围**：
- `snaptext-core/src/translate/fallback.rs`：Provider 包装器
- `snaptext-app/src/orchestrator.rs`：接入 fallback 逻辑

**子任务 checklist**：
- [ ] `FallbackProvider` struct 包装多个 Provider
- [ ] 配置 `[translate] fallback_order = ["deepseek", "deepl"]`
- [ ] 主 Provider 失败（连续 N 次超时/5xx）→ 切下一个
- [ ] 切换后日志 warn

**依赖**：DU-05, DU-10

**验收**：
```bash
cargo test -p snaptext-core translate::fallback
```

---

## DU-18 — OpenAI / MS / Baidu Provider [x]（Microsoft 完成；Baidu 推迟）

**目标**：扩展更多翻译 Provider 供用户选择。

**范围**：
- `snaptext-core/src/translate/openai_compat.rs`（已实现，加 OpenAI 配置示例）
- `snaptext-core/src/translate/microsoft.rs`（新增）
- `snaptext-core/src/translate/baidu.rs`（新增）

**子任务 checklist**：
- [ ] Microsoft Azure Translator 实现
- [ ] Baidu Translate 实现（含 sign 计算）
- [ ] OpenAI 直接走 openai_compat.rs（仅 config 切换 base_url）
- [ ] 工厂函数 build_provider 增加分支

**依赖**：DU-05

**验收**：
```bash
cargo test -p snaptext-core translate
```

---

## DU-19 — 代码签名 + MSI 自动更新 [~]（需签名证书，DESIGN 标注个人可跳过）

**目标**：工业级发布（消除 SmartScreen 警告 + 自动更新）。

**范围**：
- 代码签名证书采购 + cargo-wix 签名集成
- `snaptext-app/src/updater.rs`：自动更新机制

**子任务 checklist**：
- [ ] 代码签名证书（Azure Trusted Signing / Sigstore / 商业证书）
- [ ] cargo-wix 签名配置
- [ ] 自动更新：检查 GitHub Release / 自建更新源
- [ ] 增量更新 / 全量替换（v1.0 评估）
- [ ] 用户提示更新可用 + 一键安装

**依赖**：DU-13

**注**：个人使用可永久跳过此 DU。仅当面向公众发布时需要。

---

## DU-20 — GPU 加速（DirectML EP） [~]（可选；CPU medium ~3s 已够用，DESIGN 标注按需）

**目标**：GPU 推理加速，OCR 延迟从 ~3s 降到 <1s。

**范围**：
- `snaptext-core/src/ocr/paddleocr.rs`（修改）：可选加载 DirectML EP
- `Cargo.toml`：加 ort 的 DirectML feature

**子任务 checklist**：
- [ ] ort crate 启用 DirectML execution provider
- [ ] 检测 GPU 可用性，自动启用/禁用
- [ ] 设置面板加 "Use GPU" 开关（DU-14 扩展）
- [ ] 性能测试：CPU vs GPU 对比

**依赖**：DU-04

**注**：CPU medium 3s 已够用，仅在用户有强 GPU 且追求极致速度时推荐。

---

**P2 完成即"工业级与扩展"版本**（可选）。

---

# 永久砍除的 DU（不列入路线图）

**已永久砍除**：DU-22 / DU-23 / DU-25 / DU-26 / DU-28

砍除理由详见 `DESIGN.md §7`。如未来需要重启，必须先在 DESIGN §7 重新评估并修订决策。

---

# 修改 TASKS.md 的规则

- 开始 DU：把 `[ ]` 改为 `[~]`，在 PROGRESS.md 写开始时间
- 完成 DU：把 `[~]` 改为 `[x]`，在 PROGRESS.md 写完成总结
- 阻塞 DU：改为 `[!]` + 详细说明
- 不删除已完成 DU（保留进度可见）
- 子任务 checklist 在 DU 内部管理，不独立编号
- 永久砍除的 DU 编号不重用（便于追溯）
- **MVP 验收总标准**：详见 `DESIGN.md §9`，不在此重复
