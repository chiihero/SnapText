# CODE_MAP — 文件路径 ↔ 职责 ↔ 依赖映射

> 给 AI 用：快速定位"改 X 看哪里"、"动 A 影响哪些 B"、"哪些文件禁止碰"。

最后更新：2026-07-07（**修复结果窗口复用时不刷新 bug**：用户反馈"快捷键截图→结果窗口→再按快捷键框选，结果窗口停在第一次的内容没刷新"。根因：`Capture.vue::onUp` 用固定 label `"result"` 调 `new WebviewWindow`，第二次框选时 Tauri 发现同 label 窗口已存在→不重建页面→`Result.vue::onMounted` 不会再执行→不会重新拉 `last_crop`/OCR/翻译，后端 `state.last_crop` 已是新数据但前端不取。**修复**：结果窗口改"存在则复用刷新、不存在才新建"。① `Capture.vue::onUp` crop 后 `WebviewWindow.getByLabel("result")` 判断：存在则 `emit("result-refresh")` + `show()` + `setFocus()`，不存在则 `new`（首次与手动关闭后重建）。② `Result.vue` 把 `onMounted` 流程抽成 `refresh()` 函数，`onMounted` 注册 `listen("result-refresh", refresh)` + 首次调一次，`onBeforeUnmount` 清理 unlisten。③ refresh 完整重置 16 个响应式字段（漏一个会残留旧内容）+ 重设 `t0Base` 耗时基准。④ **generation 守卫**：`runOcr`/`runTranslate` 接 `gen` 参数，每个 `await` 后比对 `gen !== generation` 丢弃 stale 结果——medium 档 OCR ~3s，用户可能在第一次未跑完就框选第二次，旧请求后完成时丢弃不更新 UI、不弹错误 toast。⑤ **`img.src` 加 `?t=Date.now()` 防缓存**（从"次要"升级为"必须"）：`write_crop_png` 文件名按 monitor_id 固定（`result_<id>.png`），复用窗口时 URL 与上一次完全相同→浏览器不触发 onload→canvas 底图不更新；与 `Capture.vue` 选区窗 `?t=` 防缓存对齐。⑥ refresh 失败不 close 窗口（保留让下次快捷键自动修复），与第一次失败行为统一。**边界已处理**：OCR/翻译进行中触发第二次框选（generation 守卫）、用户关了结果窗口后框选（getByLabel 返回 null 走 new）、窗口最小化时收到 refresh（show+setFocus 唤起）、手动模式下按钮 disabled 防重复触发。**后端零改动**（`state.last_crop`/`last_ocr` 接力缓存已支持反复写入）。**验证**：vue-tsc 0 错误 + clippy -D warnings 0 警告（后端未动）。改动文件：`src/views/{Capture,Result}.vue`。手动验证留给用户：第一次快捷键出结果窗口→不关→第二次快捷键框选→结果窗口内容应更新为新截图/译文。）

2026-07-03（**修复模型无法下载 bug**：用户反馈"首次不知何原因跳过初始化后，模型没下载就再也没法下载了"。根因：模型下载的 UI 入口只存在于一次性引导页 `Onboarding.vue`，设置页 OCR 分类无兜底入口。一旦 `onboarding_completed` 变 true（首次走完引导 / 手改 toml / 异常退出），引导页永不再现；模型若缺失（首次跳过、后来被删、文件损坏半成品），用户彻底无处下载，截图只报"OCR 模型未就绪，请先下载模型"。`state.rs`/`models.rs` 注释都写了"设置页下载/诊断"但 Settings.vue 未实现——设计意图与代码脱节。**修复**：在 `Settings.vue` OCR 分类补「模型状态 tag + 下载按钮 + 进度条 + 错误重试」兜底入口，下载逻辑从 Onboarding.vue 抄一份内联（接受两处重复换设置页直下载体验，不抽 composable 避免过度设计）。关键点：Settings 是草稿机制，下载前先 `store.save(draft)` 落盘 tier（`download_models` 用入参 tier 但 `reload_ocr_provider` 读 state.config.ocr.tier，不先落盘会导致下载新档位、reload 用旧档位错配）；下载中 disable 档位 select 防半途换档；`onBeforeUnmount` 清理 unlisten 句柄（设置窗口可销毁）。**为什么不在 Home.vue 加跳转**：用户选择设置页内下载（体验好，不重走引导三步）。**验证**：vue-tsc 0 错误（后端零改动 clippy 不受影响）。改动文件：`src/views/Settings.vue`、`docs/CODE_MAP.md`。手动验证留给用户：删 models 目录 + onboarding_completed=true → 设置页 OCR 分类下载可用。）

2026-07-02（**OCR 大图内存不回落优化**：用户反馈 release 构建下 OCR 大图后内存飙升且不回落（达 2-3G）。根因在 ort 2.0.0-rc.12：默认开启的 `memory_pattern` 在动态 shape（oar-ocr Type0 resize，每张图尺寸不同）下按"见过的最大 shape"扩容 pattern buffer 并永久保留，且 ort 无"推理后释放 arena"的 API（arena 仅随 session drop 释放）。叠加 oar-ocr 默认偏大的 batch（det=8）和 SnapText 接力缓存不清，导致大图后内存只涨不降。**4 处改动对症优化**：① `paddleocr.rs::new` 经 oar-ocr 的 `ort_session()` 透传口子传 `OrtSessionConfig`——关 `memory_pattern`、`intra_threads` 封顶 4、`image_batch_size(2)`/`region_batch_size(16)`（降单次推理峰值 + 阻断 pattern buffer 扩容）。② `ocr_translate.rs::translate_region` 写历史后清空 `last_crop`/`last_ocr` 接力缓存（框选流程已结束，释放裁剪图 + OCR 行）。③ `main.rs` shot:// 协议改为持锁期间直接编码 BMP（免 clone 整张全屏图，4K RGBA 单份 30MB+），保持原有三态状态码语义（200/500/404）。④ 不改 OCR 默认档（用户确认保持 Medium）、不重建 session（oar-ocr 无 API 且成本高）。**验证**：fmt + clippy -D warnings（0 警告）+ test --workspace（core 46 + src-tauri 19 全过）。改动文件：`crates/snaptext-core/src/ocr/paddleocr.rs`、`src-tauri/src/commands/ocr_translate.rs`、`src-tauri/src/main.rs`。实测验证留给用户：连续多次框选大图确认内存不再持续攀升。）

2026-07-01（**文档补强：三目录命名溯源**：用户反馈 `crates`/`src`/`src-tauri` 三个目录"看起来奇怪"。经调研确认三者名字分别绑定 Vite（`src/` 默认前端入口）、Tauri CLI（`src-tauri/` 强约定，CLI 靠它定位后端项目）、Cargo workspace（`crates/` 社区惯例，crate 分组目录），改名得不偿失且偏离 create-tauri-app 主流模板。决定不改目录，改为在 §顶层结构补命名脚注消解困惑。`src` 与 `src-tauri` 视觉撞名是所有 Tauri 项目的固有特征，靠"前端=TS / 后端=Rust"语言差异区分。改动文件：`docs/CODE_MAP.md`、`README.md`。）

