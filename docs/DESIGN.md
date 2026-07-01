# SnapText 设计方案

| 项目 | 内容 |
|---|---|
| 文档版本 | v0.2（精简版，砍 ADR / 配置示例 / 目录结构 / 参考资料） |
| 日期 | 2026-06-25 |
| 状态 | 已与用户对齐，待实施 |
| 适用范围 | Windows 11 桌面截图 OCR + 翻译工具 |

> 本文档遵循 `AGENTS.md` 中的开发规范。

---

## 🤖 AI 协作导航（先读这里）

| 顺序 | 文档 | 解决的问题 |
|---|---|---|
| 1 | [`../AGENTS.md`](../AGENTS.md) | 用户全局规则（最高优先级） |
| 2 | [`PROGRESS.md`](PROGRESS.md) | 当前进展到哪一步 |
| 3 | [`TASKS.md`](TASKS.md) | 可领取的交付单元（DU） |
| 4 | [`CODE_MAP.md`](CODE_MAP.md) | 改 X 看哪些文件、不能动哪些 |
| 5 | [`CONVENTIONS.md`](CONVENTIONS.md) | 项目特定强制约定 |
| 6 | [`AI_GUIDE.md`](AI_GUIDE.md) | 项目特定陷阱 + 实施模式 |
| 7 | [`GLOSSARY.md`](GLOSSARY.md) | 项目特定术语表 |
| 8 | 本文档 | 高层设计与完整架构 |

**配置示例**：见 `crates/snaptext-core/src/config.rs` 的 `Default` 实现。
**目录结构**：见 `CODE_MAP.md`。
**架构决策**：直接看本文档 §4，无独立 ADR 文件。

### 文档同步铁律

| 代码变更 | 必须更新 |
|---|---|
| 新增/删除/重命名文件 | `CODE_MAP.md` |
| 修改 trait 签名 | `CODE_MAP.md` + 本文档对应章节 |
| 完成一个 DU | `TASKS.md`（标 [x]）+ `PROGRESS.md` |
| 改变架构决策 | 直接修改本文档对应章节 |
| 引入新术语 | `GLOSSARY.md` |

---

## 1. 项目概述

**SnapText** 是一个 Windows 11 桌面工具，提供 Snipaste 风格的截图 → OCR → 翻译一体化体验：

- 用户按下热键 → 屏幕变暗进入选区模式
- 鼠标框选文本区域 → 本地 OCR 识别
- 译文以悬浮卡片形式原位显示，可一键复制

**目标用户**：需要频繁阅读外文（英文 / 日文）文档、网页、PDF 的中文用户。

**核心价值**：隐私（OCR 本地）、低成本（默认 DeepSeek）、可控（Provider 可切换）。

---

## 2. 需求摘要

| 维度 | 决策 | 备注 |
|---|---|---|
| 目标平台 | Windows 11 x64 | 不支持 Win10 / ARM64 |
| 使用场景 | 网页 / PDF / 文档普通文本 | 漫画竖排、游戏画面不在范围 |
| 翻译方向 | 英→中、日→中、日→英 | 主选 |
| OCR 方案 | 本地 ONNX，PP-OCRv6 **medium + small 两档，默认 medium** | 完全离线 |
| 翻译方案 | 云 API，默认 DeepSeek；MVP 仅 DeepSeek + DeepL | trait 抽象 |
| 交互方式 | Snipaste 风格：热键 → 框选 → 悬浮卡 | 热键用户可配置 |
| 离线能力 | 不强求（联网优先） | OCR 本地，翻译走云 |
| 历史记录 | MVP 内置 sqlite 写入 | 读取接口随 P1 DU-15 |
| 分发形态 | 现代 Win11 PC，单 exe + 首启下载模型 | 总磁盘 ~200MB |

---

## 3. 总体架构

### 3.1 模块拓扑

