# GLOSSARY — 项目特定术语表

> 仅列**项目特定**术语。通用术语（OCR / MT / LLM / DPI / MSI / SDK 等）AI 已知，不再重述。
> 文档和代码注释使用术语时必须查这里。

最后更新：2026-06-24

## 项目角色与流程

| 中文 | 英文 / 标识符 | 定义 |
|---|---|---|
| 提供者 | Provider | 实现了某个 `XxxProvider` trait 的具体类型。例：`OpenAiCompatProvider`, `PaddleOcrProvider` |
| 后端 | Backend | Provider 内部使用的具体技术。例：DeepSeek 的后端是 HTTP API；PaddleOcr 的后端是 ONNX Runtime |
| 管线 | Pipeline | 多步骤处理链。例：OCR Pipeline = det → cls → rec |
| 档位 | Tier | 模型大小的预设。本项目 PP-OCRv6 用 `medium` / `small`（不含 tiny，因不含日文） |
| 模型 | Model | 推理用的权重文件（.onnx） |

## 任务与优先级

| 中文 | 英文 | 定义 |
|---|---|---|
| **优先级** | **P0 / P1 / P2** | 任务优先级。P0 = 必做发布门槛；P1 = 应做完整体验；P2 = 可做扩展工业级 |
| **交付单元** | **DU (Delivery Unit)** | AI 单次会话可完成的完整模块。本项目共 20 个 DU |
| **永久砍除** | Permanently Cut | 已明确不做，不列入路线图。如需重启必须改 DESIGN §7 |
| 状态机 | State Machine | 应用在 Idle / Selecting / Recognizing / Translating / Showing 间的转换 |

## UI 与交互

| 中文 | 英文 | 定义 |
|---|---|---|
| 框选 | Region Selection | 用户在 overlay 上拖拽矩形选区 |
| 选区 | Selection / Region | 框选后的矩形区域 |
| 悬浮卡片 | Floating Card / Card | 显示译文的 UI 元素 |
| 蒙版 | Mask / Dim | 框选时整屏半透明黑色覆盖层 |
| 透传 | Click-through | 鼠标点击穿过当前窗口到下层 |
| 视口 | Viewport | egui 0.29+ 多窗口 API |

## 架构与代码

| 中文 | 英文 / 标识符 | 定义 |
|---|---|---|
| 调度器 | Orchestrator | 中央协调器，持有所有 Provider，管理状态机 |
| 命令 | Command | UI/Hotkey 发给 Orchestrator 的消息（`enum Command`） |
| 事件 | Event | Orchestrator 发给 UI 的消息（`enum Event`） |
| 显示器帧 | Captured Frame | 一次捕获得到的图像 + 元数据 |
| OCR 行 | OcrLine | OCR 输出的一行文字 + bbox + 置信度 |
| 包围盒 | Bbox | 矩形 `{x, y, w, h}` |
| 书写方向 | Writing Direction | Horizontal / Vertical |
| OpenAI 兼容 | OpenAI Compatible | 协议兼容 OpenAI `/v1/chat/completions` 的端点。DeepSeek / Moonshot / 智谱等属此类 |

## 文件与目录

| 路径 | 含义 |
|---|---|
| `%APPDATA%\SnapText\` | 用户数据根目录 |
| `%APPDATA%\SnapText\config.toml` | 用户配置 |
| `%APPDATA%\SnapText\models\ppocr\v6\{tier}\` | PP-OCRv6 ONNX 模型缓存 |
| `%APPDATA%\SnapText\history.db` | sqlite 历史数据库 |
| `%APPDATA%\SnapText\logs\` | 日志目录 |
| `%LOCALAPPDATA%\Programs\SnapText\` | 安装目录 |

## 易混淆对照（不要混用）

| 易混点 | 区分 |
|---|---|
| Provider（业务抽象）vs Backend（技术实现） | Provider 是 trait；Backend 是 Provider 内部用的具体技术 |
| Model（模型权重）vs Tier（档位） | 同一档位可以有不同格式模型 |
| OCR（识别）vs Translate（翻译） | 本项目里独立两步，OCR 总先于 Translate |
| Captured Frame（已捕获帧）vs Image（通用图像） | Captured Frame 有元数据（来源显示器、时间戳） |
| Hotkey（全局热键）vs Shortcut（菜单快捷键） | 本项目仅关注全局热键 |