2026-07-01（**项目级清理第 2 批（脚本归整 + 文档完善）**：根目录散落的中文脚本归入 `scripts/` 并改英文名——`开发.bat`→`scripts/dev.bat`、`打包.bat`→`scripts/build.bat`、`重置引导.bat`→`scripts/reset-onboarding.bat`（消除中文文件名在 git/shell 的编码隐患，与既有 `scripts/*.ps1` 统一约定）。三个 bat 的 `cd /d "%~dp0"` 改为 `cd /d "%~dp0.."`（脚本已移入 `scripts/` 子目录，须切到上一级项目根才能找到 `package.json`/`node_modules` 跑 `npm run tauri`）。`reset-onboarding.bat` 内对 `kai-fa.bat` 的提示引用改为 `scripts\dev.bat`。`.gitignore` 删历史遗留 `smiley-tmp.zip`（与项目无关）。`README.md` 从一行扩为工程文档（技术栈/环境/启动/结构/文档索引）。CODE_MAP `scripts/` 表同步补三行。**验证**：grep 确认无 `kai-fa`/中文 bat 残留引用；三个 bat 的 cd 行已改为项目根。）

2026-07-01（**首启引导页**：原 `main.rs::setup` 的 `ensure_models` 在启动时同步后台下载 OCR 模型，用户无感知且不可选；前端虽有 `download_models` 命令封装但**零调用方**（事件 `download-progress`/`download-done` 无人监听）。改为引导页主动触发：① 删 `main.rs` 的 `ensure_models` 函数及其调用（启动不再下模型，秒进主界面）；② 新增 `GeneralConfig.onboarding_completed: bool`（默认 false）作单标志位——仅用户走完引导（完成/跳过）调 `complete_onboarding` 命令才置 true，中途关闭/崩溃/下载失败仍为 false → 下次重进引导页。引导页不记步骤进度（断点续传属过度设计），配置末尾统一 `save_config` 一次（避免分步多次触发后端重注册热键副作用）。③ 新增 `complete_onboarding` 命令（`config_cmd.rs`，只置标志+落盘，不复用 `save_config` 因语义不同——不重建 Provider/不重注册热键）。④ 前端新增 `Onboarding.vue`（三步向导：快捷键→下载模型→翻译配置），复用 Settings.vue 的草稿机制（深拷贝+统一保存）+ Provider UI 片段；下载步监听 `download-progress`（按 det/rec/dict 三段权重 33/47/20 折算进度）/`download-done`，`onBeforeUnmount` 清理 unlisten 句柄（Capture.vue 缺这步但它是常驻窗口，引导页可销毁必须清理）。⑤ `Home.vue` onMounted 判断 `onboarding_completed===false` 则 `router.replace('/onboarding')`。模型下载幂等：进步先 `is_models_ready()` 检查，已就绪跳过；未就绪重下整个（不续传，文件不大）。**为什么单标志位不记步骤**：模型幂等检查 + 配置末尾保存已让重进体验够好，再记步骤是过度设计。**为什么引导页放主窗口内**（路由而非独立窗口）：首启场景单一，改动最简，复用现有路由+窗口。改动文件：`crates/snaptext-core/src/config.rs`、`src-tauri/src/main.rs`、`src-tauri/src/commands/config_cmd.rs`、`src/api.ts`、`src/router.ts`、`src/views/{Home,Onboarding}.vue`。**验证**：fmt + clippy -D warnings（0 警告）+ test（46 通过 0 失败）+ vue-tsc（0 错误）。）

2026-07-01（**OCR Provider 降级（修复首启引导引入的启动崩溃）**：删除 `main.rs::ensure_models`（启动同步下载）后，`AppState::build` 的 `PaddleOcrProvider::new(...)?` 在模型缺失时 panic 阻断启动。改为与翻译降级同款哲学：`state.ocr` 字段从 `Arc<dyn OcrProvider>` 改为 `Mutex<Option<Arc<dyn OcrProvider>>>`，`build` 里 `match PaddleOcrProvider::new` 失败→`None` 不崩（仅 warn）。消费点仅 `recognize_region` 一处，取 `state.ocr.lock().await.clone()` 为 None 时返回中文错误"OCR 模型未就绪，请先下载模型"。新增命令 `reload_ocr_provider`（`config_cmd.rs`）：用当前 `config.ocr.tier` 构造 PaddleOcrProvider 写回 `state.ocr`，引导页 `download-done` 成功后调它即时生效（无需重启）。**为什么不沿用 `save_config`**：reload 只动 OCR 不动翻译/热键，语义独立。**与现有降级体系一致**：翻译（缺 Key→None）、热键（占用→降级运行+提示）、OCR（缺模型→None）三者同构，都是"缺资源不崩、UI 引导修复"。改动文件：`src-tauri/src/{state.rs,main.rs}`、`src-tauri/src/commands/{config_cmd.rs,ocr_translate.rs}`、`src/api.ts`、`src/views/Onboarding.vue`。**验证**：fmt + clippy -D warnings（0 警告）+ test（46 通过 0 失败）+ vue-tsc（0 错误）。

2026-07-01（**项目级清理第 1 批（删除垃圾 + 死代码）**：① 删运行时垃圾 `smiley-tmp.zip`（405KB，gitignore 已忽略）+ `app1.log`（未跟踪）。② 删 `CLAUDE.md`——与 `AGENTS.md` 逐字相同（diff 确认），重复文件维护必分裂，CODE_MAP/DESIGN 均引用 `AGENTS.md` 为主。③ 删 `types.rs` 的 `AppState` enum（Idle/Selecting/Recognizing/Translating/Showing 5 变体）——egui 状态机遗留，grep 全工作区 **0 消费方**，Tauri 架构下窗口各自管理状态不用集中状态机。④ 同步删 CODE_MAP types.rs 表格的 `AppState` 行。**验证**：fmt + clippy -D warnings（0 警告）+ test（65 通过 0 失败）。）

2026-07-01（**删除 fallback 死代码**：`translate/fallback.rs`（`FallbackProvider`）实现完整但 `build_provider` 从不构造，`config.fallback_order` 字段无任何消费方——属提前写好的半成品（DU-17 故障转移），接线需联动 `is_retryable` + 多 Provider 嵌套构造，工作量属新功能开发而非清理。决定删除以求代码库零死代码，将来 DU-17 重新实现。删除：`fallback.rs` 整文件 + `translate/mod.rs` 的 `pub mod fallback` + `TranslateConfig.fallback_order` 字段（含 Default）+ `api.ts` 的 `fallback_order` 类型字段。旧 config.toml 残留 `fallback_order` 被 `#[serde(default)]` 忽略，平滑。**验证**：clippy -D warnings（0 警告）+ test（46 通过 0 失败）+ vue-tsc（0 错误）。改动文件：`crates/snaptext-core/src/translate/{fallback.rs 删,mod.rs}`、`crates/snaptext-core/src/config.rs`、`src/api.ts`。）