```
┌──────────────────────────────────────────────────────────────┐
│  src/ — Vue 3 前端（Naive UI，webview 渲染）                   │
│  ├─ views/Home|Settings|History（主窗口路由）                 │
│  ├─ views/Capture（选区窗口：Canvas 框选）                     │
│  ├─ views/Result（结果窗口：图上译文叠加）                     │
│  └─ api.ts（invoke 封装）→ ──┐                                │
└──────────────────────────────┼───────────────────────────────┘
                               │ Tauri IPC（invoke / event）
┌──────────────────────────────▼───────────────────────────────┐
│  src-tauri/ — Rust 后端（Tauri 2）                             │
│  ├─ commands/（#[tauri::command]：config/capture/ocr_translate │
│  │            /history/models）                                │
│  ├─ state.rs（AppState：持 Provider 句柄 + Config）            │
│  ├─ window.rs（选区/设置/历史窗口 + 系统托盘）                 │
│  └─ main.rs（Builder + setup + 插件：热键/单实例/剪贴板/dialog）│
└──────────────────────────────┬───────────────────────────────┘
                               │ snaptext-core（workspace 依赖）
┌──────────────────────────────▼───────────────────────────────┐
│  snaptext-core — 纯逻辑库（100% 复用，平台无关）                │
│  ├─ capture（WGC/DXGI）  ocr（oar-ocr/PP-OCRv6 ONNX）         │
│  ├─ translate（OpenAI 兼容/DeepL/Microsoft，reqwest）          │
│  ├─ history（sqlite + r2d2，V002 schema）                      │
│  └─ model_manager / config / types                             │
└──────────────────────────────────────────────────────────────┘
                  %APPDATA%\SnapText\
                  ├─ config.toml
                  ├─ history.db          (sqlite, V002)
                  ├─ logs\snaptext.log
                  └─ cache\tmp\*.png     (截图临时文件)

  模型（便携，跟 exe 同级）：<exe 目录>\models\ppocr\v6\{tier}\{det,rec}.onnx + dict.txt
```

### 3.2 进程与线程模型

**单进程**，Tauri 后端使用 Tauri 内置的 tokio runtime（前端在 webview）：

| 线程 | 职责 | 关键约束 |
|---|---|---|
| Main thread | Tauri 事件循环 + 窗口/托盘/热键 | Win32 消息循环必须在此线程 |
| Tokio worker × N | 命令的 async 执行（HTTP/文件 IO/调度） | 默认 `num_cpus` 个 |
| `spawn_blocking` | ONNX 推理（CPU 密集） | 不阻塞 reactor |
| 模型下载专用线程 | `download_models` 独立线程 + 独立 runtime block_on | core 闭包非 Send，故隔离 |

**跨线程通信**：前端 ↔ 后端走 Tauri IPC（`invoke` 请求-响应、`emit`/`listen` 事件，如下载进度）。后端内部无 mpsc channel（取代旧 egui Orchestrator），命令直接读 `State<AppState>` 调 Provider。sqlite 访问经 `Arc<r2d2::Pool>`（`spawn_blocking` 内取连接）。

### 3.3 核心交互流程

无集中状态机（egui 期曾用 `AppState` enum，迁移 Tauri 后已删）。窗口各自管理状态，跨窗口靠"后端缓存 + 前端主动拉取"反竞态模式（见 §5.4/§5.5）：

```
热键 ──▶ trigger_capture_cmd（截图 + emit capture-ready）
  │
  ▼
Capture.vue（选区窗，常驻隐藏）收到事件 → draw 截图 → show
  │  鼠标抬起
  ▼
crop_region（裁剪 + 缓存 last_crop）→ 开 Result 窗 → 隐藏选区窗
  │
  ▼
Result.vue onMounted → recognize_region（OCR + 缓存 last_ocr）
  │
  ▼
translate_region（翻译 + 配对 + 落库）→ 原位显示译文
```

---

## 4. 技术栈选型（含决策理由）

### 4.1 总览

| 层 | 选型 | 决策理由 |
|---|---|---|
| 截图 | `windows-capture` | 双 API（WGC + DXGI），多显示器 + per-window 支持 |
| UI | **Tauri 2 + Vue 3 + Naive UI** | 迁移自 egui：UI 痛点（deferred viewport/Arc<Mutex> 借用）一次性解决；前端生态成熟，core 复用 |
| OCR 推理 | ONNX Runtime (`ort`) | 官方 PP-OCRv6 格式直接支持，Windows 打包最轻，CPU 性能稳 |
| OCR 封装 | **oar-ocr 优先；DU-04 内验证失败则同任务切 ort 自实现** | 避免第三方小众 crate 风险；trait 抽象保证切换零成本 |
| OCR 模型 | PP-OCRv6 medium + small 两档 | medium 精度最高，small 兼顾速度；详见 §5.2 |
| 翻译 SDK | `reqwest` + Provider trait | 统一抽象、统一重试/超时 |
| 系统集成 | `tauri-plugin-*`（global-shortcut/clipboard-manager/single-instance/dialog）+ Tauri 原生 tray | Tauri 统一托管，无需主线程消息循环手工集成 |
| 异步运行时 | `tokio`（Tauri 内置） | 生态最广 |
| HTTP | `reqwest` + `rustls` | 避免 OpenSSL 依赖 |
| 序列化 | `serde` + `toml` + `serde_json` | 标配 |
| 错误处理 | `anyhow`（应用）+ `thiserror`（库 trait） | 标配 |
| 日志 | `tracing` + `tracing-subscriber` | 异步友好，span 追踪 |
| 数据库 | `rusqlite` + `r2d2_sqlite` | 同步 + 连接池，适合桌面 |
| 打包 | **Tauri bundler**（NSIS + MSI） | 取代 cargo-wix；Tauri 自带 |

