# CODE_MAP — 文件路径 ↔ 职责 ↔ 依赖映射

> 给 AI 用：快速定位"改 X 看哪里"、"动 A 影响哪些 B"、"哪些文件禁止碰"。

最后更新：2026-06-26（模型便携化：model_root 改为 exe 同级 `models/`；下载失败清理 `.part`；DESIGN §4.3 deepseek-chat 同步；2 个环境耦合测试改为自包含）

## 文件状态图例

- 🟢 已实现且稳定
- 🟡 已实现待重构
- 🔴 **未实现（计划中）**
- ⚫ 不计划实现（已排除）
- 🔒 锁定（AI 不要改，需用户授权）

---

## 顶层结构

```
SnapText/
├── Cargo.toml                  🟢 workspace 根
├── Cargo.lock                  🟢
├── rust-toolchain.toml         🟢 stable + MSVC
├── README.md                   🟡 骨架（DU-13 完善）
├── LICENSE                     🟢 MIT
├── AGENTS.md                   🔒 项目规范（人工维护）
├── .gitignore                  🟢
├── docs/                       本文档目录
├── crates/
│   ├── snaptext-core/          🟡 库 crate（DU-01 已完成；后续 DU 扩展 capture/ocr/translate/history/model_manager）
│   └── snaptext-app/           🟡 二进制 crate（DU-01 已完成；后续 DU 扩展 orchestrator/ui/tray/hotkey/clipboard）
├── scripts/                    🔴 辅助脚本（DU-13）
└── wix/                        🔴 cargo-wix 模板（DU-13）
```

---

## crates/snaptext-core/

库 crate，包含所有 trait 定义与具体实现。**纯逻辑层，不依赖 UI 框架**。

### src/lib.rs 🟢

crate 入口，仅 `pub mod xxx;` 与 crate 级文档测试。**不要在此写逻辑代码**。

### src/types.rs 🟢

跨模块共享的值类型。**不写 trait，不写业务逻辑**。

| 类型 | 用途 | 被谁用 |
|---|---|---|
| `Lang`（enum） | ISO 639-1 语言码（`En` / `Zh` / `Ja` / `Auto`） | OCR / Translate / History |
| `Bbox` | 矩形 `{x, y, w, h}`，i32 像素坐标 | Capture / OCR / UI |
| `MonitorId` | 显示器标识（newtype around `String`） | Capture / UI |
| `MonitorInfo` | 显示器元数据（分辨率、DPI、位置） | Capture / UI |
| `CapturedFrame` | 已捕获帧（含图、来源显示器、时间戳） | Capture / UI |
| `OcrLine` | OCR 单行结果（文本、bbox、置信度、方向） | OCR / Translate / UI |
| `WritingDirection` | `Horizontal` / `Vertical` | OCR / UI |
| `LangPair` | `{source, target}`，Translate 用 | Translate |
| `TranslateRequest` / `TranslateResponse` | 翻译 IO | Translate / History |
| `TokenUsage` | `{prompt_tokens, completion_tokens}` | Translate / History |
| `ProviderId`（`Cow<'static, str>`，支持 const 静态 + 运行时 String） | Provider 标识 | Translate / OCR |
| `AppState`（enum） | 应用状态机当前态 | Orchestrator / UI |

### src/error.rs 🟢

`thiserror` 错误类型。所有 trait 都返回 `Result<T, CoreError>`。

```rust
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("capture failed: {0}")]
    Capture(#[from] CaptureError),
    #[error("ocr failed: {0}")]
    Ocr(#[from] OcrError),
    #[error("translate failed: {0}")]
    Translate(#[from] TranslateError),
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("功能未实现：{0}")]
    NotImplemented(&'static str),
    // ...
}
```

> 不要返回 `anyhow::Error` 给上层。`anyhow` 仅在 binary 内部用。

### src/capture/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `CaptureProvider` trait 定义 + 导出默认实现 | `trait CaptureProvider`, `pub use WindowsCaptureProvider` | `types`, `error`, `async-trait` |
| `windows_capture.rs` | `WindowsCaptureProvider` 实现（WGC 优先 + DXGI fallback） | `struct WindowsCaptureProvider` | `windows-capture`, `image`, `tokio`, `async-trait` |

**修改约束**：动 `mod.rs` 的 trait 签名 = 同步动 `windows_capture.rs` + 所有调用方（`orchestrator.rs`）。