2026-06-30（**热键注册失败优雅降级**：原 `main.rs::setup` 用 `?` 把 `global_shortcut().register()` 错误抛出，热键被其他程序占用（典型：上一次 snaptext 进程残留未释放）时整个应用 panic 无法启动。改为降级：注册失败不阻断启动，写入 `AppState.hotkey_error: Mutex<Option<String>>`（Some=失败原因），沿用代码库"后端缓存状态 + 前端主动拉取"反竞态模式（不引入 Tauri 事件）。新增命令 `get_hotkey_status()`（`config_cmd.rs`）返回该状态；`save_config` 重注册热键时同步刷新该字段（成功清空/失败写入），设置页保存后状态即时更新。前端三处接线：`stores/config.ts` 加 `hotkeyError` state + load/save 拉取；`Home.vue` 启动时非空则弹 `message.warning` 一次性引导；`Settings.vue` 快捷键卡片非空时显示红色 `n-alert`，保存成功后自动消失。**为什么不用事件**：与 `captured`/`last_crop`/`last_ocr` 同款理由——Pinia 不跨窗口共享，子窗口 `onMounted` 主动拉取最稳。**验证**：fmt + clippy -D warnings（0 警告）+ test（49 通过）+ vue-tsc（0 错误）。改动文件：`src-tauri/src/{main,state}.rs`、`src-tauri/src/commands/config_cmd.rs`、`src/api.ts`、`src/stores/config.ts`、`src/views/{Home,Settings}.vue`。）

2026-06-30（**HTTP/图像编码样板收敛（零行为变化，第二轮重构）**：在上一轮（重试 with_retry / sqlite spawn_db / OpenAiCompatParams）基础上继续消除啰嗦写法。① **HTTP 错误分类收敛**：三个 Provider 的 `do_once` 此前各内联两段完全相同的样板——`send().map_err(|e| if is_timeout {...})`（reqwest 错误→Timeout/Request 分类）与 `if !status.is_success() { 读 body 封装 Api }`（非 2xx 检查），抽成 `translate::classify_send_err(e)`（同步）+ `ensure_2xx(resp)`（async）两个 helper，各 Provider 用 `.map_err(classify_send_err)?` + `ensure_2xx(resp).await?` 一行替代，每个 do_once 砍 ~8 行。② **图像编码收敛**：`ocr_translate.rs` 两处 PNG 编码（write_crop_png 写盘 / translate_region 入历史）各内联 `Cursor::new → write_to → map_err`，抽成 `encode_png(img)` helper 复用。③ **translate_region 可读性**：原 `screenshot_png` 字段在 struct 字面量里嵌 4 层 match（lock→as_ref→map→match），抽成 `read_last_crop_png(&state)` 预先算好，主流程清爽。main.rs 的 shot 协议 BMP 编码因格式不同 + 在异步协议闭包内，保留不抽。**验证**：fmt + clippy -D warnings（0 警告）+ test（68 通过 0 失败）。改动文件：`translate/mod.rs`、`translate/{openai_compat,deepl,microsoft}.rs`、`src-tauri/src/commands/ocr_translate.rs`。）

2026-06-30（**内部结构重构（零行为变化）**：消除三处明确的重复/样板，提升可读性，不触及任何对外接口。① **translate 重试收敛**：三个 Provider（openai_compat/deepl/microsoft）此前各有一段逐字相同的 18 行重试循环（`last_err` + `for attempt` + 指数退避 + `is_retryable` 判定），抽成 `translate::with_retry(max_retries, do_once)` 泛型 helper，各 Provider 的 `translate()` 改为调用它、保留各自的 `do_once` 单次逻辑。② **sqlite 样板收敛**：`sqlite_store.rs` 8 个 async 方法此前各有一段逐字相同的 `spawn_blocking` 样板（pool.clone → get? → dao → 双层 map_err），抽成 `spawn_db(label, f)` 私有 helper，每个方法压到 2~3 行。③ **OpenAiCompatProvider 参数收敛**：`new()` 原有 10 参数触发 clippy `too_many_arguments`，抽成 `OpenAiCompatParams` struct 字段自文档。④ 修两处 clippy `redundant_closure`。**验证**：`cargo fmt --check` + `clippy -D warnings`（0 警告，含原有 2 警告）+ `test --workspace`（68 通过 0 失败）全过；行为零变化，现有 do_once/dao/集成测试全覆盖。改动文件：`translate/mod.rs`、`translate/{openai_compat,deepl,microsoft}.rs`、`history/sqlite_store.rs`、`src-tauri/src/commands/capture.rs`。）

2026-06-30（**选区窗口消除截图白闪**：选区窗口从创建到 Canvas 画上截图之间有 WebView2 冷启动 + 拉取截图 + 解码的空窗期，默认白底窗口会整屏白闪一下。`window.rs::open_capture_window` 创建时改 `.visible(false)`，`Capture.vue` 首次 `draw()` 成功画上截图后经双层 `requestAnimationFrame` 等 WebView 合成完该帧，再 `getCurrentWindow().show()`（`firstDrawn` flag 保证只 show 一次）。双层 rAF 是关键：`drawImage` 写 canvas 缓冲是同步的，但浏览器合成到屏幕要等下一渲染帧，若 `show()` 早于合成 WebView2 默认白底会露一帧（短白闪）。效果：热键后桌面静止 ~200-500ms（不可见等待）→ 选区窗直接以截图内容出现，无白闪；总响应时间不变。改动文件：`src-tauri/src/window.rs`、`src/views/Capture.vue`。）