### 4.2 OCR 后端选型证据（PP-OCRv6_small, Intel Xeon 8350C, CPU）

| 后端 | 单图延迟 | Windows 打包 | v6 支持 | 结论 |
|---|---|---|---|---|
| **ONNX Runtime** | 0.61s | onnxruntime.dll ~25MB | ✅ 官方格式 | **采用** |
| OpenVINO | 0.59s（仅 Intel 略快） | Runtime 包大，AMD 弱 | 需转换 | 不采用 |
| MNN / NCNN | 慢于 ORT / x86 慢 2x | 静态库膨胀 | ❌ 官方未导出 | 不采用 |
| Paddle Inference | 0.79s | 动态库 >2GB | ✅ 原生 | 不采用（部署过重） |

### 4.3 翻译 Provider 选型

**MVP 范围（仅 2 个）**：

| Provider | 类型 | 默认 | 用途 |
|---|---|---|---|
| **DeepSeek** | LLM (OpenAI 兼容) | ✅ | 性价比之王，¥1/百万输入 token |
| **DeepL** | 专用 MT | — | 免费额度内质量天花板 |

**P2 扩展**（trait 已设计好）：OpenAI / Microsoft Azure / Baidu

**DeepSeek 模型**：不硬编码模型名。配置默认 `model = ""`（空），用户在设置页填 API Key 后点"刷新模型列表"动态拉取（`GET /v1/models`），从真实返回的 id 里选；下拉也允许手动输入（兼容第三方 OpenAI 兼容端点）。空 model 触发翻译时 Provider 直接报错"请先在设置选择模型"，不发无效请求。

#### DeepSeek API 事实基准

> ⚠️ **唯一依据是中文官方文档 `https://api-docs.deepseek.com/zh-cn/`。英文版（`api-docs.deepseek.com` 无 zh-cn）已过时，不作为依据。** 修改 DeepSeek 相关代码前必读本节。

**模型列表**：`GET {base_url}/models`（`/v1/models` 同义），返回 `{object:"list", data:[{id, object, owner}]}`。命令 `list_deepseek_models(base_url, api_key)` 包装它。

**思考模式**（V3.2 引入，**默认开启**）：
- 开关 = 请求体 `thinking` 参数：`{"thinking":{"type":"enabled"}}` 开 / `{"thinking":{"type":"disabled"}}` 关。
- 强度 = 请求体 `reasoning_effort` 参数：取值 `high`（默认）/ `max`；`low`/`medium` 映射到 `high`，`xhigh` 映射到 `max`。
- **互斥**：`thinking:disabled` 与 `reasoning_effort` 不能同时传。
- 思考过程走响应 `reasoning_content`（与 `content` 同级），翻译单轮场景忽略不展示。

**本实现的请求体组合**（`openai_compat.rs::translate`）：

| 配置（`DeepSeekConfig`） | 请求体注入 |
|---|---|
| `reasoning_enabled=false`（默认） | `thinking:{type:"disabled"}` |
| `reasoning_enabled=true` + `high` | `thinking:{type:"enabled"}` + `reasoning_effort:"high"` |
| `reasoning_enabled=true` + `max` | `thinking:{type:"enabled"}` + `reasoning_effort:"max"` |

翻译是简单请求场景，官方文档建议关闭思考以加速、降本，故**默认 `reasoning_enabled=false`**。

---

## 5. 关键模块设计

### 5.1 截图（Capture）

```rust
#[async_trait]
pub trait CaptureProvider: Send + Sync {
    async fn list_monitors(&self) -> Result<Vec<MonitorInfo>, CoreError>;
    async fn capture_monitor(&self, id: &MonitorId) -> Result<CapturedFrame, CoreError>;
    async fn capture_all(&self) -> Result<Vec<CapturedFrame>, CoreError>;
}
```

