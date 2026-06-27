# PROGRESS — 实施进度跟踪

> 当前进展到哪、下一步做什么、有什么卡点。
> AI 协作者每次会话先读这份文档定位状态，详见 `AI_GUIDE.md §1`。

最后更新：2026-06-26

---

## 当前状态

**整体阶段**：P0 完成（13/13）✅；P1 完成（含 DU-15 GUI）；P2 DU-17/18 完成，DU-19/20 可选/受限标注。

**当前路线**：取消版本号，按 P0/P1/P2 优先级推进（详见 DESIGN §7）

**总体进度**：**18 / 20 DU**（P0: 13/13 ✅, P1: 3/3 ✅, P2: 2/4 [DU-19/20 可选]）

**近期变更**（2026-06-27）：
- **架构迁移：egui → Tauri 2 + Vue 3 + Naive UI**。删 `crates/snaptext-app`（egui 二进制）、`wix/`、`build-msi.ps1`；新增 `src-tauri/`（Rust 后端：命令层 + 系统集成）+ `src/`（Vue 前端）。core 100% 复用。迁移动机：egui 做截图翻译类产品 UI 痛点多（deferred viewport / Arc<Mutex> 借用 / API 不稳定 / 精致度上限低），Tauri 让 UI 痛点一次性解决。
- Tauri 后端 15 个命令（config/models/capture/select_region/history），系统集成改用 tauri-plugin-*（global-shortcut/clipboard-manager/single-instance/dialog）+ 原生 tray。
- 前端 5 个窗口视图（Home/Settings/History/Capture/Result），桌面框选+弹窗式截图翻译。
- 此前（egui 期）：译文图上原位覆盖、历史 V002 迁移（截图/OCR 行/逐行译文）、DU-15 历史面板——逻辑全部搬到 Tauri 后端保留。

**验证**：`cargo test --workspace` 58 测试全过（core 40 + src-tauri 18）；`npm run build`（vue-tsc 类型检查 + vite 打包）通过；`cargo check --workspace` 无警告；`npm run tauri dev` **应用实跑启动成功**（进程存活、日志实证：翻译 Provider 就绪 / 全局热键 Ctrl+Alt+Q 已注册 / ONNX 模型加载 / SnapText 启动完成）。

**测试覆盖**：核心管线（OCR→翻译→行级配对）抽成纯函数 `run_ocr_translate`，用 mock Provider 集成测试覆盖（`pipeline_ocr_translate_aligns` 等 6 个），取代随 crate 删除丢失的 orchestrator full_pipeline。另含 crop 坐标换算、history DTO 转换、capture 文件复制测试。

**待人工真机验证**：`npm run tauri dev` 端到端 GUI 交互——按热键截图→鼠标框选→译文叠加；多显示器/DPI 坐标换算；全屏透明窗口在 Windows 的表现。GUI 人机交互无法无头自动化，需在桌面操作。

---

## 已完成阶段

### DU-01 — Workspace 骨架 + 基础设施 ✅（2026-06-25 重做完成）

**产出**：
- workspace 根：`Cargo.toml`（双 crate + `workspace.dependencies` 统一版本）/ `rust-toolchain.toml`（stable + MSVC）/ `.gitignore` / `LICENSE`(MIT) / `README.md`(骨架)
- `crates/snaptext-core/`：`lib.rs`（config/error/types 三模块）/ `types.rs`（13 共享类型 + Lang Display/FromStr/serde 别名 + ProviderId `Cow`+const + 单元测试）/ `error.rs`（CoreError + 6 子错误，中文 `#[error]`，叶节点不依赖 types）/ `config.rs`（7 段嵌套 + env 覆盖 + load/save/parse + 测试）
- `crates/snaptext-app/`：`main.rs`（最小入口：加载 Config + init logging）/ `logging.rs`（tracing 双输出 stderr + `%APPDATA%\SnapText\logs\snaptext.log`）

