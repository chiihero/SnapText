# SnapText

截图 OCR + 翻译桌面应用（Windows）。框选屏幕区域 → 本地 OCR 识别 → 调用翻译 API → 译文图上原位覆盖，全程快捷键驱动。

## 技术栈

- **后端**：Rust（Tauri 2 workspace）
  - `crates/snaptext-core`：纯逻辑库（OCR / 翻译 / 历史 / 模型管理 / 截图）
  - `src-tauri`：Tauri 二进制（系统集成 + 命令层）
- **前端**：Vue 3 + TypeScript + Naive UI + Vite
- **OCR**：PaddleOCR PP-OCRv6（本地 ONNX 推理，离线）
- **翻译**：DeepL / Microsoft / OpenAI 兼容（DeepSeek 等）

## 快速开始

### 环境要求

- [Node.js](https://nodejs.org)
- [Rust](https://rustup.rs)（见 `rust-toolchain.toml`）
- Windows（截图依赖 Windows Graphics Capture API）

### 开发运行

双击 `scripts/dev.bat`，或手动：

```bash
npm install
npm run tauri dev
```

首次启动会进入引导页配置快捷键、下载 OCR 模型、设置翻译 Provider。

### 打包

双击 `scripts/build.bat`，或手动：

```bash
npm run tauri build
```

安装包输出到 `src-tauri/target/release/bundle/`（NSIS + MSI）。

### 发布到 GitHub Release

推送形如 `v*` 的 tag 即触发云端自动构建并发布（见 `.github/workflows/release.yml`）：

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI 在 `windows-latest` 上编译，自动产出 `*-setup.exe` + `*.msi` 到 Release。构建约 10–20 分钟，可在 Actions 页查看日志。

### 其他脚本

- `scripts/reset-onboarding.bat`：重置引导标志（保留 Key 和模型，仅让引导页下次重新出现）
- `scripts/download-models.ps1`：离线下载 OCR 模型（辅助）
- `scripts/stress-test.ps1`：压测脚本

## 项目结构

```
crates/snaptext-core/   纯逻辑库（OCR / 翻译 / 历史 / 截图 / 模型管理）
src-tauri/              Tauri 后端（命令层 + 窗口 + 状态）
src/                    Vue 前端（views / stores / styles）
scripts/                开发与打包脚本
docs/                   设计文档（CODE_MAP / DESIGN / TASKS / PROGRESS）
```

> 三个核心目录名分别绑定 Vite（`src/`）、Tauri CLI（`src-tauri/`）、Cargo workspace（`crates/`）的约定，非项目自定义；命名溯源见 [`docs/CODE_MAP.md`](docs/CODE_MAP.md) §顶层结构。

## 文档

详细的架构、文件职责、设计决策见 [`docs/`](docs/)：

- [`docs/CODE_MAP.md`](docs/CODE_MAP.md)：文件路径 ↔ 职责 ↔ 依赖映射
- [`docs/DESIGN.md`](docs/DESIGN.md)：核心模块设计与技术选型
- 开发规范见 [`AGENTS.md`](AGENTS.md)

## License

MIT
