# AI_GUIDE — 给 AI 协作者的实施指引

> 这份文档专门给 AI 助手看，**仅列项目特定**信息（陷阱、约束、模式）。
> 通用规则见 `CONVENTIONS.md` + `AGENTS.md`。
> 读完本指南 + `CODE_MAP.md` + `TASKS.md` + `PROGRESS.md` 后，应能独立领取 DU 并产出可验收代码。

最后更新：2026-06-24

---

## 1. 启动 checklist（每次会话开头）

按序读：

1. `AGENTS.md`（用户全局规则，**最高优先级**）
2. `PROGRESS.md`（当前进展）
3. `TASKS.md`（找一个 `P0 待开始`的 DU）
4. `CODE_MAP.md`（涉及哪些文件）
5. `CONVENTIONS.md`（项目特定约定）
6. 本文档 §3（项目陷阱）
7. `DESIGN.md`（如果做涉及架构的 DU）

**优先级约束**：P0 未完成不做 P1，P1 未完成不做 P2。

**永久砍除的 DU**（DU-22/23/25/26/28）永远不能领取，如有人要求做必须先在 DESIGN §7 重新评估并修订决策。

---

## 2. 任务领取协议

### 2.1 选 DU

从 `TASKS.md` 选满足条件的 DU：
- 状态 = `[ ]`
- 依赖 DU 都已 `[x]`
- 优先级约束满足（见 §1）

**不要**同时领多个 DU。

### 2.2 领取（写入 PROGRESS.md "进行中"段）

### 2.3 完成

- 验收命令必须全过（每个 DU 都列了 `cargo test` 等命令）
- 更新 `CODE_MAP.md`（如文件结构变了）
- `TASKS.md` 把 `[~]` 改 `[x]`
- `PROGRESS.md` 移到"已完成阶段"
- 不要顺手做别的 DU 的范围

---

## 3. 项目特定陷阱（必看）

### 3.1 ort::Session 不是 Send

`ort::Session` **默认不是 Send + Sync**。多线程共享必须用：

```rust
pub struct PaddleOcrProvider {
    det_session: Arc<Mutex<Session>>,
    rec_session: Arc<Mutex<Session>>,
}
```

推理时 `let session = self.det_session.lock().await; session::run(...)`。

### 3.2 winit / global-hotkey / tray-icon / arboard 都要求主线程

Windows 上这些 API 必须在主线程（拥有消息循环的线程）调用。
tokio worker 通过 channel 发命令到主线程，主线程在 eframe frame callback 里处理。

错误模式：`tokio::spawn(async { EventLoop::new() })` → panic。

### 3.3 ONNX 模型加载是慢操作（1-3 秒）

启动时加载一次，存 `Arc<Mutex<Session>>` 复用。
**绝不能**每次 OCR 调用都重新加载。

### 3.4 egui Viewport API 创建 overlay

winit 0.30+ + eframe 0.29+ 用 Viewport 创建多窗口（每屏一个 overlay）：

```rust
ctx.show_viewport_deferred(
    ViewportId::from_hash_of(monitor_id),
    ViewportBuilder {
        fullscreen: Some(true),
        decorations: Some(false),
        always_on_top: Some(true),
        ..Default::default()
    },
    |ctx, ui| { /* 渲染 */ },
);
```

不要在 event loop 外创建 window。

### 3.5 tokio runtime 跨 winit callback

egui 回调是同步的。UI 提交 async 任务：

```rust
let handle = self.runtime.handle().clone();
handle.spawn(async move { /* ... */ });
```

启动时构造一个全局 `tokio::runtime::Runtime`，`Arc` 共享，UI 用 `rt.handle().spawn(...)`。

### 3.6 reqwest::Client 必须共享

```rust
// 错误 ❌
async fn translate(text: &str) {
    let client = reqwest::Client::new();  // 连接池浪费
}

// 正确 ✅
pub struct DeepSeekProvider {
    client: reqwest::Client,  // 构造时注入
}
```

Provider 在 Orchestrator 启动时构造一次。

### 3.7 Windows 路径

```rust
let config_path: PathBuf = dirs::config_dir()
    .ok_or_else(|| anyhow::anyhow!("no config dir"))?
    .join("SnapText")
    .join("config.toml");
```

不要硬编码 `C:\Users\xxx`，不要拼字符串。

### 3.8 oar-ocr 验证流程（DU-04 专用）

DU-04 开始时，**第一步**：在 tests/ 写独立脚本验证 `oar-ocr`：
- crates.io 是否真存在 0.7+ 版本
- 能否加载 PP-OCRv6 ONNX 模型
- 能否跑通一张英文测试图

**如失败**：同 DU 内立即切到 `ort` 自实现（不停下、不询问、不新建文档）。在 PR 描述记录变更。

### 3.9 DeepSeek 模型名 fallback（DU-05 专用）

`deepseek-v4-flash` 模型名未在 DeepSeek 官方确认。
**DU-05 实施时如 API 报 `model not found`**：立即切到 `deepseek-chat`（V3 通用，官方确认存在），在 PR 描述记录变更，并改 `DESIGN.md §4.3`。

### 3.10 click-through 切换

选区阶段：`window.set_cursor_hittest(true)`（接收鼠标）
显示卡片后：`window.set_cursor_hittest(false)`（穿透到下层应用）

**关闭机制**：穿透后用户点卡片外无法触发本窗口事件。用热键再按 / Esc / 定时器检测活动窗口变化。

---

## 4. 卡住时怎么办

### 4.1 信息不足

任务有歧义、缺文件路径、不清楚验收 → **不要瞎猜**。在 DU 行下方加注释 `⚠️ 待用户确认：xxx`，然后跳到下一个无歧义 DU，或停下等用户。

### 4.2 编译错误反复

**2 次没修好就停下**。在 `PROGRESS.md` 写：
```
- [阻塞] DU-XX
  - 阻塞原因：lifetime 编译错误，已尝试 2 次未解决
  - 错误信息：...
  - 已尝试：...
  - 请求帮助
```

不要乱试导致代码越来越乱。

### 4.3 发现 bug 但不在自己 DU 范围

记录到 `TASKS.md` "发现的问题"段：
```
- [bug] translate::deepl 的 rate limit 计算未考虑流式响应
```

不要顺手修。

---

## 5. 输出风格

完成任务后回报：

```
DU-XX 已完成 ✅

【产出】
- crates/.../xxx.rs (新增, 180 行)
- crates/.../yyy.rs (修改, +20 行)
- tests/zzz_test.rs (新增, 集成测试)

【验收】
- cargo test -p snaptext-core xxx ... N passed
- cargo clippy ... 无 warning
- cargo build --release ... 成功

【文档】
- CODE_MAP.md 已更新
- PROGRESS.md 已标记 DU-XX 完成

【未决项】
- xxx 待用户确认
```

不要：
- ❌ 复述用户的话
- ❌ 自夸 / 道歉
- ❌ 解释"我做了什么"细节（除非用户问）
- ❌ "如果您需要 X 请告诉我" —— 直接列出未决项