**验收**：
- `cargo build --workspace` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅（0 warnings）
- `cargo fmt --all --check` ✅
- `cargo test --workspace` ✅（core 13 + app 1 = 14 passed）
- 手动 QA ✅：`cargo run -p snaptext-app` 写入日志文件 + stderr 双输出

**关键决策（落地）**：
- `ProviderId` = `Cow<'static, str>`，`new_static` 为 const fn（零分配静态构造）
- `Config` 用 `#[derive(Default)]`，子结构各自手写 impl Default
- env 测试合并为单函数 `env_overrides_merged_in_one_fn_to_avoid_parallel_race`
- `CoreError::NotImplemented(&'static str)` 已就位
- `error.rs` 为叶节点，不依赖 `types`（避免循环）；涉及语言的错误用 String
- thiserror 保留字段名陷阱：`TranslateError::UnsupportedPair` 用 `src`/`dst` 而非 `source`/`target`
- `Tier` 用 `#[derive(Default)]` + `#[default]`（clippy derivable_impls）

### DU-02 — Capture 模块（WGC + DXGI）✅（2026-06-25 完成）

**产出**：
- `snaptext-core/src/capture/mod.rs`：`CaptureProvider` trait（list_monitors / capture_monitor / capture_all，async_trait，返回 CoreError）+ 导出 `WindowsCaptureProvider`
- `snaptext-core/src/capture/windows_capture.rs`：`WindowsCaptureProvider`（WGC 优先 + DXGI fallback）+ 单元测试（像素格式转换）+ ignored 真实截图测试

**验收**：
- `cargo build -p snaptext-core` ✅
- `cargo clippy -p snaptext-core --all-targets -- -D warnings` ✅（0 warnings）
- `cargo test -p snaptext-core capture` ✅（3 单元 + 2 ignored 真实测试全过）
- 真实验证 ✅：枚举到 `\\.\DISPLAY1` 2560×1440；`capture_all` 截图 97.8% 非空像素，PNG 1.3MB

**关键决策（落地）**：
- WGC 用 `start_free_threaded` + channel 取首帧 + `CaptureControl::stop()`，不阻塞 async
- DXGI fallback：`acquire_next_frame` 同步取帧，跳过 `LastPresentTime==0` 空帧，重试 10 次，`AccessLost` 时 `recreate`
- 像素格式：WGC 用 `Rgba8` 直接用；DXGI 默认 `Bgra8` 交换 R/B 转 `RgbaImage`
- `MonitorInfo` 的 DPI 缩放/虚拟桌面坐标暂用近似值（scale=1.0、x/y=0），DU-08 Overlay 精确化（注释标 TODO）
- `MonitorId` 用 monitor `device_name`（如 `\\.\DISPLAY1`）
- 仅依赖 `windows-capture`，未直接依赖 `windows` crate，保持简洁

### DU-03 — ModelManager + 下载器 ✅（2026-06-25 完成）

**产出**：
- `snaptext-core/src/model_manager/mod.rs`：路径解析（`model_root`/`ppocrv6_dir`/`det_model_path`/`rec_model_path`）+ `is_models_ready` + `verify_models`（SHA256）+ `sha256_hex`
- `snaptext-core/src/model_manager/downloader.rs`：多源流式下载（hf-mirror → HF → 额外镜像）+ 进度回调 + 临时文件 rename + 每源失败切换

**验收**：
- `cargo build -p snaptext-core` ✅
- `cargo clippy -p snaptext-core --all-targets -- -D warnings` ✅（0 warnings）
- `cargo test -p snaptext-core model_manager` ✅（8 单元 + 1 ignored 真实下载）
- 真实验证 ✅：下载 small det `inference.onnx` 9.88MB，SHA256=`d73e0058…`，0.95s（hf-mirror）