2026-06-30（**日志配置接线 + 翻译重试接入 + 删除 3 个无消费方字段**：排查"有配置/UI 但后端没接线"的同类 bug 第二、三批。**A 组日志**：`main.rs::init_logging` 改签名接收 `&Config`，读 `general.log_level`（空回退 "info"，env 优先级最高）和 `general.log_file`（None 用默认路径），此前两者均硬编码。**B 组重试**：`max_retries` 字段此前零重试逻辑（Provider 失败直接 `?`），现三个 Provider（openai_compat/deepl/microsoft）各加内联重试循环，新增 `translate::is_retryable(&CoreError)` + `RETRY_BASE_MS`（500ms）共用判定/退避。**智能重试边界**：重试 `Timeout`/`Request`/HTTP 5xx/429；不重试 HTTP 其他 4xx（鉴权）/`Parse`/`UnsupportedPair`。退避 `500ms * 2^attempt`。各 Provider 把单次逻辑抽成 `do_once`，`translate()` 包重试循环。**D 组删除**：`onboarding_completed`（引导页不存在）、`hotkey.cancel`（取消靠 Capture.vue 写死 Escape）、`fallback_to_dxgi`（DXGI 回退为无条件合理默认，开关反而有害）—— 三字段删 config + api.ts 类型 + Settings.vue（cancel 输入框）；`CaptureConfig` 删字段后留空 struct 供未来扩展；旧 config.toml 残留字段被 `#[serde(default)]` 忽略。**C 组故障转移暂不做**（`fallback_order` 仍是死代码，待决策）。改动文件：`main.rs`、`config.rs`、`translate/mod.rs`、`translate/openai_compat.rs`、`translate/deepl.rs`、`translate/microsoft.rs`、`api.ts`、`Settings.vue`。）

2026-06-30（**接线历史清理 + 蒙版透明度配置，删除过度设计的 show_original**：排查"有配置/UI 但后端没接线"的同类 bug 后修复第一批。① **历史清理接线**：`retention_days`/`max_records`/`auto_clean_on_start` 三字段此前是死代码（`cleanup_blocking` 实现了但启动时从不调用），在 `state.rs::AppState::build` 构造完 history 后按 config 调一次 `cleanup_blocking`，设置页"启动时自动清理"开关 + 滑块生效。② **蒙版透明度接线**：`overlay_dim_alpha` 此前 Capture.vue 硬编码 `rgba(0,0,0,0.4)`，改为 onMounted 读 `cfg.ui.overlay_dim_alpha`（异步拉取，默认 0.5）。③ **删除 `show_original`**：原"点击行显示原文"开关 Result.vue 从未读取，且 Result.vue 自身的点击切换交互已完整（默认译文 + 工具栏全局切换 + 单行点击切换），该开关属过度设计，删字段（`config.rs`/`api.ts`）+ 设置项（`Settings.vue`）；旧 config.toml 残留字段被 `#[serde(default)]` 忽略，平滑。改动文件：`state.rs`、`Capture.vue`、`config.rs`、`api.ts`、`Settings.vue`。）

2026-06-30（**翻译 prompt 双模式（系统默认/自定义）**：在「prompt 可配置化」基础上加 `TranslateConfig.prompt_use_custom`（默认 false）。**系统默认模式**（默认）：UI 只读展示，渲染走后端固定常量——后端升级默认 prompt 时所有默认模式用户自动受益（不读 `prompt_template` 字段，修了"配置固化"隐患）。**自定义模式**：渲染走 `prompt_template` 字段，切到自定义时若该字段为空则预填系统默认值作为编辑起点。新增命令 `get_default_prompt()`（`commands/config_cmd.rs`）返回 `prompt.rs::DEFAULT_PROMPT_TEMPLATE` 字符串供前端只读展示——**前端零硬编码常量**（删 `Settings.vue` 原 `DEFAULT_PROMPT_TEMPLATE`），单一数据源彻底消除两端不同步。改动文件：`config.rs`、`prompt.rs`、`commands/config_cmd.rs`、`main.rs`、`api.ts`、`Settings.vue`。）

2026-06-30（**翻译 prompt 可配置化**：写死在 `translate/prompt.rs` 的 LLM 翻译 prompt 改为可在设置页编辑的配置项。新增 `TranslateConfig.prompt_template`（顶层，所有 LLM 类 Provider 共用；DeepL/Microsoft 是专用 MT 不受影响），占位符 `{{source}}`/`{{target}}`/`{{input}}` 双花括号（避免与用户原文里的花括号冲突，Jinja2/mustache 惯例），渲染用 `str::replace` 不引模板引擎。**单一数据源**：`prompt.rs::DEFAULT_PROMPT_TEMPLATE` 常量同时供 `config.rs` 默认值和前端「恢复默认」按钮引用（前端 `Settings.vue` 手动对齐文案）。**容错兜底**：用户模板若漏掉 `{{input}}`，渲染时自动追加原文，防模型拿不到源文本瞎编。`render_translate_prompt` 签名从 `(req)` 改为 `(req, template)`；`OpenAiCompatProvider` 加 `prompt_template` 字段经 `build_provider` 透传。改动文件：`config.rs`、`translate/prompt.rs`、`translate/openai_compat.rs`、`translate/mod.rs`、`api.ts`、`Settings.vue`。）

2026-06-30（**DeepSeek 翻译改造**：① 模型不再硬编码——`DeepSeekConfig.model` 默认空串，设置页填 Key 后调新命令 `list_deepseek_models`（`GET {base_url}/models`）动态拉取模型 id 填下拉，下拉可手输兼容第三方端点；空 model 翻译时报错"请先选择模型"。② 新增思考模式配置 `reasoning_enabled`（默认 false，翻译简单场景官方建议关）+ `reasoning_effort`（high/max）。请求体组合：关→`thinking:{type:"disabled"}`；开→`thinking:{type:"enabled"}`+`reasoning_effort`。删旧默认 `deepseek-chat` + 过时注释。⚠️ **DeepSeek API 唯一依据为中文官方文档 `api-docs.deepseek.com/zh-cn/`，英文版已过时**——见 DESIGN §4.3「DeepSeek API 事实基准」。改动文件：`config.rs`、`translate/openai_compat.rs`、`translate/mod.rs`、`src-tauri/commands/models.rs`、`src-tauri/main.rs`、`api.ts`、`Settings.vue`。）

2026-06-30（**框选→结果窗流程重构**：旧 `select_region` 是一个干完全部（裁剪→OCR→翻译→配对→写历史→缓存结果）的大命令，框选抬起后选区窗卡在"识别中…"几秒才一次性弹带译文的窗口。拆成三层命令 `crop_region`/`recognize_region`/`translate_region`：抬起仅跑 `crop_region`（几十 ms）即开结果窗显示原图→"正在识别"→图上原位显示原文→"正在翻译"→替换为译文。删 `select_region`/`get_last_result`/`state.last_result`/`SelectResult`/`run_ocr_translate`/`OcrTranslateOutcome`；核心管线拆为 `run_ocr`+`run_translate` 两纯函数（集成测试相应改写）。新增 `state.last_crop`（裁剪图路径+图）+`last_ocr`（OCR 行）两接力缓存，沿用现有"后端缓存+前端主动拉取"反竞态模式，不引入事件。前端 `api.ts`/`Capture.vue`/`Result.vue` 三件套同步重构。）

