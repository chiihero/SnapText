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

### 3.1 ONNX 推理跨线程共享（oar-ocr）

oar-ocr 0.7+ 的 `OAROCR` 已实现 `Send + Sync`（内部 `Arc<Session>`），用 `Arc<OAROCR>` 跨线程共享，**无需 `Mutex`**（ort 直接用法曾需 `Arc<Mutex<Session>>`，oar-ocr 封装已解决）：

```rust
pub struct PaddleOcrProvider {
    engine: Arc<OAROCR>,  // 构造时 Arc::new，命令层 Arc clone
}
```

推理在 `spawn_blocking` 中执行（CPU 密集，不阻塞 tokio reactor）。

### 3.2 Tauri 命令直接读 State，无 channel

egui 期曾有 mpsc channel（UI ↔ Orchestrator）。迁移 Tauri 后已删——`#[tauri::command]` 函数用 `State<'_, AppState>` 取共享状态，直接调 Provider。**不要**重新引入 channel 或手动 `tokio::runtime::Runtime` 构造（Tauri 内置 runtime，`#[tauri::command] async fn` 自动跑在其上）。

### 3.3 ONNX 模型加载是慢操作（1-3 秒）

`PaddleOcrProvider::new` 在 `AppState::build`（main setup）加载一次，存 `Arc<OAROCR>` 复用。**绝不能**每次 OCR 调用都重新加载。

### 3.4 跨窗口数据：后端缓存 + 前端拉取（反竞态）

Tauri 多窗口 JS 上下文隔离，**Pinia 不跨窗口共享**，Tauri `emit` 事件可能因子窗口未加载完而丢失。故选区→结果窗的数据传递走反竞态模式：

- 后端命令写缓存（`state.captured`/`last_crop`/`last_ocr`）
- 子窗口 `onMounted` 主动调命令拉取（`get_last_capture`/`get_last_crop`/...）

**不要**用 Pinia 跨窗口传数据，也**不要**依赖 `emit` 给刚创建的子窗口（会丢）。选区窗常驻复用后，`emit("capture-ready")` 可靠（窗口早已加载完），但仍保留拉取命令作兜底。

### 3.5 shot:// 自定义协议取截图（不写临时文件）

全屏截图经 `shot://` 协议（`main.rs::register_asynchronous_uri_scheme_protocol`）从 `state.captured` 内存直接编码 BMP 返回，前端 `<img src="http://shot.localhost/<id>">`。**不写临时文件**（旧版写临时 BMP，省 ~150ms IO）。陷阱：shot:// URL 按 monitor id 固定，WebView2 HTTP 缓存会命中旧帧——前端必须加 `?t=Date.now()` 时间戳 + 后端响应加 `Cache-Control: no-store`（双保险）。

### 3.6 reqwest::Client 必须共享

```rust
// 错误 ❌
async fn translate(text: &str) {
    let client = reqwest::Client::new();  // 连接池浪费
}

// 正确 ✅
pub struct OpenAiCompatProvider {
    client: reqwest::Client,  // 构造时注入
}
```

`AppState.client` 在 main setup 构造一次，`build_provider` 注入各 Provider。

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
