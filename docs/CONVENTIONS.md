# CONVENTIONS — 项目特定强制约定

> 仅列**项目特定**约定，通用 Rust 规则（命名、错误处理、异步、测试）不再重述。
> 通用规则参考：`AGENTS.md` + Rust API Guidelines + `CONVENTIONS.md` 历史 v0.1（已删除）。
> 违反以下约定 = 退回重写。

最后更新：2026-06-24

---

## 1. 语言

- 代码注释、文档、commit message、日志文本：**中文**
- 标识符（变量、函数、类型、模块）：**英文**
- 用户可见字符串（UI 文案、错误提示）：**中文**
- 文件名：英文小写 + 下划线

## 2. 架构边界（不可逾越）

- ❌ `snaptext-core` 不得依赖 `src-tauri`
- ❌ `snaptext-core` 不得引入 UI/系统集成依赖（`tauri` / 前端框架 / 窗口 API）
- ❌ 前端（`src/`）不得直接调 `reqwest` / `ort`，必须经 Tauri 命令（`invoke`）
- ❌ `main.rs` 不得写业务逻辑，仅初始化 + 命令注册
- ❌ 跨 DU 修改文件（你做 DU-05，不要改 DU-08 涉及的文件）

## 3. Provider 实现约定

所有 Provider（Capture / Ocr / Translate）必须：
- 实现 `Send + Sync`
- 用 `#[async_trait]`
- 返回 `Result<T, CoreError>`
- CPU 密集操作用 `tokio::task::spawn_blocking`
- HTTP 调用必须有超时（LLM 30s / 专用 MT 10s）
- 共享 `reqwest::Client`（不每次 new）

## 4. ONNX 推理跨线程共享

PP-OCRv6 推理经 `oar-ocr` 封装（`OAROCR` 内部 `Arc<Session>`，已 `Send + Sync`），用 `Arc<OAROCR>` 跨线程共享，**无需 `Mutex`**：

```rust
pub struct PaddleOcrProvider {
    engine: Arc<OAROCR>,
}
```

详见 `AI_GUIDE.md §3.1`。

## 5. Windows 主线程约束（Tauri 接管）

以下系统集成由 Tauri 2 + 插件在主线程自动处理，**不要手动构造** EventLoop / runtime：

| 职责 | Tauri 接管方式 |
|---|---|
| Win32 消息循环 | `tauri::Builder::run`（内部事件循环） |
| 全局热键 | `tauri-plugin-global-shortcut`（注册失败降级，见 DESIGN §5.5） |
| 系统托盘 | `tauri::tray::TrayIconBuilder` |
| 剪贴板 | `tauri-plugin-clipboard-manager` |

## 6. tokio runtime（Tauri 内置）

`main.rs` 用 `tauri::Builder::default()`，runtime 由 Tauri 内置提供。`#[tauri::command] async fn` 自动跑在 Tauri 的 tokio runtime 上。**不要**手动构造 `tokio::runtime::Runtime`（例外：模型下载专用线程 `download_models` 因闭包非 Send，隔离用独立 runtime `block_on`）。

## 7. 单文件长度

- 单文件 > 400 行 = 重构信号（例外：`types.rs` / `config.rs` 集中定义）
- 单函数 > 50 行 = 重构信号（例外：明显 match 大分派）

## 8. 错误消息中文

所有 `#[error("...")]` 字符串中文（与 §1 一致）：

```rust
#[error("模型文件未找到：{path}")]
ModelNotFound { path: PathBuf },
```

## 9. 禁止反模式

- ❌ `as any` 等价：`unimplemented!()`, `todo!()`, `.unwrap()`, `.expect()`（非 test）
- ❌ `#[allow(dead_code)]` 跳过 lint（除非文档说明）
- ❌ 空 catch：`let _ = result;`
- ❌ `unwrap_or_default()` 吞错误
- ❌ hot path 分配大量 String / Vec（OCR 推理循环、UI 帧循环）
- ❌ 每次调用 new `reqwest::Client` 或 `ort::Session`
- ❌ 改 `AGENTS.md`（用户级规范）
- ❌ 改已发布的设计决策（要修订直接改 `DESIGN.md`）

## 10. 文档同步铁律

| 代码变更 | 必须更新 |
|---|---|
| 新增/删除/重命名文件 | `CODE_MAP.md` |
| 修改 trait 签名 | `CODE_MAP.md` + `DESIGN.md` 对应章节 |
| 完成 DU | `TASKS.md`（标 [x]）+ `PROGRESS.md` |
| 引入新依赖 | `Cargo.toml` 注释 + 必要时改 `DESIGN.md §4` |
| 改变架构决策 | 直接改 `DESIGN.md` 对应章节 |
| 引入新术语 | `GLOSSARY.md` |