**默认实现**：`WindowsCaptureProvider`（WGC 优先 + DXGI fallback）。

**关键设计**：热键触发后**立即**对所有显示器各捕获一帧（<50ms），缓存到内存。选区 Overlay 直接把这帧绘制为背景，避免选区过程中屏幕内容变化。

**坐标系**（单屏已精确，多屏未完成）：截图帧为**物理像素**（windows-capture 取自 `dmPelsWidth/Height`），前端窗口为逻辑像素。`MonitorInfo.scale = GetDpiForMonitor/96.0`，前端用 `物理 = 逻辑 × scale` 换算框选 bbox；`MonitorInfo.x/y` 固定 0（多屏 origin 待实现）。`crop_frame`（src-tauri）把 bbox clamp 到图像边界，越界返回 Err 而非 panic。

### 5.2 OCR

```rust
#[async_trait]
pub trait OcrProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn supported_languages(&self) -> &[Lang];
    async fn recognize(&self, img: &image::DynamicImage, lang: Lang) -> Result<Vec<OcrLine>, CoreError>;
}

pub struct OcrLine {
    pub text: String,
    pub bbox: Bbox,
    pub confidence: f32,
    pub writing_direction: WritingDirection,
}
```

**默认实现**：`PaddleOcrProvider`，委托 `oar-ocr` crate（如不可用同 DU 内切自实现 ort 管线）。

**档位支持**（详见 §4.1）：

| Tier | 单图 OCR（Intel Xeon, CPU） | 模型磁盘 | 内存峰值 | 含日文 |
|---|---|---|---|---|
| **medium**（默认） | ~3s | ~133MB | ~500MB | ✅ |
| small | ~0.6s | ~30MB | ~200MB | ✅ |

**模型路径**：`%APPDATA%\SnapText\models\ppocrv6\{tier}\{det,rec}.onnx`

**关键约束**：`ort::Session` 不是默认 Send，必须用 `Arc<Mutex<Session>>` 包装。详见 `AI_GUIDE.md §3.1`。

### 5.3 翻译（Translate）

```rust
#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn supported_pairs(&self) -> &[LangPair];
    async fn translate(&self, req: TranslateRequest) -> Result<TranslateResponse, CoreError>;
}

pub struct TranslateRequest {
    pub text: String,
    pub source: Lang,
    pub target: Lang,
    pub context_hint: Option<String>,
    pub glossary: Option<HashMap<String, String>>,
}
```

**MVP 实现（仅 2 个）**：
1. `OpenAiCompatProvider`（默认，含 DeepSeek）— 走 OpenAI 兼容 `/v1/chat/completions`
2. `DeepLProvider` — 走 DeepL REST API

**LLM 类 Provider 共用 prompt 模板**（`translate/prompt.rs`，**双模式可配置**）：

模板存于 `TranslateConfig.prompt_template`（顶层字段，DeepL/Microsoft 是专用 MT 不走 prompt，不受影响），通过 `TranslateConfig.prompt_use_custom` 切换两种模式：

| 模式 | `prompt_use_custom` | UI | 渲染数据源 |
|---|---|---|---|
| 系统默认（默认） | `false` | 只读展示后端常量 | 后端固定常量 `DEFAULT_PROMPT_TEMPLATE`（**不读字段**） |
| 自定义 | `true` | 可编辑 | `prompt_template` 字段 |

**为什么默认模式不读字段而读常量**：这样后端升级默认 prompt 时，所有默认模式用户自动受益——修了"配置固化"（用户一旦保存过 `prompt_template` 就拿不到未来升级）的隐患。默认模式下 `prompt_template` 字段值不参与渲染，但仍随 config 持久化（保留用户上次自定义，切回自定义模式不丢失）。

占位符用**双花括号** `{{source}}`/`{{target}}`/`{{input}}`——避免与用户原文里的 `{...}`（如 JSON/代码片段）冲突（Jinja2/mustache 惯例），渲染用 `str::replace` 不引模板引擎。

- **单一数据源**：`prompt.rs::DEFAULT_PROMPT_TEMPLATE` 常量是唯一真值。`config.rs` 默认值引用它；前端设置页经新命令 `get_default_prompt()`（`commands/config_cmd.rs`）拉取同一常量做只读展示——**前端零硬编码**，彻底消除两端不同步。
- **容错兜底**：用户模板若漏掉 `{{input}}`，渲染时自动在末尾追加原文，防模型拿不到源文本瞎编。
- **默认值**（英文系统指令——LLM 对英文更稳、token 更省；含角色设定 + 输出约束 + 保留换行/标点风格）：
```
You are a precise translator. Translate the text below from {{source}} to {{target}}.
- Output ONLY the translation.
- No explanations, quotes, or prefixes.
- Preserve line breaks and the original punctuation style.

{{input}}
```