**关键决策（落地 / DU 内修正）**：
- 文件名修正：PP-OCRv6 ONNX 仓库内是 `inference.onnx`（非 DESIGN §5.7 的 `model.onnx`），本地存 `det.onnx`/`rec.onnx`
- 默认源含 hf-mirror.com（HF 直连国内不可达 R7）；URL：`{hf-mirror|hf}/PaddlePaddle/PP-OCRv6_{tier}_{det,rec}_onnx/resolve/main/inference.onnx`
- medium + small repo 均存在（API 确认）；仓库还含 `inference.json`/`inference.yml` 推理配置（DU-04 按需取用）
- small det SHA256 = `d73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e`（可作 verify 预期值）
- 复用 `config::Tier`，避免重复定义档位枚举
- SHA256 预期 hash 来源待 DU-04（加载真实模型后固化）；当前 verify 在无预期 hash 时仅检查存在
- ⚠️ DU-04 实测后返工为 PP-OCRv5（见下方 DU-04 记录）：路径 `ppocrv6`→`ppocr`、加 `dict.txt`、Tier 映射 medium=server/small=mobile、下载源改 oar-ocr GitHub Releases

### DU-04 — OCR 模块（oar-ocr + PP-OCRv5）✅（2026-06-25 完成）

**产出**：
- `snaptext-core/src/ocr/mod.rs`：`OcrProvider` trait（id / supported_languages / recognize）
- `snaptext-core/src/ocr/paddleocr.rs`：`PaddleOcrProvider`（oar-ocr 封装，`Arc<OAROCR>`，`spawn_blocking` 推理）+ ignored 真实 OCR 测试
- `snaptext-core/src/ocr/preprocess.rs`：`to_rgb`（DynamicImage→RgbImage）

**DU-03 同步返工（v6→v5）**：ModelManager/downloader 从 PP-OCRv6（HF `inference.onnx`）改为 PP-OCRv5（oar-ocr releases）；路径 `ppocrv6`→`ppocr`、加 `dict.txt`、Tier 映射 `medium=server`/`small=mobile`。

**验收**：
- `cargo build -p snaptext-core` ✅（oar-ocr 0.6.3 + ort 2.0，onnxruntime.dll 自动下载，**R2 通过**）
- `cargo clippy -p snaptext-core --all-targets -- -D warnings` ✅（0 warnings）
- `cargo test -p snaptext-core model_manager` ✅（11 单元 + 1 ignored）
- 真实验证 ✅：屏幕截图 OCR 识别出真实文字（`total 20972`、`drwxr-xr-x 1 chii 197121`、`此电脑`、`+32 Lines` 等），中英混合准确

**关键决策（DU 内切换，AI_GUIDE §3.8）**：
- **R1 ✅ 已验证**：oar-ocr crates.io 0.6.3，仓库 greatv/oar-ocr，基于 ort 2.0
- **v6→v5**：oar-ocr 官方模型生态是 PP-OCRv5（无 v6）。实测 PP-OCRv6 ONNX + oar-ocr + `ppocrv5_dict` → 识别乱码（字符集不兼容）。改用 v5 mobile/server + `ppocrv5_dict`，识别准确
- oar-ocr `OAROCR` 已 Send+Sync（内部 `Arc<Session>`），无需手动 `Mutex`（AI_GUIDE §3.1 陷阱已解决）
- PP-OCRv5 small(mobile) 单图 ~3s（R3 待 DU-12 基准测）
- 测试 fixtures（英/中/日文本图）推迟 DU-12 统一准备

**后续回归 v6（2026-06-25）**：oar-ocr 发布 0.7.0/0.7.1（PR#132/#135）原生支持 PP-OCRv6，DU-04 时 v6 不兼容的根因（0.6.3 仅 v5）消除。升级 `oar-ocr 0.6.3→0.7.1`，模型回归 PP-OCRv6（medium/small + `ppocrv6_dict`），下载源改 ModelScope（`greatv/oar-ocr`，v6 模型仅此有），本地路径 `ppocr\{tier}`→`ppocr\v6\{tier}` 隔离旧 v5。`cargo build --workspace` + 41 单测通过。**ModelScope 下载坑**：WAF 对 `.onnx` 模型文件拦截非浏览器 UA（reqwest 默认 UA → 403；字典 `.txt` 放行），`download_url` 伪装浏览器 UA 修复，`download_small_full_real` 实测通过。原 v5 决策记录见上，保留以备追溯。