2026-06-29（**截图性能**：全屏临时图 PNG→BMP，框选前延迟从 ~1.3s 降到 ~0.3s（PNG 单线程编码是瓶颈，BMP 无压缩）。**修结果窗口一闪而过 bug**：选区结果原走 Pinia 跨窗口传递，但 Tauri 多窗口 JS 上下文隔离、Pinia 不共享，结果窗口 `Result.vue` 读到 `store.lastResult === null` 即 `close()` 自杀。改用与截图缓存同款的反竞态模式：`select_region` 写 `state.last_result`，新增 `get_last_result` 命令，`Result.vue onMounted` 主动拉取；删孤立的 `stores/capture.ts`。架构迁移：egui → Tauri 2 + Vue 3 + Naive UI。删 `crates/snaptext-app`、`wix/`、`build-msi.ps1`；新增 `src-tauri/`（Rust 后端，命令层 + 系统集成）+ `src/`（Vue 前端）。core 100% 复用。译文图上原位覆盖、历史 V002、行级译文逻辑从旧 orchestrator 搬到 `src-tauri/commands/`。**补集成测试**：核心管线抽成纯函数 `run_ocr_translate`，用 mock Provider 覆盖端到端（取代随 crate 删除丢失的 orchestrator full_pipeline）；src-tauri 测试 6→18→20。**修白屏**：Naive UI 组件未注册导致页面空白，加 `unplugin-vue-components` + `NaiveUiResolver` 按需自动注册。**bug 修复**（见各模块 ⚠️ 标记）：`dao::row_to_record` 列索引错位致带截图记录 list 崩溃；`HistoryStore` 加 `get_screenshot` 取代全表拉 BLOB；`monitor_to_info` 用真实 DPI scale；`capture_wgc` 会话 stop 收尾；`crop_frame` 越界 clamp）

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
├── Cargo.toml                  🟢 workspace 根（members: snaptext-core + src-tauri）
├── Cargo.lock                  🟢
├── rust-toolchain.toml         🟢 stable（1.80+）
├── package.json                🟢 前端依赖 + 脚本（vite/tauri）
├── vite.config.ts              🟢 前端构建（dev port 1420；Naive UI 按需引入 unplugin-vue-components + NaiveUiResolver）
├── tsconfig.json               🟢 前端类型
├── index.html                  🟢 前端入口
├── README.md                   🟡 骨架
├── LICENSE                     🟢 MIT
├── AGENTS.md                   🔒 项目规范（人工维护）
├── .github/workflows/          🟢 CI（release.yml：push tag 触发云端打包发布）
├── .gitignore                  🟢（含 node_modules/dist/src-tauri）
├── docs/                       本文档目录（含 RELEASE.md 发布手册）
├── crates/
│   └── snaptext-core/          🟢 库 crate（纯逻辑层，100% 复用）
├── src-tauri/                  🟢 Tauri 2 二进制（Rust 后端：命令层 + 系统集成）
└── src/                        🟢 Vue 3 前端（Naive UI）
```

> **为什么有三个 src 开头的目录**：名字分别来自三个工具链的约定，非项目自定义——
> `src/`（Vite 默认前端入口）、`src-tauri/`（Tauri CLI 强约定，CLI 靠它定位后端项目）、
> `crates/snaptext-core/`（Cargo workspace 惯例，crate 分组目录）。
> `src` 与 `src-tauri` 视觉撞名是所有 Tauri 项目的固有特征，靠"前端=TS / 后端=Rust"语言差异区分。


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
| `windows_capture.rs` | `WindowsCaptureProvider` 实现（WGC 优先 + DXGI fallback） | `struct WindowsCaptureProvider` | `windows-capture`, `windows`(DPI 查询), `image`, `tokio`, `async-trait` |

**修改约束**：动 `mod.rs` 的 trait 签名 = 同步动 `windows_capture.rs` + 所有调用方（`orchestrator.rs`）。

**DPI 与坐标**（单屏正确，多屏 origin 未做）：`monitor_to_info` 经 Win32 `GetDpiForMonitor` 算真实 `scale = dpi/96.0`，供前端把框选的逻辑坐标换算成物理坐标（截图帧是物理像素）。`x/y` 固定 0（多屏 origin 需 `GetMonitorInfoW` 的 `rcMonitor`，未实现）。`capture_wgc` 在 `recv_timeout` 成败两路都调 `control.stop()` 回收会话（曾因超时 `?` 提前返回跳过 stop，泄漏 WGC 后台会话）。`crop_frame`（src-tauri）把 bbox clamp 到图像边界，越界返回 Err 而非 panic。

### src/ocr/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `OcrProvider` trait 定义 | `trait OcrProvider` | `types`, `error`, `image`, `async-trait` |
| `paddleocr.rs` | `PaddleOcrProvider` 实现（oar-ocr 封装） | `struct PaddleOcrProvider`（持 `Arc<OAROCR>`） | `oar-ocr`, `tokio`（spawn_blocking） |
| `preprocess.rs` | 图像预处理（oar-ocr 后端内部完成 resize/归一化，此处仅转 `RgbImage`） | `to_rgb` | `image` |
| `postprocess.rs` 🟢 | OCR 输出后处理（去 CJK 空格 / 合并换行 / trim） | `clean_ocr_text` | — |

**关键事实**（DU-04 落地）：oar-ocr 的 `OAROCR` 已实现 `Send + Sync`（内部 `Arc<Session>`），用 `Arc<OAROCR>` 跨线程共享，无需手动 `Mutex`（AI_GUIDE §3.1 陷阱已被 oar-ocr 解决）。ort 2.0 的 onnxruntime.dll 经 `download-binaries` 自动下载（R2 通过）。

**ort session 内存优化配置**（2026-07-02 落地，对症"大图 OCR 后内存不回落"）：`PaddleOcrProvider::new` 构造 `OAROCRBuilder` 时显式传 `OrtSessionConfig`——关 `memory_pattern`（oar-ocr 用动态 shape/Type0 resize，ort 默认开的 mem pattern 会按"见过的最大 shape"扩容并永久保留，ort 官方明确说动态尺寸应关）、`intra_threads` 封顶 4（默认用满全核，各线程临时 buffer 叠加抬高峰值）、`image_batch_size(2)` + `region_batch_size(16)`（oar-ocr 默认 det=8 / rec=推荐，框选场景一次一张图，大 batch 无收益纯费内存）。ort 2.0.0-rc.12 **无"推理后释放 arena"API**（arena 仅随 session drop 释放），故靠关 pattern + 降 batch 从源头压住峰值；`OrtSessionConfig` 经 oar-ocr 的 `ort_session()` 透传口子传给 det/rec 所有 session，无需 fork。

### src/translate/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `TranslationProvider` trait + `common_pairs` + 工厂 + 重试判定 + 统一重试包装 + HTTP 错误分类 | `trait TranslationProvider`, `build_provider`, `is_retryable(&CoreError)`, `with_retry(max_retries, do_once)`, `classify_send_err(e)`, `ensure_2xx(resp)`, `RETRY_BASE_MS` | `types`, `config`, `error`, `async-trait` |
| `prompt.rs` | LLM prompt 模板渲染 | `DEFAULT_PROMPT_TEMPLATE` 常量（单一数据源）、`default_prompt_template()`（命令层取值入口）、`render_translate_prompt(req, template)` | `types` |
| `openai_compat.rs` | OpenAI 兼容（DeepSeek 走此路） | `OpenAiCompatProvider`、`OpenAiCompatParams`（构造入参 struct，收敛 10 参数） | `reqwest`, `serde_json` |
| `deepl.rs` | DeepL REST API | `DeepLProvider` | `reqwest` |
| `microsoft.rs` 🟢 | Azure Translator（DU-18） | `MicrosoftProvider` | `reqwest` |
| `baidu.rs` 🔴 P2 | 百度翻译（含 sign MD5，无 key 验证，推迟） | — | — |
| `postprocess.rs` 🟢 | 译文后处理（去引号 / trim / 去前缀） | `clean_translation` | — |

**关键事实**（DU-05 落地）：
- **DeepSeek 模型不硬编码**：`DeepSeekConfig.model` 默认空串，设置页填 Key 后动态拉取（`GET /v1/models`，命令 `list_deepseek_models`）+ 可手输。空 model 翻译时报错"请先选择模型"。
- **DeepSeek 思考模式**：`reasoning_enabled`（默认 false）+ `reasoning_effort`（high/max）。关→`thinking:{type:"disabled"}`；开→`thinking:{type:"enabled"}`+`reasoning_effort`。**DeepSeek API 唯一依据为中文官方文档**（见 DESIGN §4.3「DeepSeek API 事实基准」）。
- Provider 构造时拿共享 `reqwest::Client`（CONVENTIONS §3.6），不每次 new。
- 超时：LLM 30s / MT 10s；错误归类 `TranslateError`（Timeout / Api{status,body} / Parse / Request）。
- **重试**（2026-06-30 接线，2026-06-30 收敛）：三个 Provider 各自的 `translate()` 调用统一的 `translate::with_retry(max_retries, do_once)`（泛型 helper），按 `max_retries`（默认 2）指数退避（`RETRY_BASE_MS=500ms` × `2^attempt`）。`is_retryable` 判定：重试 Timeout/Request/5xx/429，不重试其他 4xx/Parse/UnsupportedPair。各 Provider 把单次 HTTP 逻辑抽成私有 `do_once`，`with_retry` 在外层包重试循环调它（旧版三 Provider 各内联一份逐字相同的重试循环，已收敛）。
- **HTTP 错误分类**（2026-06-30 收敛）：各 Provider 的 `do_once` 内 reqwest 错误→`TranslateError` 分类（超时/其余）走 `classify_send_err(e)`，非 2xx→`Api{status,body}` 走 `ensure_2xx(resp)`，两个 helper 在 `translate/mod.rs` 供三 Provider 复用（旧版各内联一份逐字相同的分类样板）。

### src/history/ 🟢

| 文件 | 职责 | 关键 API | 依赖 |
|---|---|---|---|
| `mod.rs` | `HistoryStore` trait + `HistoryRecord` / `HistoryStats` | `trait HistoryStore`, `HistoryRecord` | `types`, `async-trait` |
| `sqlite_store.rs` | sqlite + r2d2 连接池（size 5）实现；`spawn_db` 私有 helper 统一 8 个 async 方法的 `spawn_blocking` 样板（pool.clone → get? → dao → 双层 map_err） | `SqliteHistoryStore::open/open_default/cleanup_blocking`、`spawn_db(label, f)` | `rusqlite`, `r2d2_sqlite`, `r2d2`, `dao`, `migration`, `chrono` |
| `dao.rs` | CRUD（`insert`/`list`/`search`/`cleanup`/`delete_by_id`/`clear_all`） | `dao::insert`, `dao::list`, `dao::delete_by_id`, `dao::clear_all` | `rusqlite`, `chrono`, `serde_json` |
| `schema.sql` | DDL（DESIGN §5.6） | 资源文件 | — |
| `migration.rs` | `PRAGMA user_version` 迁移（TARGET_VERSION=2） | `run_migrations` | `rusqlite` |
| `migrations/V001__initial.sql` | 初始迁移脚本 | 资源文件（include_str） | — |
| `migrations/V002__image_and_lines.sql` | 加 `screenshot_png`/`ocr_lines_json`/`line_translations_json` | 资源文件（include_str） | — |

**约定**：连接池 size 5，读写均 `spawn_blocking`。`insert`/`list`/`search`/`stats`/`delete_before`/`delete_by_id`/`clear_all`/`cleanup`/`get_screenshot` 已实现（DU-06 + DU-15）。`get_screenshot(id)` 按 id 精确查单列 `screenshot_png`，供历史面板详情取图（取代旧 `list(10000)` 全表拉 BLOB——既丢图又慢）。清理（retention_days + max_records）由 `state.rs::AppState::build` 在构造 history 后按 `config.history.auto_clean_on_start` 调一次 `cleanup_blocking`（2026-06-30 接线，此前为死代码）。

> ⚠️ 列索引陷阱：`dao::row_to_record` 用位置索引读列，`ocr_lines_json`(18)/`line_translations_json`(19) 紧跟 `screenshot_png`(17, BLOB)。曾因索引错位（误读 17/18）导致任何带截图的记录 `list` 时 BLOB→String 类型不符而崩溃——已修，并有 `list_reads_back_v002_fields_when_populated` 回归测试。

**HistoryRecord 字段**（V002 扩展）：基础文本字段 + `bbox` + V002 新增 `screenshot_png: Option<Vec<u8>>`（选区截图 PNG）、`ocr_lines: Option<Vec<OcrLine>>`（行级 OCR，含 bbox）、`line_translations: Option<Vec<String>>`（与 ocr_lines 按索引配对的逐行译文）。三者配合译文图上原位覆盖与历史面板回看。`id: i64`（主键，insert 时 0，list 读回填充）。

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

**首次引导 / 目标语言**：`GeneralConfig.onboarding_completed`（首启引导是否完成，false→启动 `router.replace('/onboarding')` 进引导页；用户走完引导调 `complete_onboarding` 置 true）；`TranslateConfig.target_lang`（翻译目标语言，源语言固定 `Auto`）。

---

## src-tauri/ 🟢（Tauri 2 二进制：Rust 后端）

Tauri 应用后端。命令层包装 `snaptext-core` 的 Provider，系统集成（托盘/热键/单实例/剪贴板），窗口管理。**取代旧 egui 的 orchestrator + ui + hotkey + tray + clipboard 全部**。

### src/main.rs 🟢

`tauri::Builder` 入口。职责：
1. 注册插件（single-instance / global-shortcut / clipboard-manager / dialog）
2. `setup`：初始化 tracing → 构造共享 `reqwest::Client` → `AppState::build` → `manage` → 注册全局热键 → 构建托盘（模型下载已移至前端引导页主动触发，不再在启动时同步下载）
3. `invoke_handler`：注册全部 `#[tauri::command]`