**通用功能**：超时（LLM 30s，专用 MT 10s）+ 指数退避重试 2 次 + Token usage 解析。

**流式输出**：MVP 不启用（用户决策），等整段返回再渲染。

### 5.4 选区 Overlay（Snipaste 风格）

**生命周期**（三层命令分阶段反馈，框选抬起即开结果窗，OCR/翻译在结果窗内异步进行）：
1. 热键 → `trigger_capture_cmd` 先截图再开选区窗（Capture.vue）
2. Capture.vue 拉取缓存全屏图渲染，鼠标拖拽画矩形 → 实时刷新（蒙版还原矩形内 + 显示尺寸）
3. 鼠标抬起 → 调 `crop_region`（仅裁剪缓存帧 bbox 区 + 写临时 PNG，几十 ms）→ 立即创建结果窗口（Result.vue）→ 关闭选区窗
4. Result.vue `onMounted` 渲染原图 → 调 `recognize_region`（OCR + 后处理，顶部"正在识别…"）→ 图上按 OCR 行 bbox 原位擦白显示**原文**
5. → 调 `translate_region`（整段翻译 + `align_lines` 行配对 + 写历史，顶部"正在翻译…"）→ 原位替换为**译文**

**为什么三层命令而非一个大命令**：旧 `select_region` 把裁剪→OCR→翻译→配对→落库打包成一个命令，框选抬起后选区窗要 `await` 整个管线（几秒）才能开结果窗，期间用户只看到"识别中…"死等，体验差。拆成 `crop_region`/`recognize_region`/`translate_region` 后，抬起几十 ms 即弹窗显示原图，OCR/翻译转为结果窗内的分阶段进度，识别中间结果（原文）也可见。三层之间用 `state.last_crop`/`state.last_ocr` 接力，沿用"后端缓存+前端主动拉取"反竞态模式（不引入 Tauri 事件，避免子窗口未加载完事件丢失的竞态）。

**透明度处理**：选区阶段 `set_cursor_hittest(true)`（接收鼠标），显示卡片后 `set_cursor_hittest(false)`（穿透到下层）。

### 5.5 命令层（取代旧 Orchestrator）

egui 期曾有集中 `Orchestrator`（`Command`/`Event` enum + mpsc channel）。迁移 Tauri 后已删——Tauri 命令直接读 `State<AppState>` 调 Provider，无 channel。命令清单见 `CODE_MAP.md` src-tauri/commands 表（capture / ocr_translate / config_cmd / models / history 五组）。

**翻译降级与即时生效**：`AppState.translate` 为 `Mutex<Option<Arc<dyn TranslationProvider>>>`——启动时缺 API Key 则为 `None`（翻译时回错误提示去设置，不阻塞截图/OCR/设置面板）。设置页保存后 `save_config` 命令调 `build_provider` 即时重建并写回该 Mutex（无需重启）。

**OCR 降级（同款哲学）**：`AppState.ocr` 为 `Mutex<Option<Arc<dyn OcrProvider>>>`——启动时模型缺失（`PaddleOcrProvider::new` 失败）则为 `None`（不崩，`recognize_region` 命令取 None 时返回"OCR 模型未就绪，请先下载模型"）。引导页/设置页下载完模型后调 `reload_ocr_provider` 命令（用当前 tier 构造）即时重建写回，无需重启。三者（翻译缺 Key / 热键被占用 / OCR 缺模型）同构：缺资源不崩、降级运行 + UI 引导修复。

**热键注册降级（同款哲学）**：全局热键注册失败（典型：上一次进程残留未释放热键、或被其他软件占用）不阻断启动——`main.rs::setup` 注册失败时写入 `AppState.hotkey_error` 并继续，前端经 `get_hotkey_status` 拉取后在 Home 弹一次性引导 + Settings 快捷键卡片标红，用户改键保存后 `save_config` 重注册成功即清空状态。与翻译降级同款"缺资源不崩、降级运行 + UI 提示引导修复"模式，也沿用 `captured`/`last_crop`/`last_ocr` 的"后端缓存状态 + 前端主动拉取"反竞态（不引入 Tauri 事件，因子窗口 Pinia 不共享、emit 可能丢失）。