### src/ocr/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `OcrProvider` trait 定义 | `trait OcrProvider` | `types`, `error`, `image`, `async-trait` |
| `paddleocr.rs` | `PaddleOcrProvider` 实现（oar-ocr 封装） | `struct PaddleOcrProvider`（持 `Arc<OAROCR>`） | `oar-ocr`, `tokio`（spawn_blocking） |
| `preprocess.rs` | 图像预处理（oar-ocr 后端内部完成 resize/归一化，此处仅转 `RgbImage`） | `to_rgb` | `image` |
| `postprocess.rs` 🟢 | OCR 输出后处理（去 CJK 空格 / 合并换行 / trim） | `clean_ocr_text` | — |

**关键事实**（DU-04 落地）：oar-ocr 的 `OAROCR` 已实现 `Send + Sync`（内部 `Arc<Session>`），用 `Arc<OAROCR>` 跨线程共享，无需手动 `Mutex`（AI_GUIDE §3.1 陷阱已被 oar-ocr 解决）。ort 2.0 的 onnxruntime.dll 经 `download-binaries` 自动下载（R2 通过）。

### src/translate/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `TranslationProvider` trait + `common_pairs` + 工厂 | `trait TranslationProvider`, `build_provider` | `types`, `config`, `error`, `async-trait` |
| `prompt.rs` | LLM prompt 模板渲染 | `render_translate_prompt` | `types` |
| `openai_compat.rs` | OpenAI 兼容（DeepSeek 走此路） | `OpenAiCompatProvider` | `reqwest`, `serde_json` |
| `deepl.rs` | DeepL REST API | `DeepLProvider` | `reqwest` |
| `microsoft.rs` 🟢 | Azure Translator（DU-18） | `MicrosoftProvider` | `reqwest` |
| `baidu.rs` 🔴 P2 | 百度翻译（含 sign MD5，无 key 验证，推迟） | — | — |
| `postprocess.rs` 🟢 | 译文后处理（去引号 / trim / 去前缀） | `clean_translation` | — |
| `fallback.rs` 🟢 | Provider 故障转移包装器（主失败切备用） | `FallbackProvider` | Provider trait |

**关键事实**（DU-05 落地）：
- R9 已解决：DeepSeek 无 `deepseek-v4-flash`，默认模型 `deepseek-chat`（V3，官方确认）。
- Provider 构造时拿共享 `reqwest::Client`（CONVENTIONS §3.6），不每次 new。
- 超时：LLM 30s / MT 10s；错误归类 `TranslateError`（Timeout / Api{status,body} / Parse / Request）。

### src/history/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `HistoryStore` trait + `HistoryRecord` / `HistoryStats` | `trait HistoryStore`, `HistoryRecord` | `types`, `async-trait` |
| `sqlite_store.rs` | sqlite + r2d2 连接池（size 5）实现 | `SqliteHistoryStore::open/open_default/cleanup_blocking` | `rusqlite`, `r2d2_sqlite`, `r2d2`, `dao`, `migration`, `chrono` |
| `dao.rs` | CRUD（P0 `insert` + `cleanup`） | `dao::insert`, `dao::cleanup` | `rusqlite`, `chrono` |
| `schema.sql` | DDL（DESIGN §5.6） | 资源文件 | — |
| `migration.rs` | `PRAGMA user_version` 迁移 | `run_migrations` | `rusqlite` |
| `migrations/V001__initial.sql` | 初始迁移脚本 | 资源文件（include_str） | — |

**约定**：连接池 size 5，读写均 `spawn_blocking`。`insert`/`list`/`stats`/`delete_before`/`cleanup` 已实现（DU-06 + DU-15）。清理（retention_days + max_records）由 Orchestrator 启动时调 `cleanup_blocking`。

### src/model_manager/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | 模型路径解析 + 完整性校验（SHA256） | `model_root` / `ppocr_dir` / `det_model_path` / `rec_model_path` / `is_models_ready` / `verify_models` / `sha256_hex` | `dirs`, `sha2` |
| `downloader.rs` | ModelScope 流式下载（主源 + 可选镜像前缀，伪装浏览器 UA 绕 WAF 对 `.onnx` 的拦截）+ 进度回调 + 临时文件 rename | `download_models` / `candidate_urls` | `reqwest`(rustls), `sha2`, `futures-util`, `tokio` |