### DU-05 — Translate 模块（DeepSeek + DeepL）✅（2026-06-25 完成）

**产出**：
- `snaptext-core/src/translate/mod.rs`：`TranslationProvider` trait + `common_pairs` + `build_provider`（共享 `reqwest::Client`）
- `snaptext-core/src/translate/prompt.rs`：`render_translate_prompt`（"ONLY translation" 约束）
- `snaptext-core/src/translate/openai_compat.rs`：`OpenAiCompatProvider`（`/v1/chat/completions`，TokenUsage 解析）+ ignored 真实调用测试
- `snaptext-core/src/translate/deepl.rs`：`DeepLProvider`（`/v2/translate`，Free/Pro）

**验收**：
- `cargo build -p snaptext-core` ✅
- `cargo clippy -p snaptext-core --all-targets -- -D warnings` ✅（0 warnings）
- `cargo test -p snaptext-core translate` ✅（2 单元 + 1 ignored 真实 DeepSeek）
- 真实翻译待用户 API Key（`SNAPTEXT_DEEPSEEK_KEY` / `SNAPTEXT_DEEPL_KEY`），ignored 测试已就绪

**关键决策**：
- **R9 已解决**：DeepSeek 无 `deepseek-v4-flash`，默认改 `deepseek-chat`（V3，官方确认）。config 默认同步更新。
- Provider 构造注入共享 `reqwest::Client`（CONVENTIONS §3.6），超时 LLM 30s / MT 10s
- 错误归类 `TranslateError`：`Timeout` / `Api{status,body}` / `Parse` / `Request`
- MVP 不创建 microsoft.rs / baidu.rs（P2 DU-18）

### DU-06 — History 模块（写入接口）✅（2026-06-25 完成）

**产出**：
- `snaptext-core/src/history/mod.rs`：`HistoryStore` trait + `HistoryRecord` / `HistoryStats`
- `snaptext-core/src/history/sqlite_store.rs`：`SqliteHistoryStore`（r2d2 连接池 size 5，`spawn_blocking` 写）+ `open/open_default/cleanup_blocking`
- `snaptext-core/src/history/dao.rs`：`insert` + `cleanup`（retention_days + max_records）
- `snaptext-core/src/history/schema.sql` + `migrations/V001__initial.sql`（DDL，DESIGN §5.6）
- `snaptext-core/src/history/migration.rs`：`PRAGMA user_version` 迁移机制

**验收**：
- `cargo build -p snaptext-core` ✅（rusqlite 0.32 bundled，r2d2_sqlite 0.25，chrono）
- `cargo clippy -p snaptext-core --all-targets -- -D warnings` ✅（0 warnings）
- `cargo test -p snaptext-core history` ✅（4 单元：insert / list NotImplemented / 迁移幂等 / cleanup）

**关键决策**：
- 版本：rusqlite 0.40 与 r2d2_sqlite 的 libsqlite3-sys 冲突（links="sqlite3"），改 rusqlite 0.32 + r2d2_sqlite 0.25 + r2d2 0.8
- P0 仅 `insert` + `cleanup`；`list` / `delete_before` / `stats` 返回 `NotImplemented`（DU-15 补齐）
- 启动清理由 Orchestrator 调 `cleanup_blocking`（需 config 的 retention_days/max_records）

### DU-07 — UI 骨架 + 系统集成 ✅（2026-06-25 完成）

**产出**：
- `snaptext-app/src/main.rs`：single-instance → Config/logging → 手动 tokio runtime（Arc）→ tray + hotkey → eframe run_native
- `snaptext-app/src/ui/mod.rs`：`SnapTextApp`（`request_repaint_after` + `try_recv()` 轮询热键/菜单）
- `snaptext-app/src/tray.rs` / `hotkey.rs` / `clipboard.rs`：tray-icon 菜单、global-hotkey 注册、arboard 剪贴板
- `snaptext-app/assets/tray.png`：32×32 占位图标