**首启引导（`complete_onboarding` 命令）**：`GeneralConfig.onboarding_completed: bool`（默认 false）作单标志位。启动时 `Home.vue` 读该字段，false 则 `router.replace('/onboarding')` 进引导页。仅用户走完引导（完成/跳过）调 `complete_onboarding` 命令才置 true——该命令只置标志 + `cfg.save()` 落盘，**不复用 `save_config`**（语义不同：不重建 Provider、不重注册热键，因引导页配置草稿已在调用前由 `save_config` 落盘）。中途关闭/崩溃/下载失败都仍为 false → 下次重进引导页。引导页不记步骤进度（断点续传属过度设计），配置末尾统一保存一次（避免分步多次触发后端重注册热键副作用），模型下载靠 `is_models_ready()` 幂等检查（已就绪跳过，未就绪重下整个，不续传）。

**译文图上原位覆盖 + 行级配对**：OCR 行带 bbox 经 `recognize_region` 命令返回前端。整段翻译后，译文按 `\n` 切分与 OCR 行按 index 配对（`align_lines`：译文行多于原文并入末行、少于原文补空）。Result.vue 以选区裁剪图为背景，在每个 OCR 行 bbox 位置擦白后绘制该行译文——即微信截图翻译式原位覆盖。同一份数据（截图 PNG + ocr_lines + line_translations）写历史库供回看。

### 5.6 历史记录（History）

**写入 + 读取接口均已实现**（DU-06 写入 + DU-15 读取，含 V002 图像/行级扩展）。

`%APPDATA%\SnapText\history.db` (sqlite)，schema：

```sql
CREATE TABLE translation_history (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at      TEXT NOT NULL,
    source_lang     TEXT NOT NULL,
    target_lang     TEXT NOT NULL,
    original_text   TEXT NOT NULL,
    translated_text TEXT NOT NULL,
    provider        TEXT NOT NULL,
    model           TEXT,
    prompt_tokens   INTEGER,
    completion_tokens INTEGER,
    total_cost_cny_milli INTEGER,
    monitor_id      TEXT, bbox_x INTEGER, bbox_y INTEGER,
    bbox_w INTEGER, bbox_h INTEGER, notes TEXT,
    -- V002 字段（译文图上原位覆盖 + 历史回看）
    screenshot_png          BLOB,  -- 选区截图（PNG 压缩）
    ocr_lines_json          TEXT,  -- Vec<OcrLine> JSON（text+bbox+confidence+direction）
    line_translations_json  TEXT   -- Vec<String> 逐行译文 JSON（与 ocr_lines 按索引配对）
);
CREATE INDEX idx_history_created ON translation_history(created_at DESC);
```

迁移经 `PRAGMA user_version` 版本化（V001 建表、V002 加图像/行级字段），幂等。

**启动清理**：`state.rs::AppState::build` 构造 history 后，若 `config.history.auto_clean_on_start` 为 true，调一次 `cleanup_blocking(retention_days, max_records)` 删除过期/超量记录（清理逻辑在 core `dao::cleanup` 已实现，2026-06-30 才接线到启动流程）。

详见 `CODE_MAP.md` history 模块。

### 5.7 模型管理

**下载触发**：模型下载由前端引导页（`Onboarding.vue`）主动触发，**不在 `main.rs::setup` 启动时同步下载**（旧 `ensure_models` 已删——避免首启后台静默拉模型、用户无感知不可选）。引导页进步先调 `is_models_ready()` 幂等检查，已就绪跳过，未就绪点"下载"调 `download_models` 命令（后台线程 + `download-progress`/`download-done` 事件推送进度）。设置页诊断也可手动触发下载。

首次启动从 ModelScope 下载（`greatv/oar-ocr` 仓库，国内直连），含 SHA256 校验。
URL：`https://www.modelscope.cn/models/greatv/oar-ocr/resolve/master/pp-ocrv6_{tier}_{det,rec}.onnx` + `ppocrv6_dict.txt`（v6 模型仅 ModelScope 有；oar-ocr GitHub Releases 仅 v3-v5）。
本地目录：可执行文件同级的 `models\ppocr\v6\{tier}\`（**便携模式**，模型跟程序走；`v6` 段隔离历史版本）。开发运行时位于 `target\{debug,release}\models\`，安装后位于安装目录。⚠️ 安装目录须可写——勿装到 `Program Files`，否则首启下载会因普通用户无写权限而失败。

### 5.8 UI 层（Vue 3 + Naive UI）

多窗口共用一套路由表，靠 hash 路由（`#/home`/`#/onboarding`/`#/settings`/`#/history`/`#/capture`/`#/result`）区分窗口内容。浅色主题（固定，不做切换），全局 CSS 变量在 `src/styles/global.css`，Naive UI 组件经 `unplugin-vue-components` + `NaiveUiResolver` 按需自动注册。