### src/state.rs 🟢

`AppState`（`app.manage` 注入，命令用 `State<'_, AppState>` 取用）。持有 `Arc<dyn CaptureProvider>`、`Mutex<Option<Arc<dyn OcrProvider>>>`（模型缺失降级 None，下载后 `reload_ocr_provider` 重建）、`Mutex<Option<Arc<dyn TranslationProvider>>>`、`Arc<dyn HistoryStore>`、`Mutex<Config>`、`reqwest::Client`、`Mutex<Vec<CapturedFrame>>`（截图缓存）、`Mutex<Option<LastCrop>>`（裁剪缓存，三层命令接力）、`Mutex<Option<LastOcr>>`（OCR 缓存，三层命令接力）、`Mutex<Option<String>>`（`hotkey_error`，热键注册状态）。**取代旧 Orchestrator 的 Provider 持有角色**——Tauri 命令直接读 state 调 Provider，无 channel。`captured`/`last_crop`/`last_ocr` 三套缓存都是反竞态模式：先写后端，子窗口 `onMounted` 主动命令拉取（Pinia 不跨窗口共享，emit 事件会因子窗口未加载完而丢失）。`hotkey_error` 同款反竞态：启动注册或 save_config 重注册时写入，前端经 `get_hotkey_status` 拉取提示（详见 main.rs 热键降级）。`build()` 里 OCR/翻译 Provider 模型/Key 缺失均降级 None（不崩），构建完 history 后按 `config.history.auto_clean_on_start` 启动时调一次 `cleanup_blocking`（retention_days + max_records）。