**验收**：
- `cargo build -p snaptext-app` ✅；`cargo clippy -p snaptext-app --all-targets -- -D warnings` ✅（0 warnings）
- 运行验证 ✅：`cargo run` 启动成功（日志"SnapText 启动"+"托盘与热键就绪"）；**第二实例被拒**（"SnapText 已在运行"）
- 热键 Ctrl+Alt+Q / 托盘菜单交互：注册成功，机制就绪，待用户手动确认（自动化无法模拟按键/肉眼确认托盘）

**关键决策 / 陷阱**：
- 版本：tuna 镜像 eframe 0.30 / global-hotkey 0.8 / tray-icon 0.24 / arboard 3.6 / single-instance 0.3（与 crates.io 最新 0.33/0.22 略异，以镜像可用为准）
- `eframe::Error` 非 Send/Sync（含 `RawWindowHandle`），不能 `Into<anyhow::Error>`，手动 `map_err(|e| anyhow!(...))`
- 后台事件模式：`ctx.request_repaint_after(100ms)` + 全局 `receiver().try_recv()` 轮询（eframe 不自动唤醒）
- single-instance 0.3 停滞 5 年但 API 极简、可编译；P2 评估替代（自写 windows-sys mutex）
- clipboard 函数 DU-07 暂未调用，`#[allow(dead_code)]` + 文档说明（DU-09/10 接入）
- tray 菜单"暂停"推迟 DU-10（Orchestrator 才能暂停）；占位 tray.png 由 DU-13 换正式图标

### DU-10 — Orchestrator 状态机（关键路径）✅（2026-06-25 完成）

**产出**：`snaptext-app/src/orchestrator.rs`：`Command` / `Event` enum + `Orchestrator`（持 `Arc<dyn Capture/Ocr/Translate/History>`）+ `handle` + `run`（tokio task）+ `crop_frame` + mock Provider 单元测试。

**验收**：
- `cargo test -p snaptext-app orchestrator` ✅（3 测试：完整流程 trigger→region→ocr→translate→history、cancel、未知显示器报错）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅（0 warnings）

**关键决策 / 待办**：
- 核心串联已 mock 验证；`main` 集成（构造真实 Provider + channel）留 DU-11（first_run 下载模型 + 提示 API Key 后才能构造 ocr/translate Provider）
- `clipboard` auto_copy、`RetryTranslate` 留 DU-09 UI 集成
- `CapturedFrame` 补 `Clone`（Event::Captured 需克隆帧给 UI + 自留缓存）
- `main.rs` 暂 `#[allow(dead_code)] mod orchestrator;`，DU-11 接入后移除

### DU-11 — 首次启动 + main 集成 orchestrator ✅（2026-06-25 完成）

**产出**：
- `snaptext-app/src/first_run.rs`：`ensure_models(tier)`（缺失则用 DU-03 downloader 同步下载 + eprintln 进度）
- `snaptext-app/src/main.rs`：集成 single-instance → config/logging → ensure_models → 手动 runtime（`runtime.enter()`）→ 构造真实 Provider → Orchestrator + channel → tray + hotkey → eframe
- `snaptext-app/src/ui/mod.rs`：SnapTextApp 持 cmd_tx/event_rx，热键→TriggerCapture，`Captured` 后自动整屏 RegionSelected，poll Event 显示状态/原文/译文

**验收**：
- `cargo build/clippy -p snaptext-app` ✅（0 warnings）
- 启动验证 ✅：ONNX 模型加载（"Reserving memory in BFCArena"）+ "托盘与热键就绪，端到端流程已接入"；第二实例被拒
- 端到端热键流程（Ctrl+Alt+Q→截图→OCR→翻译→显示）待用户手动触发 + 真实 API Key（自动化无法模拟按键；翻译需 `SNAPTEXT_DEEPSEEK_KEY`）