**关键事实**（DU-03 + DU-04 + v6 回归）：
- 模型来源：**oar-ocr 官方 PP-OCRv6**（medium/small）+ `ppocrv6_dict`。oar-ocr 0.7+ 原生支持 v6——DU-04 时 0.6.3 仅 v5 曾改用 v5，升 0.7.1 后回归 DESIGN 的 v6。
- 档位映射：`Tier::Medium`→`medium`，`Tier::Small`→`small`（v6 直接同名，非 v5 的 server/mobile）。
- 下载源：ModelScope `greatv/oar-ocr`，`https://www.modelscope.cn/models/greatv/oar-ocr/resolve/master/pp-ocrv6_{variant}_{det,rec}.onnx` + `ppocrv6_dict.txt`；国内直连，`extra_mirrors` 可选镜像前缀。
- 本地目录：可执行文件同级 `models\ppocr\v6\{tier}\{det,rec}.onnx` + `dict.txt`（**便携模式**，跟 exe 走；开发时在 `target\{debug,release}\models\`，安装后在安装目录，须可写；`v6` 段隔离历史版本）。

### src/config.rs 🟢

`Config` 结构（对应 `config.toml`），含 `load() -> Result<Config>` 和 `save()`。
路径解析：`%APPDATA%\SnapText\config.toml`（用 `dirs::config_dir()`）。
热重载：用 `notify` crate 监听文件变更（P2 评估）。

**首次引导 / 目标语言**：`GeneralConfig.onboarding_completed`（引导是否完成，未完成则启动弹 `ui/onboarding.rs`）；`TranslateConfig.target_lang`（翻译目标语言，源语言固定 `Auto`）。

---

## crates/snaptext-app/

二进制 crate。包含 main、UI、调度、系统集成。

### src/main.rs 🟢（DU-07 骨架 + DU-11 集成 orchestrator 端到端）
### src/first_run.rs 🟢（DU-11：模型缺失时同步下载，eprintln 进度）

入口。职责：
1. 初始化 `tracing`
2. 加载 `Config`
3. 启动 tokio runtime
4. 创建 Orchestrator channel
5. 启动 `global-hotkey` 监听
6. 启动 `tray-icon`
7. 进入 `eframe` 事件循环（**主线程**）

**不要** 在 main 里写业务逻辑。所有复杂度进 `orchestrator.rs` 或 UI 层。

**降级启动**：翻译 Provider 缺 API Key 时不再 `exit(1)`，构造为 `None` + warn 日志，程序照常启动；首启 `!onboarding_completed` 时把 `onboarding_open=true` 传入 UI 触发引导页。

### src/logging.rs 🟢

`tracing` + `tracing-subscriber` 初始化。

| API | 用途 |
|---|---|
| `fn init() -> Result<()>` | 初始化全局订阅，双输出（stderr + `%APPDATA%\SnapText\logs\snaptext.log`） |

**约束**：用 `dirs::config_dir()` 解析路径（AI_GUIDE §3.7），不硬编码。默认级别 `info`，`RUST_LOG` 可覆盖。

### src/orchestrator.rs 🟡（DU-10 核心完成 + mock 测试；DU-11 集成 main；缺 Key 降级 + 运行时重建）

中央协调器，持有 Provider（`Arc<dyn ...>`）+ channel。

| 类型 | 职责 |
|---|---|
| `struct Orchestrator` | 状态机主体，`handle` + `run`（tokio task） |
| `enum Command` | UI/Hotkey → Orchestrator 命令（含 `UpdateTranslateConfig` / `UpdateTargetLang` 即时生效） |
| `enum Event` | Orchestrator → UI 事件 |

**关键事实**（DU-10）：核心流程 TriggerCapture→Captured→RegionSelected→crop→OCR→Translate→History 已实现并 mock 测试通过。clipboard auto_copy / RetryTranslate 接入后续。

**翻译降级 + 运行时重建**：`translate: Option<Arc<dyn TranslationProvider>>`——缺 API Key 时为 `None`（翻译发 `Error` 提示去设置，不阻塞截图/OCR/设置面板）；另持 `translate_config` + `client`，收到 `UpdateTranslateConfig` 调 `rebuild_translate()` 即时重建。`UpdateTargetLang` 即时切目标语言。

**依赖**：`Arc<dyn CaptureProvider>`, `Arc<dyn OcrProvider>`, `Option<Arc<dyn TranslationProvider>>`, `Arc<dyn HistoryStore>`, `TranslateConfig`, `reqwest::Client`, channel。

### src/hotkey.rs 🟢

`global-hotkey` 封装。

| API | 用途 |
|---|---|
| `fn register(cfg: &HotkeyConfig) -> Result<(GlobalHotKeyManager, HotKey)>` | 注册触发热键，返回 manager 与已注册 HotKey |
| `fn re_register(manager: &GlobalHotKeyManager, old: HotKey, cfg: &HotkeyConfig) -> Result<HotKey>` | 运行时切换热键（设置保存后即时生效；先注册新再注销旧） |

**陷阱**：`global-hotkey` 在 Windows 上要求消息循环在主线程。egui 的 event loop 已经是消息循环，需要把 `GlobalHotKeyEvent::receiver().try_recv()` 集成到 egui 的 `ctx.run` 回调里。

### src/tray.rs 🟢

`tray-icon` 封装。同样要求主线程消息循环。菜单项：显示 / 暂停 / 设置 / 历史 / 退出。

### src/clipboard.rs 🟢

`arboard` 封装。注意 Windows 上 clipboard 操作必须在主线程（或拥有窗口的线程）。**不要** 在 tokio worker 线程直接调 `Clipboard::set_text`，要发命令给主线程。

### src/ui/ 🟢（DU-07/11 SnapTextApp；DU-08 overlay；DU-09 card 独立 viewport；DU-14 设置独立窗口+左侧导航；浅色主题 theme.rs）

| 文件 | 职责 | egui 关键 API |
|---|---|---|
| `mod.rs` | `SnapTextApp`（实现 `eframe::App`） | `eframe::App` trait |
| `fonts.rs` 🟢 | 中文字体注入（运行时读 Windows `msyh.ttc`/`simhei.ttf`，ab_glyph 预校验后注入 egui 字体族，避免中文乱码） | `FontDefinitions`, `FontData`, `ctx.set_fonts` |
| `theme.rs` 🟢 | 浅色主题（配色常量 + `apply(ctx)` 设 visuals/style + `card_frame` 分组容器；`main.rs` creation context 调用） | `ctx.set_visuals`, `ctx.set_style`, `Frame::group` |
| `overlay.rs` 🟢 | 选区 Overlay（全屏置顶 Viewport + 帧背景纹理 + 50% 蒙版 + 鼠标框选 + Esc 取消） | `show_viewport_deferred`, `Painter`, `interact_pos`, `Arc<Mutex<OverlayState>>` |
| `card.rs` 🟢 | 译文悬浮卡片（独立 always-on-top viewport，跟随选区位置 / 近屏边翻向 / 固定显示；原文折叠 + 复制译文/原文/关闭） | `show_viewport_deferred`, `ViewportCommand::OuterPosition`, `Arc<Mutex<CardState>>` |
| `settings.rs` 🟢 | 设置面板（独立 viewport：OS 原生标题栏 + 左侧导航 8 分类 + 右侧分组卡片；API Key 密码框；草稿机制 `Arc<Mutex<SettingsState>>`，保存写回 + 即时下发；关闭检测 `ViewportEvent::Close`） | `show_viewport_deferred`, `SidePanel::left`, `ScrollArea`, `card_frame` |
| `onboarding.rs` 🟢 | 首次启动引导页（热键/引擎/Key/档位/目标语言；完成/跳过后标记 `onboarding_completed`） | `egui::Window`, `ComboBox` |
| `history_view.rs` | 历史记录面板（P1, DU-15，精简版） | `egui::ScrollArea` |
| `updater.rs` | 自动更新（P2, DU-19） | HTTP + 签名校验 |

---

## scripts/

| 文件 | 用途 | 归属 |
|---|---|---|
| `download-models.ps1` | 离线下载 PP-OCRv6 模型到 `%APPDATA%\SnapText\models\` | DU-13 |
| `build-msi.ps1` | 调用 `cargo-wix` 生成 MSI | DU-13 |
| `stress-test.ps1` | 稳定性压测（模拟热键 + 鼠标，连续框选） | DU-12 验收 |
| `mirror-models.ps1` | 模型上传到国内镜像源（OSS / Gitee），发布前手动跑 | DU-13 发布辅助 |
| `verify-deps.ps1` | 开发环境自检（Rust / MSVS / WiX） | 开发辅助（无 DU 绑定） |

---

## 依赖图（模块层）

```
                    ┌─────────────┐
                    │   types     │  (无依赖)
                    └──────┬──────┘
                           │
        ┌──────────┬───────┼─────────┬────────────┐
        ▼          ▼       ▼         ▼            ▼
    capture      ocr   translate  history     model_manager
        │          │       │         │            │
        └──────────┴───────┼─────────┴────────────┘
                          ▼
                       config
                          │
                          ▼
                    orchestrator (app crate)
                          │
              ┌───────────┼───────────┐
              ▼           ▼           ▼
            hotkey      tray        ui
                          │
                          ▼
                        main
```

**铁律**：箭头方向严格遵循。`types` 是叶节点，所有模块可依赖它。任何 core 内模块**不得依赖** app crate 任何东西。

---

## 禁止碰清单（🔒）

| 路径 | 原因 |
|---|---|
| `AGENTS.md` | 用户级项目规范 |
| `docs/DESIGN.md` §1-§4 | 已对齐的核心设计（如需修订需用户确认） |
| `LICENSE` | 法律文件 |

---

## 修改 CODE_MAP 的时机

每次满足以下任一条件，必须同步更新本文档：
1. 新增/删除/重命名源码文件
2. 新增/删除 crate
3. trait 签名变更
4. 模块依赖方向调整

由实施者（人或 AI）在提交 PR / 完成任务时同步更新。
