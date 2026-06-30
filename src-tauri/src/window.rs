//! 窗口管理：选区窗口 / 结果窗口 / 设置窗口 / 历史窗口的创建 + 托盘。

use tauri::{
    menu::{Menu, MenuItem},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

/// 启动时预创建选区窗口并隐藏（WebView2/Vue 实例常驻预热）。
///
/// 热键时仅 `show_capture_window`（已存在的窗口直接 show），省掉每次创建窗口的
/// ~400ms WebView2 冷启动（参照 Snipaste/Flameshot 模式）。窗口 hidden 不遮挡桌面，
/// 截图完成后再 show。Capture.vue 启动后监听 `capture-ready` 事件，收到才绘制截图。
pub fn ensure_capture_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window("capture").is_some() {
        return Ok(());
    }
    // 创建时隐藏：选区窗口从创建到 Canvas 画上截图之间有 webview 初始化空窗期，
    // 默认白底会整屏白闪。先 hidden 创建，Capture.vue 首次 draw() 完成后主动 show()，
    // 让窗口"直接以截图内容出现"，消除白闪。
    let _win = WebviewWindowBuilder::new(
        app,
        "capture",
        WebviewUrl::App("index.html#/capture".into()),
    )
    .title("SnapText 选区")
    .fullscreen(true)
    .decorations(false)
    .always_on_top(true)
    .resizable(false)
    .visible(false)
    .build()?;
    Ok(())
}

/// Tauri 命令：前端"开始截图"按钮调用，与全局热键走同一路径。
///
/// 关键时序：**先截图（选区窗口仍 hidden 不遮挡）→ emit 事件通知前端绘制**。
/// 选区窗口启动时已预创建（main setup），所以前端页面早已加载，emit 事件可靠送达
/// （旧版"子窗口未加载完丢事件"的竞态已不存在）。
///
/// show 由前端负责：Capture.vue 收到 `capture-ready` 后从 shot:// URI 拉截图、
/// draw 画上 canvas、双层 rAF 等合成完才 show。若后端 emit 后立即 show，窗口会
/// 在 canvas 绘制前暴露白底（白闪），故后端不 show。
#[tauri::command]
pub async fn trigger_capture_cmd(app: AppHandle) -> Result<(), String> {
    let state = app.state::<crate::state::AppState>();
    let start = std::time::Instant::now();
    // 1. 先截图（选区窗口 hidden，截到真实桌面）。
    let dtos = crate::commands::capture::do_capture_all(state.inner()).await?;
    tracing::info!(
        capture_total_ms = start.elapsed().as_millis(),
        "trigger_capture 截图阶段完成"
    );
    // 2. emit 通知前端绘制（绘制完前端自行 show，确保无白闪）。
    let _ = app.emit("capture-ready", &dtos);
    tracing::info!(
        total_ms = start.elapsed().as_millis(),
        "trigger_capture 完成（已 emit，前端将绘制并 show）"
    );
    Ok(())
}

/// 打开或聚焦一个普通窗口（设置 / 历史）。已存在则聚焦。
pub fn open_panel(app: &AppHandle, label: &str, title: &str, hash: &str, w: f64, h: f64) {
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    if let Err(e) = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html{hash}").into()),
    )
    .title(title)
    .inner_size(w, h)
    .resizable(true)
    .center()
    .build()
    {
        tracing::warn!(error = %e, label, "打开面板窗口失败");
    }
}

/// 构建系统托盘：显示 / 设置 / 历史 / 退出。
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "history", "历史记录", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &history, &quit])?;

    let app_handle = app.clone();
    let _tray = tauri::tray::TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().expect("缺默认图标"))
        .tooltip("SnapText")
        .menu(&menu)
        .on_menu_event(move |_tray, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app_handle.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "settings" => open_panel(
                &app_handle,
                "settings",
                "SnapText 设置",
                "#/settings",
                720.0,
                560.0,
            ),
            "history" => open_panel(
                &app_handle,
                "history",
                "SnapText 历史记录",
                "#/history",
                880.0,
                600.0,
            ),
            "quit" => app_handle.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