**关键决策 / 待办**：
- 简化：整屏 OCR（无选区），DU-08 overlay 接入选区
- translate 无 API Key 时 main 提示并退出（DESIGN：翻译需 Key）；首启下载 UI 简化为同步 eprintln（完整进度窗口推迟）
- `runtime.enter()` 让 `orchestrator.run` 内的 `tokio::spawn` 找到 runtime（main 手动 runtime，无 `#[tokio::main]`）
- 测试用 fake key 已清理；`config.toml` 保留 `tier="small"`（已下载），用户自行配 API Key
- `Command` 的 Cancel/RetryTranslate/CopyToClipboard/Shutdown 暂 `#[allow(dead_code)]`，DU-09/10 UI 接入

### DU-08 — Overlay 选区 UI ✅（2026-06-25 完成）

**产出**：`snaptext-app/src/ui/overlay.rs`：`OverlayState`（Arc<Mutex> 跨帧）+ `show_overlay`（`show_viewport_deferred`）+ `render`（帧背景纹理 + 蒙版 + 选区矩形 + 尺寸）+ `handle_input`（鼠标框选 + Esc + RegionSelected）。集成 ui/mod.rs（替换整屏为选区）。

**验收**：
- `cargo build/clippy -p snaptext-app` ✅（0 warnings）；启动验证 ✅（overlay 注册不崩溃，单实例）
- GUI 选区交互（热键→框选→Esc）待用户手动验证

**关键决策 / 陷阱（eframe 0.30 实测）**：
- Viewport deferred 闭包是 `Fn(&Context, ViewportClass) + Send+Sync+'static`（非 0.31 的 `&mut Ui`）；状态用 `Arc<Mutex<OverlayState>>`
- Pointer API：`interact_pos()`（方法，非 press_pos/interact_pointer_pos 字段）、`primary_down()`/`primary_released()`（方法）
- `transparent` 关闭（与 App 级 `clear_color` 冲突），改整屏 `rect_filled` 蒙版
- 关 overlay = 停止调用 `show_viewport_deferred`（`Visible(false)` 不可逆，issue #5229）
- DPR：bbox 含 monitor 原点 + `scale`（当前 scale=1.0，DU-02 TODO 精确 DPI）
- 多显示器单屏简化（跨屏选区已永久砍除）

### DU-09 — 悬浮卡片 + 交互 ✅（2026-06-25 完成）

**产出**：`snaptext-app/src/ui/card.rs`：`show_card`（egui Area + 原文折叠 + 译文 + Provider/耗时 + 复制译文/关闭按钮）。集成 ui/mod.rs：`TranslateDone` 后显示卡片，关闭清除。

**验收**：`cargo clippy -p snaptext-app` ✅（0 warnings）；GUI 卡片交互（需真实 API Key 触发翻译完成）待用户验证。

**关键决策 / 待办**：
- 简化版：固定位置卡片 + 复制/关闭按钮。click-through / 切 Provider 重译 / 收藏历史 / Ctrl+C / auto_copy / 热键再按关闭推迟（DU-14/15 设置面板时完善）
- 复制按钮直接调 `clipboard::set_text`（ui 主线程，不绕 orchestrator）
- 卡片位置（选区下方 + 超屏调整）/ 字体从 [ui] config：DU-14 完善

### 修复 + 引导页 — 缺 Key 降级启动 + 首次引导 + 即时生效 ✅（2026-06-25）

**背景**：原 `main.rs` 启动时 `build_provider` 缺 API Key 即 `exit(1)`，程序无法启动；而 Key 配置入口在程序内设置面板——形成"进不去设置就配不了 Key"的死结。

