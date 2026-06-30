//! 窗口管理：选区窗口 / 结果窗口 / 设置窗口 / 历史窗口的创建 + 托盘。

use tauri::{
    menu::{Menu, MenuItem},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};

/// 创建（或聚焦已存在的）选区窗口。
///
/// 全屏无边框置顶，覆盖整个主屏。Capture.vue 挂载后读已缓存的截图渲染。
pub fn open_capture_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(existing) = app.get_webview_window("capture") {
        existing.show()?;
        existing.set_focus()?;
        return Ok(());
    }
    // 创建时隐藏：选区窗口从创建到 Canvas 画上截图之间有 WebView 冷启动 +
    // 拉取截图 + 解码的空窗期，默认白底会整屏白闪。先 hidden 创建，Capture.vue
    // 首次 draw() 完成后主动 show()，让窗口"直接以截图内容出现"，消除白闪。
    let _win = WebviewWindowBuilder::new(app, "capture", WebviewUrl::App("index.html#/capture".into()))
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
/// 关键时序：**先截图（此时无遮挡）→ 再创建选区窗口**。
/// 若先开窗口再截图，全屏窗口会盖住桌面，截到的就是白屏自己。
/// 截图结果存 state.captured，Capture.vue onMounted 主动调 get_last_capture 拉取
/// （不用 emit 事件，避免窗口页面未加载完时事件丢失的竞态）。
#[tauri::command]
pub async fn trigger_capture_cmd(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let start = std::time::Instant::now();
    // 1. 先截图（无遮挡，截到真实桌面）。
    crate::commands::capture::do_capture_all(&app, state.inner()).await?;
    tracing::info!(capture_total_ms = start.elapsed().as_millis(), "trigger_capture 截图阶段完成");
    // 2. 再开窗（Capture.vue 挂载后主动拉取已缓存截图）。
    let win_start = std::time::Instant::now();
    open_capture_window(&app).map_err(|e| format!("打开选区窗口失败：{e}"))?;
    tracing::info!(open_window_ms = win_start.elapsed().as_millis(), total_ms = start.elapsed().as_millis(), "trigger_capture 选区窗口已创建");
    Ok(())
}

/// 打开或聚焦一个普通窗口（设置 / 历史）。已存在则聚焦。
pub fn open_panel(app: &AppHandle, label: &str, title: &str, hash: &str, w: f64, h: f64) {
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    if let Err(e) = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html{hash}").into()))
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
            "settings" => open_panel(&app_handle, "settings", "SnapText 设置", "#/settings", 720.0, 560.0),
            "history" => open_panel(&app_handle, "history", "SnapText 历史记录", "#/history", 880.0, 600.0),
            "quit" => app_handle.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

