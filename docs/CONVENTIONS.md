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

- ❌ `snaptext-core` 不得依赖 `snaptext-app`
- ❌ `snaptext-core` 不得引入 UI 依赖（`eframe` / `egui` / `winit`）
- ❌ UI 层不得直接调 `reqwest` / `ort`，必须通过 trait
- ❌ `main.rs` 不得写业务逻辑，仅初始化
- ❌ 跨 DU 修改文件（你做 DU-05，不要改 DU-08 涉及的文件）

## 3. Provider 实现约定

所有 Provider（Capture / Ocr / Translate）必须：
- 实现 `Send + Sync`
- 用 `#[async_trait]`
- 返回 `Result<T, CoreError>`
- CPU 密集操作用 `tokio::task::spawn_blocking`
- HTTP 调用必须有超时（LLM 30s / 专用 MT 10s）
- 共享 `reqwest::Client`（不每次 new）

## 4. ort::Session 强制包装

PP-OCRv6 推理用 `ort::Session`。**默认不 Send**，必须：

```rust
pub struct PaddleOcrProvider {
    det_session: Arc<Mutex<Session>>,
    rec_session: Arc<Mutex<Session>>,
}
```

详见 `AI_GUIDE.md §3.1`。

## 5. Windows 主线程约束

以下 API 必须在主线程调用（winit event loop 所在线程）：

| API | 原因 |
|---|---|
| `winit::EventLoop` | Win32 消息循环 |
| `global-hotkey` 注册 | 同上 |
| `tray-icon` 创建 | 同上 |
| `arboard::Clipboard::set_text` | 剪贴板要求窗口线程 |

**模式**：tokio worker 通过 channel 把命令发给主线程；主线程在 egui frame callback 里 poll channel。

## 6. tokio runtime 构造

`main.rs` 必须**手动**构造 runtime（不用 `#[tokio::main]`）：

```rust
fn main() -> Result<()> {
    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    // runtime.handle().spawn(...) 提交任务
    // runtime 在 main 持有，eframe run 阻塞主线程
}
```

理由：eframe 的事件循环是同步阻塞的，`#[tokio::main]` 的 main future 不能阻塞主线程。

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