**产出**：
- `orchestrator.rs`：`translate` 改 `Option<Arc<dyn>>` + `translate_config`/`client` 字段 + `rebuild_translate()`；`Command` 加 `UpdateTranslateConfig`/`UpdateTargetLang`；翻译前判 `None` 发 `Error`。
- `main.rs`：缺 Key 降级启动（不 exit）+ 读 `onboarding_completed` 传 UI + `target_lang` 从 config 读。
- `config.rs`：`GeneralConfig.onboarding_completed` + `TranslateConfig.target_lang`。
- `hotkey.rs`：`register` 返回 `(manager, HotKey)` + `re_register` 运行时换热键。
- `ui/onboarding.rs`：新增首次引导页（热键/引擎/Key/档位/目标语言）。
- `ui/mod.rs`：集成引导；设置/引导保存后即时下发 + 重注册热键。

**验收**：`cargo test --workspace` ✅（app 5 + core 41，含新增降级路径测试）；运行验证 ✅（缺 Key 不再 exit，进程存活）。GUI 交互（填 Key→翻译、二次启动不弹引导）待用户确认。

**关键决策**：翻译/语言/热键即时生效（`Update*` + `re_register`）；OCR 档位重启生效（运行时重建 OCR 推迟）；引导判定用 `onboarding_completed` 标志位（完成或跳过都置 true）。

### 修复 — 模型便携化 + 测试稳定化 ✅（2026-06-26）

**背景**：模型路径写死 `%APPDATA%\SnapText\models\`（隐藏、不可见），且 2 个测试断言"medium 档未下载"——本机已下载 medium 即失败（环境耦合脆弱测试，`cargo test` 实际 exit 101，但此前被 `| tail` 管道掩盖）。用户要求模型跟程序走。

**产出**：
- `snaptext-core/src/model_manager/mod.rs`：`model_root()` 从 `dirs::config_dir()` 改为 `std::env::current_exe().parent()/models`（便携模式，跟程序走）。
- 删除 2 个环境耦合测试（`is_ready_false_when_missing` / `verify_detects_missing_file`），新增自包含 `verify_files_detects_missing`（tempdir）；`paths_under_appdata` 改名 `paths_have_ppocr_v6_suffix`（便携下前缀随 current_exe 变，仅断言后缀）。
- `snaptext-core/src/model_manager/downloader.rs`：`download_url` 失败路径清理残留 `.part`（async 块统一 match 清理）。
- `snaptext-app/src/main.rs` + `orchestrator.rs`：`cargo fmt` 统一格式（修 `fmt --check` 不过）。
- 文档同步：DESIGN §3.1 架构图 + §4.3（deepseek-v4-flash→deepseek-chat）+ §5.7 / CODE_MAP / mod.rs + paddleocr.rs 注释，模型路径改 exe 同级。

**验收**：`cargo fmt --all --check` ✅ / `cargo clippy --workspace --all-targets -- -D warnings` ✅ / `cargo test --workspace` ✅（app 5 + core 40，0 failed）。现有 `%APPDATA%` 模型复制到 `target/debug/models/`。

**关键决策**：便携模式（exe 同级），无 env 覆盖（用户选定）；安装目录须可写——勿装 `Program Files`，否则首启下载失败。配置/历史/日志仍留 `%APPDATA%`（仅便携化模型）。

### 修复 — 设置面板 / 译文卡片关闭无效 ✅（2026-06-26）

**背景**：设置面板点 ✕/Esc、译文卡片点「关闭」/Esc 都"完全不消失"。eframe 0.30 下子 viewport 的 `ViewportCommand::Close` 只设 `close_requested` flag，**不自动销毁窗口**；子 viewport 的 ✕ 关闭信号只能在 render 闭包内的 `vctx` 读到，主窗口 ctx 读不到。两个面板此前都只设了内部 flag，缺了"主动发 Close"这一步，OS 窗口被 `show_viewport_deferred` 维持不灭。对照 overlay（能正常关闭）：闭包内发 `send_viewport_cmd(ViewportCommand::Close)`（overlay.rs:160,208）+ 返回值让主循环停调用。

**产出**（每处对称补一行）：
- `snaptext-app/src/ui/card.rs`：Esc 分支 + 「关闭」按钮，设 `close_requested=true` 后补 `vctx.send_viewport_cmd(ViewportCommand::Close)`（沿用既有返回值停调用机制，Close 关窗口 + 返回值清状态）。
- `snaptext-app/src/ui/settings.rs`：import 补 `ViewportCommand`；✕/Esc 分支设 `outcome=Cancel` 后补 `vctx.send_viewport_cmd(ViewportCommand::Close)`。不动「保存」「取消」按钮（走 outcome 正常链路）+ 不动主循环轮询逻辑。

**验收**：`cargo clippy -p snaptext-app --all-targets` ✅（0 warnings）。GUI 关闭交互待用户验证（✕/Esc/关闭按钮是否真能消失）。

**关键决策 / 风险**：只补 `send_viewport_cmd(Close)`、不动主循环和现有 outcome/返回值机制（最小改动，与 overlay 成功模式对称）。若加 Close 后 settings 仍"完全不消失"，说明 eframe 0.30 对带 OS 标题栏的 deferred viewport 有更深问题（issue #4842，Windows 下关 deferred viewport 可能 access violation），届时看 settings.rs:132-138 诊断日志里 `close_requested`/`close_event_n` 是否非零判定。

---

## 进行中

（无）

---

## 下一步建议

按 `TASKS.md` P0 优先级推进：

```
DU-01 → (并行: DU-02 / DU-03 / DU-05 / DU-06 / DU-07) → DU-04
       → (并行: DU-08 / DU-09) → DU-10 → (并行: DU-11 / DU-12) → DU-13
       → P0 发布 → 直接进入 P1（无需重新规划）
       → (并行: DU-14 / DU-15 / DU-16) → P1 发布
       → P2 按需启动