### src/commands/ 🟢

| 文件 | 命令 | 包装的 core API |
|---|---|---|
| `mod.rs` | 模块导出 | — |
| `config_cmd.rs` | `get_config` / `save_config`（写盘+重建翻译 Provider+重注册热键，结果写回 `hotkey_error`）/ `check_translate_ready` / `get_default_prompt`（返回 `prompt.rs::DEFAULT_PROMPT_TEMPLATE` 供前端只读展示，单一数据源）/ `get_hotkey_status`（返回热键注册状态 `Option<String>`，None=成功/Some=失败原因，前端用于提示）/ `complete_onboarding`（置 `config.general.onboarding_completed=true` 并落盘，不复用 `save_config`——只置标志，不重建 Provider/不重注册热键）/ `reload_ocr_provider`（用当前 tier 重建 OCR Provider 写回 `state.ocr`，模型下载完成后即时生效无需重启；启动时模型缺失降级为 None） | `Config::load/save`、`build_provider`、`PaddleOcrProvider::new`、`translate::prompt::default_prompt_template` |
| `models.rs` | `models_ready` / `download_models`（后台线程+专用 runtime，进度经 `download-progress` 事件推送）/ `list_deepseek_models`（GET `{base_url}/models` 拉取 DeepSeek 模型 id 列表，供设置页下拉） | `model_manager::is_models_ready`、`downloader::download_models`、`reqwest`（list_deepseek_models 直接发 HTTP） |
| `capture.rs` | `capture_all`（截全屏+缓存帧+写临时 BMP+返回 `MonitorDto`）/ `get_last_capture`（重建 DTO）/ `save_image_copy`（复制结果图到目标路径） | `CaptureProvider::capture_all` |
| `ocr_translate.rs` | `crop_region`（裁剪缓存帧+写临时 PNG+缓存进 `last_crop`+返回路径）/ `recognize_region`（从 `last_crop` 取图 OCR+`align` 前的原文清洗+缓存进 `last_ocr`+返回 OCR 行与整段原文）/ `translate_region`（从 `last_ocr` 取原文翻译+`align_lines` 配对+写历史+返回逐行译文/整段译文/Provider/耗时）；核心管线抽成纯函数 `run_ocr`+`run_translate`（不依赖 Tauri，便于 mock 测试）；图像编码 helper `encode_png`（PNG 写盘/入库复用）+ `read_last_crop_png`（取裁剪图编码入历史） | `OcrProvider::recognize`、`TranslationProvider::translate`、`HistoryStore::insert` |
| `history.rs` | `history_list` / `history_search` / `history_get_screenshot`（按 id 查单列截图→base64 data URL）/ `history_delete` / `history_clear` / `history_stats`；`to_dto` 纯函数剥离 `screenshot_png` 二进制 | `HistoryStore::list/search/get_screenshot/delete_by_id/clear_all/stats` |

**关键约束**（迁移自探索）：`CapturedFrame`/`DynamicImage` 不可序列化，截图与裁剪在 Rust 内完成，前端只收元数据 + 图片路径（前端 `convertFileSrc` 转 webview URL）。`HistoryRecord.screenshot_png` 二进制走单独 `history_get_screenshot`（base64）。

**可测性设计**：`run_ocr`（OCR+后处理）、`run_translate`（翻译+`align_lines` 配对）、`align_lines`（行级配对）、`crop_frame`（坐标换算）、`to_dto`（历史 DTO 转换）均为不依赖 Tauri 的纯函数，用 mock Provider 集成测试覆盖。src-tauri 共 18 个测试：6 `align_lines` 边界 + 6 管线/crop 集成（取代旧 orchestrator full_pipeline）+ 4 history DTO + 2 capture 文件复制。（管线拆分后 run_ocr/run_translate 测试数以 `cargo test` 实际为准）

### src/window.rs 🟢

窗口管理 + 托盘：
- `open_capture_window` / `trigger_capture`：选区窗口（全屏无边框置顶透明，加载 `index.html#/capture`）
- `open_panel`：设置/历史窗口（普通窗口，已存在则聚焦）
- `build_tray`：系统托盘（显示/设置/历史/退出），用 Tauri 原生 `TrayIconBuilder` + `Menu`

### tauri.conf.json / capabilities/default.json 🟢

- `tauri.conf.json`：主窗口定义（label=main, `index.html#/home`）、前端指向（devUrl:1420 / frontendDist:../dist）、打包（nsis）
- `capabilities/default.json`：权限集（窗口创建/关闭/聚焦、webview、event、global-shortcut、clipboard、dialog）

---

## src/ 🟢（Vue 3 前端，Naive UI）

多窗口共用一套路由表，靠 hash 路由（`#/home`/`#/onboarding`/`#/settings`/`#/history`/`#/capture`/`#/result`）区分窗口内容。