**选区窗口**（`Capture.vue`）：Tauri 全屏无边框置顶透明窗口，Canvas 渲染截图 + 鼠标拖拽框选。窗口在 main setup 预创建并隐藏（WebView2/Vue 常驻预热），热键时直接 show 省 ~400ms 冷启动。窗口以 hidden 创建，首次 `draw()` 画上截图 + 双层 `requestAnimationFrame` 等合成完才 show，消除创建→绘制间的白闪。截图经 `shot://` 自定义协议从内存直接取 BMP（不写临时文件）。

**结果窗口**（`Result.vue`）：Tauri 普通窗口，Canvas 以选区裁剪图为背景，按 OCR 行 bbox 擦白后绘制译文（原位覆盖）。两阶段渲染：原图 → "正在识别" → 原位显示原文 → "正在翻译" → 原位替换译文。工具栏：原文/译文切换、复制、保存、关闭。三层命令（`crop_region`/`recognize_region`/`translate_region`）在结果窗 `onMounted` 内分阶段触发，抬起几十 ms 即弹窗显示原图。

**设置面板**（`Settings.vue`）：普通窗口，左侧导航（8 分类）+ 右侧 `n-form` 分组卡片。草稿机制：编辑本地 draft 副本，保存时调 `save_config` 命令写盘 + 重建 Provider + 重注册热键；API Key 用 `n-input` 密码框。

**主窗口**（`Home.vue`）：状态卡（模型/翻译就绪态）+ 截图/设置/历史入口。热键注册失败时弹一次性 `message.warning` 引导。`onMounted` 判断 `onboarding_completed===false` 则 `router.replace('/onboarding')` 进引导页（首启场景）。

**首启引导**（`Onboarding.vue`，主窗口内路由非独立窗口）：三步向导（`n-steps`）——快捷键 → 下载 OCR 模型 → 翻译配置（可选）。复用 Settings.vue 的草稿机制（`reactive<Config>` 深拷贝 + 末尾统一 `save_config`）。下载步监听 `download-progress`（按 det/rec/dict 三段权重 33/47/20 折算总进度）/`download-done`，`onBeforeUnmount` 清理 unlisten 句柄（页面可销毁，与常驻的选区窗不同）。模型幂等检查（`is_models_ready`）已就绪跳过；下载失败显示错误 + 重试，不阻断下一步。完成/跳过调 `complete_onboarding` 置标志 → 回首页。

**关键决策**：选区/结果用独立 Tauri 窗口（非主窗口内组件），因全屏置顶选区需独立 OS 窗口。跨窗口数据不靠 Pinia（Tauri 多窗口 JS 上下文隔离、Pinia 不共享），改走"后端缓存状态（`state.captured`/`last_crop`/`last_ocr`）+ 子窗口 `onMounted` 主动命令拉取"反竞态模式。历史面板 + 关窗最小化逻辑（`minimize_to_tray_on_close`）已实现。

---

## 6. 数据流（一次完整调用）

三层命令分阶段反馈，框选抬起几十 ms 即弹结果窗显示原图，OCR/翻译在结果窗内异步进行（详见 §5.4）：

```
[User] 热键（默认 Ctrl+Alt+Q）
   │
   ▼
[Rust] trigger_capture_cmd：do_capture_all（截图 + 缓存帧进 state.captured）→ emit("capture-ready")
   │
   ▼
[Vue Capture.vue]（选区窗，常驻隐藏）收到 capture-ready → img.src = shot:// URI（从内存取 BMP）
   │               → draw() 画截图 + 双层 rAF → show()
   ▼
[Vue] 全屏 Canvas 显示截图 → 用户拖拽框选 → mouse_up
   │   bbox（物理坐标）= 屏内坐标 × scale + monitor 原点
   ▼
[Vue] invoke('crop_region', { monitor_id, bbox })
   │
   ▼  （仅裁剪 + 缓存 state.last_crop，几十 ms）
   │
[Vue] 创建结果窗口（label=result）→ 隐藏选区窗（常驻复用，不 close）
   │
   ▼
[Vue Result.vue] onMounted → 渲染原图（从 last_crop 取）
   │
   ▼
[Vue] invoke('recognize_region')  ← "正在识别…"
   │   Rust: 从 last_crop 取图 → ocr.recognize（spawn_blocking ONNX）→ 后处理 → 缓存 state.last_ocr
   ▼
[Vue] 图上原位显示原文 → invoke('translate_region')  ← "正在翻译…"
   │   Rust: 从 last_ocr 取原文 → provider.translate（缺 Key → 报错提示去设置）
   │        → align_lines 配对 → history.insert（截图 PNG + ocr_lines + line_translations 落库）
   ▼
[Vue] 原位替换为译文
      工具栏：原文/译文切换 · 复制 · 保存 · 关闭
```


