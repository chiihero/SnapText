# 发布指南（Release Playbook）

> 发版前必读。从「代码定稿」到「GitHub Release 带安装包下载」的完整流程。
> 适配 Tauri 2 + `tauri-apps/tauri-action` 自动构建方案。

---

## 前置条件（仅首次需要）

| 条目 | 检查命令 | 预期 |
|---|---|---|
| GitHub CLI 已装 | `gh --version` | 显示版本号 |
| gh 已登录 | `gh auth status` | 账号 `chiihero` 已登录 |
| 远程已配置 | `git remote -v` | `origin → github.com/chiihero/SnapText.git` |
| 工作区干净 | `git status` | nothing to commit |

**未装 gh？** 一行装好（装完**关闭并重开终端**）：

```bash
winget install --id GitHub.cli
gh auth login    # 选 GitHub.com → HTTPS → Login with a web browser
```

---

## 标准发版流程（每次发版照抄）

### Step 1 — 改版本号（三处对齐）

发版版本号必须三处一致。**单一真理源是根 `Cargo.toml` 第 8 行**，但 `tauri.conf.json` 和 `package.json` 也要手动同步（无自动化联动）。

| 文件 | 字段位置 |
|---|---|
| `Cargo.toml`（workspace 根） | `[workspace.package] version` |
| `src-tauri/tauri.conf.json` | 顶层 `"version"` |
| `package.json` | `"version"` |

> 例：0.1.0 → 0.2.0，三处全改成 `"0.2.0"`。`src-tauri/Cargo.toml` 用 `version.workspace = true` 自动继承，**不要动**。

### Step 2 — 更新变更日志

`docs/CHANGELOG.md` 顶部把对应版本的 `## [Unreleased]` 或新增 `## [X.Y.Z] — YYYY-MM-DD` 段落填充好（Keep a Changelog 格式）。

### Step 3 — 提交并推送 main

```bash
git add -A
git commit -m "chore(release): vX.Y.Z"
git push origin main
```

确认 `git status` 显示 up to date、与 `origin/main` 同步。

### Step 4 — 打 tag 触发云端构建 ⭐核心

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

**push tag 的瞬间，GitHub Actions 自动开跑**（见 `.github/workflows/release.yml`）：
- Runner：`windows-latest`
- 流程：`npm ci` → Rust stable 编译 → `tauri-action` 打包
- 产物：NSIS `*-setup.exe` + MSI `*.msi`，自动发布到 Release

### Step 5 — 监控构建（约 10–20 分钟，Rust 编译慢属正常）

```bash
gh run watch            # 实时跟踪 Actions 日志
gh release view vX.Y.Z  # 构建成功后查看 Release 产物下载链接
```

也可浏览器看：`https://github.com/chiihero/SnapText/actions`、`/releases`

---

## 发版后清理

- 若 `CHANGELOG.md` 里仍有 `[Unreleased]` 占位，确认新版本的变更已正确归档到 `[X.Y.Z]` 段下
- `docs/PROGRESS.md` / `docs/TASKS.md` 按需更新发版状态（参考 `AGENTS.md` §5.2 文档矩阵）

---

## 故障排查

| 症状 | 原因 | 处理 |
|---|---|---|
| Actions 没触发 | tag 名不是 `v*` 开头 | 改 tag 名（`v0.2.0` 而非 `0.2.0`） |
| CI 失败：权限错误 | Release 需要 `contents: write` | workflow 已声明；private repo 去 `Settings → Actions → General → Workflow permissions` 改为 `Read and write` |
| CI 失败：`tauri-action` 兼容 | Tauri 2 升级后 action 版本滞后 | 查 `tauri-apps/tauri-action` 最新 release，更新 `@v0` |
| CI 失败：`npm ci` 报 lockfile 不一致 | `package.json` 改了依赖但没更新 lockfile | 本地 `npm install` 重生成 `package-lock.json` 并提交 |
| CI 失败：Rust 编译错误 | 代码问题 | 本地先 `cargo build --release` 跑通再发版 |
| Release 建了但没产物 | 构建步骤中途失败 | `gh run view <run-id> --log` 看哪一步挂了 |
| tag 已推但 Release 没建成 | action 未触发或失败 | 手动补建：`gh release create vX.Y.Z SnapText_X.Y.Z_x64-setup.exe` |

---

## 回退方案：本地构建 + 网页手动上传（CI 跑不通时用）

```bash
# 本地出包
scripts/build.bat    # 或 npm run tauri build
# 产物在 src-tauri/target/release/bundle/{nsis,msi}/
```

然后浏览器打开 `https://github.com/chiihero/SnapText/releases/new`：
1. 选刚 push 的 tag（如 `vX.Y.Z`）
2. 填标题 `SnapText vX.Y.Z`
3. 拖入 `*-setup.exe` 和 `*.msi`
4. 填发布说明（可从 `CHANGELOG.md` 复制）
5. 发布

---

## 速查：一键发版（前置条件已就位时）

```bash
# 假设版本号已改好、CHANGELOG 已更新、改动已提交
VERSION=v0.2.0
git push origin main
git tag $VERSION
git push origin $VERSION
gh run watch
gh release view $VERSION
```