| 文件 | 职责 |
|---|---|
| `main.ts` | createApp + Pinia + router + 全局样式 |
| `App.vue` | n-config-provider（中文 locale）+ message/dialog provider + router-view |
| `api.ts` | 所有 Tauri 命令的 TS 封装 + DTO 类型（与 Rust 端对齐）+ `fileSrc`(convertFileSrc) |
| `router.ts` | hash 路由，6 个 view |
| `styles/global.css` | 全局浅色主题 CSS 变量 |
| `views/Home.vue` | 主窗口首页：状态卡（模型/翻译就绪态）+ 截图/设置/历史入口；`onMounted` 判断 `onboarding_completed===false` 则 `router.replace('/onboarding')` |
| `views/Onboarding.vue` | 首启引导：三步向导（快捷键→下载 OCR 模型→翻译配置可选）。单标志位 `onboarding_completed` 持久化——仅"完成/跳过"置 true，中途关闭仍 false→下次重进。下载步监听 `download-progress`/`download-done`（按 det/rec/dict 三段权重折算进度），`onBeforeUnmount` 清理 unlisten；模型幂等检查（`is_models_ready`）已就绪跳过；配置末尾统一 `save_config` 一次 |
| `views/Settings.vue` | 设置面板：8 分类（通用/快捷键/截图/OCR/翻译/界面/历史/关于），草稿机制保存；OCR 分类含**模型下载兜底入口**（模型被删/首次跳过引导导致 onboarding_completed=true 但模型缺失时，用户可在此重新下载，状态 tag + 下载按钮 + 进度条 + 错误重试，下载前先落盘 tier 防档位错配） |
| `views/History.vue` | 历史面板：左列表 + 右详情（截图 base64 + 原文/译文）+ 搜索/刷新/单删/清空 |
| `views/Capture.vue` | 选区窗口：全屏 Canvas 显示截图 + 鼠标拖拽框选 + 抬起调 `crop_region`（仅裁剪+写临时图）即创建/复用结果窗口、隐藏选区窗。**结果窗口复用**：crop 后 `WebviewWindow.getByLabel("result")` 判断，存在则 `emit("result-refresh")` + show + setFocus，不存在才 `new`。**窗口以 hidden 创建，首次 `draw()` 画上截图 + 双层 rAF 等合成后再 `show()`，消除创建→绘制间的白闪** |
| `views/Result.vue` | 结果窗口：原图→"正在识别"→原位显示原文→"正在翻译"→原位替换译文，两阶段渲染；工具栏（原文/译文切换、复制、保存、关闭）。**文字层为 DOM `<div>`（按 OCR 行 bbox 绝对定位）而非 canvas 位图——可鼠标选中部分文字复制（松开自动复制 + Ctrl+C）**。**窗口复用模式**：监听 `result-refresh` 事件（Capture.vue 第二次框选时 emit），onMounted 流程抽成 `refresh()` 供首次与复用共用，generation 守卫丢弃 stale 异步结果，`img.src` 加 `?t=` 防缓存 |
| `stores/config.ts` | 配置 Pinia（load/save） |
> 注：原 `stores/capture.ts` 已删除——选区结果跨窗口传递改走后端缓存 + 命令拉取（Pinia 不跨窗口共享，见 `state.rs`）。

**截图翻译交互**（桌面框选+弹窗，三层命令分阶段反馈）：
1. 热键 → Rust `trigger_capture_cmd` 创建选区窗口
2. Capture.vue `get_last_capture` → 全屏图 → 框选 → `crop_region(monitor_id, bbox)`（仅裁剪+写临时 PNG，几十 ms）→ 立即创建结果窗口 → 关闭选区窗
3. Result.vue `onMounted` 依次：渲染原图 → `recognize_region`（OCR，"正在识别"）→ 图上原位显示原文 → `translate_region`（翻译+配对+落库，"正在翻译"）→ 原位替换译文

---

## scripts/

| 文件 | 用途 | 归属 |
|---|---|---|
| `dev.bat` | 开发启动器（检查 Node/Cargo 依赖 → `npm run tauri dev`）；`cd /d "%~dp0.."` 切到项目根 | 开发 |
| `build.bat` | 打包脚本（构建 NSIS 安装包）；`cd /d "%~dp0.."` 切到项目根 | DU-13 |
| `reset-onboarding.bat` | 重置 `onboarding_completed=false`（保留 Key/模型，仅让引导页下次重显）；改 `%APPDATA%\SnapText\config.toml` | 开发辅助 |
| `download-models.ps1` | 离线下载 PP-OCRv6 模型（开发/无网环境辅助；⚠️ 脚本仍下载到 `%APPDATA%\SnapText\models\`，与现便携模式 `models\` 路径不一致，使用时手动调整） | DU-13 |
| `stress-test.ps1` | 稳定性压测（模拟热键 + 鼠标，连续框选） | DU-12 验收 |

> 已删（2026-06-29 迁移 Tauri 时清理）：`build-msi.ps1`（改用 Tauri NSIS/MSI bundler）、`mirror-models.ps1`、`verify-deps.ps1`。

---

## .github/workflows/

| 文件 | 用途 |
|---|---|
| `release.yml` | 发布工作流。`push` 形如 `v*` 的 tag（或手动 `workflow_dispatch`）→ `windows-latest` runner 上 `npm ci` + Rust stable + `tauri-apps/tauri-action@v0` 云端打包，自动产出 NSIS(`*-setup.exe`) 并创建 GitHub Release。需 `permissions: contents: write`。 |

> **为什么不本地发版**：本地 `scripts/build.bat` 也能出包，但 CI 化后发版只需 `git tag vX.Y.Z && git push origin vX.Y.Z`，长期复用、可追溯、未来易扩展跨平台 target。

---

## 依赖图（架构层）

```
   ┌─────────────────────────────────────────────┐
   │  src/ (Vue 3 前端)                            │
   │  views/ · api.ts(invoke) · stores/           │
   └───────────────────┬─────────────────────────┘
                       │ Tauri IPC (invoke / event)
                       ▼
   ┌─────────────────────────────────────────────┐
   │  src-tauri/ (Rust 后端)                       │
   │  commands/ (#[tauri::command])               │
   │  state.rs (AppState: 持 Provider)            │
   │  window.rs (窗口/托盘) · main.rs             │
   └───────────────────┬─────────────────────────┘
                       │ snaptext-core = { workspace = true }
                       ▼
   ┌─────────────────────────────────────────────┐
   │  snaptext-core (纯逻辑库，100% 复用)          │
   │  capture / ocr / translate / history /       │
   │  model_manager / config / types              │
   └─────────────────────────────────────────────┘
```

**铁律**：箭头方向严格遵循。`snaptext-core` 是纯逻辑层，不依赖任何 UI/系统框架。`src-tauri` 依赖 core 并包装为 Tauri 命令。`src`（前端）只能经 Tauri IPC 调命令，不直接碰 core。core 内 `types` 是叶节点。

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