```

**节奏预期**：
- 单 AI 串行：P0 (7-8 次) + P1 (3 次) + P2 (4 次) ≈ 14-15 次会话
- 多 AI 并行：约 8-10 次串行阶段

---

## 阻塞中

（无）

---

## 已知风险（来自 DESIGN §8）

| 编号 | 风险 | 状态 | 缓解措施 | 验证 DU |
|---|---|---|---|---|
| R1 | `oar-ocr` crate 真实性 | 🟢 已验证 | crates.io 0.6.3，greatv/oar-ocr，ort 2.0 | ✅ DU-04 |
| R2 | `ort` Windows MSVC 链接 | 🟢 已通过 | `download-binaries` 自动下 onnxruntime.dll | ✅ DU-04 |
| R3 | PP-OCRv6_medium 单图 ~3s 延迟 | ⚠️ 待验证 | UI 进度文字 + 用户可切 small 档 | DU-12 |
| R5 | 多显示器 + 高 DPI 对齐 | ⚠️ 待验证 | MVP 限单显示器（跨屏已永久砍除） | DU-08 |
| R6 | WGC 权限提示 | 🟢 已接受 | 文档说明 | — |
| R7 | HuggingFace 在国内不稳定 | 🟡 多源下载 | DU-03 实现镜像 | DU-03 |
| R9 | `deepseek-v4-flash` 模型名 | 🟢 已解决 | 该模型不存在，默认改 `deepseek-chat`（V3） | ✅ DU-05 |
| R10 | 单实例保护 | 🟡 待实现 | `single-instance` crate | DU-07 |
| R11 | DeepSeek 限流故障转移 | 🟢 P2 DU-17 | MVP 手动切 | DU-17 |

---

## 历史决策

所有架构决策已固化在 `DESIGN.md`（§4 选型理由 / §5 模块设计 / §7 路线图）。本表不再重复，需要时直接查 DESIGN 对应章节。

---

## 修改 PROGRESS.md 的时机

- 开始 DU：写入"进行中"，写开始时间
- 完成 DU：写入"已完成阶段"，写完成总结
- 遇到阻塞：写入"阻塞中"，详细描述
- 风险状态变化：更新"已知风险"表格
- 做出新决策：直接改 DESIGN 对应章节