---

## 7. P0/P1/P2 优先级路线图

**取消版本号规划**（原 v0.1/v0.2/v0.3/v1.0 改为优先级）。AI 协作时按 P0 → P1 → P2 顺序连续推进，每个 P 完成都有可用版本。

### P0（必做，发布门槛）— 13 DU

DU-01 ~ DU-13。详见 `TASKS.md`。完成即可发布首个可用版本。

### P1（应做，完整体验）— 3 DU

- DU-14：设置 GUI 面板（精简版：热键 / Provider 切换 / tier 运行时切换 / API Key / 4 UI 开关）
- DU-15：历史记录 GUI（精简版：列表 + 搜索 + 单删 + 清空）+ 读取接口
- DU-16：OCR + 译文后处理（去空格 / 合并换行 / 标点修正）

P1 三个 DU 可并行。完成即发布"完整体验"版本。

### P2（可做，扩展与工业级）— 4 DU

- DU-17：DeepSeek 故障自动转移（曾预写 `FallbackProvider` 骨架后删除求零死代码，做时重新实现）
- DU-18：OpenAI / MS / Baidu Provider
- DU-19：代码签名 + MSI 自动更新（个人用可跳过）
- DU-20：GPU 加速（DirectML EP，按需）

P2 各 DU 相对独立，根据用户实际需求选择做或不做。

### 永久砍除（不列入路线图）

| DU | 功能 | 砍除理由 |
|---|---|---|
| DU-22 | 术语表生效 | 小众需求 |
| DU-23 | 模型下载断点续传 | 用户未选 |
| DU-25 | API Key 加密存储 | 个人过度工程（Windows ACL 已够） |
| DU-26 | 日文竖排 | 用户主场景不含漫画 |
| DU-28 | 持续监控模式（字幕跟随） | 偏离核心定位 |

### 推进策略

- AI 按 P0 → P1 → P2 顺序连续推进，无版本阻断
- 任何时候停止都有可用版本
- 单 AI 串行：~14-15 次会话完成全部 20 DU
- 多 AI 并行：~8-10 次串行阶段

---

## 8. 风险与未决项

| # | 风险 | 当前方案 | 验证 DU |
|---|---|---|---|
| R1 | `oar-ocr` crate 真实性 | DU-04 内同任务验证 + 失败同任务切自实现 | DU-04 |
| R2 | `ort` Windows MSVC 链接 | `load-dynamic` feature，运行时加载 DLL | DU-01/04 |
| R3 | PP-OCRv6_medium 单图 ~3s 延迟 | UI 进度文字 + 用户可切 small 档 | DU-12 |
| R5 | Tauri 全屏选区窗口多显示器 + 高 DPI 对齐 | MVP 限单显示器内选区 | DU-08 |
| R6 | WGC 首次启动触发 Win11 屏幕捕获权限提示 | 文档说明 + 引导用户允许 | 不可避免 |
| R7 | ModelScope 在国内不稳定 | DU-03 多源下载（ModelScope 主源 + `extra_mirrors` 可选镜像前缀） | DU-03 |
| R9 | `deepseek-v4-flash` 模型名未确认 | **DU-05 内如失败立即切 `deepseek-chat`** | DU-05 |
| R10 | 单实例运行（防止多开冲突 hotkey） | `single-instance` crate | DU-07 |
| R11 | DeepSeek 限流故障转移 | P2 DU-17；MVP 手动切 | DU-17 |

---

## 9. P0 验收标准

- 在 1920×1080 Win11 上，从热键到译文显示总耗时 ≤ 5 秒（PP-OCRv6_medium + DeepSeek）
- 安装包 < 100MB
- 模型缓存：medium ~133MB（如同时装 small 则 +30MB）
- 内存峰值 < 1GB
- 连续 100 次框选：0 崩溃，内存增长 < 50MB
- 编译警告：0
- 测试覆盖：核心模块 ≥ 70%
