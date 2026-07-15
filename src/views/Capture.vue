<script setup lang="ts">
// 选区窗口（常驻隐藏）：全屏显示主屏截图 + Canvas 鼠标框选 + 抬起仅调 crop_region。
//
// 窗口在 main setup 时预创建并隐藏（WebView2/Vue 已热），热键触发时：
// Rust 端先截图（窗口仍 hidden 不遮挡）→ emit("capture-ready") → show。
// 因窗口常驻、页面早已加载，事件可靠送达（旧版"子窗口未加载完丢事件"竞态已不存在）。
// Esc/抬起成功后 hide() 而非 close()（窗口常驻复用）。
// 截图经 shot:// 自定义协议从内存直接取 BMP（不再写临时文件）。
import { onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import { api, type MonitorDto } from "../api";

// 结果窗口固定 label：复用模式（已存在则 emit 刷新事件 + show，不存在才新建）。
// 旧实现每次 new 同 label 窗口，第二次起 Tauri 发现已存在不重建，onMounted 不重跑 →
// 结果窗停在第一次内容（bug）。改为复用：第二次框选时 Result.vue 收到 result-refresh
// 事件重新拉取 last_crop → OCR → 翻译。
const RESULT_LABEL = "result";

const canvas = ref<HTMLCanvasElement | null>(null);
const status = ref("加载截图…");
const primary = ref<MonitorDto | null>(null);
const img = new Image();
const dragStart = ref<{ x: number; y: number } | null>(null);
const dragCur = ref<{ x: number; y: number } | null>(null);
// 选区蒙版不透明度（读 config.ui.overlay_dim_alpha，默认 0.5）。
const overlayAlpha = ref(0.5);
// 窗口常驻：每次 show 前必须重置为 false，否则第二次截图直接 show 不等绘制 → 白闪。
// 收到 capture-ready（后端截图就绪）时置 false，draw() 首次画上截图 + 双层 rAF 后才 show。
const firstDrawn = ref(false);
// unlisten 卸载函数（窗口常驻，理论上 onMounted 只跑一次，保留以便清理）。
let unlisten: (() => void) | null = null;

onMounted(async () => {
  // 读蒙版不透明度配置（不阻塞截图加载，失败用默认）。
  api.getConfig().then((cfg) => {
    overlayAlpha.value = cfg.ui.overlay_dim_alpha ?? 0.5;
  }).catch(() => {});

  // 监听后端"截图就绪"事件：热键截图完成后触发，此时窗口即将 show，
  // 收到即重置绘制状态 + 拉取截图渲染。窗口常驻所以事件可靠。
  try {
    unlisten = await listen<MonitorDto[]>("capture-ready", (event) => {
      firstDrawn.value = false;
      loadAndDraw(event.payload);
    });
  } catch (e) {
    console.error("capture-ready listen 失败", e);
  }

  // 兜底：若窗口是首次加载（还没收到任何 capture-ready，但 state.captured 已有缓存，
  // 如调试时手动 reload），主动拉取一次。
  try {
    const monitors = await api.getLastCapture();
    if (monitors.length > 0) {
      loadAndDraw(monitors);
    }
  } catch {
    // 无缓存正常（首次启动未截图），等 capture-ready 即可。
  }

  // Esc 关闭（隐藏）窗口。
  window.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      getCurrentWindow().hide();
    }
  });
});

// 拉取截图配置 + 渲染：从 monitors 选主屏，img.src 用 shot:// URI 直接从内存取 BMP。
function loadAndDraw(monitors: MonitorDto[]) {
  primary.value = monitors.find((m) => m.primary) ?? monitors[0] ?? null;
  if (!primary.value) {
    status.value = "未找到显示器";
    return;
  }
  status.value = "拖动鼠标框选文字区域 · Esc 取消";
  img.onload = () => {
    draw();
  };
  // shot_path 已是 shot:// URI（http://shot.localhost/<id>），直接当 src，从内存取 BMP。
  // 加时间戳 query：URL 每次唯一，强制绕过 WebView2 HTTP 缓存——否则同一 monitor id
  // 的 shot:// URL 恒定，第二次起直接命中缓存显示旧截图（后端 state.captured 已是新帧）。
  img.src = `${primary.value.shot_path}?t=${Date.now()}`;
}

function pos(ev: MouseEvent): { x: number; y: number } {
  const c = canvas.value!;
  const r = c.getBoundingClientRect();
  return { x: ev.clientX - r.left, y: ev.clientY - r.top };
}

function onDown(ev: MouseEvent) {
  dragStart.value = pos(ev);
  dragCur.value = dragStart.value;
}

function onMove(ev: MouseEvent) {
  if (dragStart.value) {
    dragCur.value = pos(ev);
    draw();
  }
}

async function onUp() {
  if (!dragStart.value || !dragCur.value || !primary.value) return;
  const a = dragStart.value;
  const b = dragCur.value;
  const w = Math.abs(b.x - a.x);
  const h = Math.abs(b.y - a.y);
  if (w < 3 || h < 3) {
    // 太小，忽略。
    dragStart.value = null;
    dragCur.value = null;
    draw();
    return;
  }
  // 虚拟桌面坐标：屏幕内坐标 * scale + monitor 原点。
  const scale = primary.value.scale || 1;
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  const bbox = {
    x: Math.round(x * scale + primary.value.x),
    y: Math.round(y * scale + primary.value.y),
    w: Math.round(w * scale),
    h: Math.round(h * scale),
  };
  status.value = "处理中…";
  draw();
  try {
    // 仅裁剪+写临时图（几十 ms），不等 OCR/翻译。后端 crop_region 把裁剪图
    // 缓存进 last_crop，结果窗口 onMounted 依次调 recognize/translate。
    await api.cropRegion(primary.value.id, bbox);
    // 结果窗口复用：已存在则 emit 事件让 Result.vue 重跑刷新流程，否则首次新建。
    const existing = await WebviewWindow.getByLabel(RESULT_LABEL);
    if (existing) {
      await existing.emit("result-refresh");
      await existing.show();
      await existing.setFocus();
    } else {
      await new WebviewWindow(RESULT_LABEL, {
        url: "index.html#/result",
        title: "SnapText 译文",
        width: 800,
        height: 600,
        resizable: true,
        center: true,
      });
    }
    // 隐藏选区窗口（常驻复用，不 close）。
    dragStart.value = null;
    dragCur.value = null;
    await getCurrentWindow().hide();
  } catch (e) {
    status.value = `处理失败：${e}`;
  }
}

// 绘制：背景图 + 拖拽蒙版 + 选区框 + 尺寸标注。
function draw() {
  const c = canvas.value;
  if (!c || !primary.value || !img.complete || !img.naturalWidth) return;
  const ctx = c.getContext("2d")!;
  c.width = window.innerWidth;
  c.height = window.innerHeight;
  // 背景：截图按窗口尺寸拉伸（截图是物理像素，窗口是逻辑像素）。
  const scale = primary.value.scale || 1;
  ctx.drawImage(img, 0, 0, c.width, c.height);
  // 首次画上截图后才显示窗口（消除创建→绘制间的白闪），每次 show 前重置 firstDrawn。
  // 双层 rAF：drawImage 写 canvas 缓冲是同步的，但浏览器合成该帧到屏幕要等
  // 下一渲染帧。若 show() 早于合成，WebView2 默认白底会露一帧 → 短暂白闪。
  // 推迟 show 到两次 rAF 之后（约 +32ms），确保 canvas 帧已合成再显窗。
  if (!firstDrawn.value) {
    firstDrawn.value = true;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        getCurrentWindow().show().catch(() => {});
      });
    });
  }
  // 蒙版 + 选区。
  if (dragStart.value && dragCur.value) {
    const a = dragStart.value;
    const b = dragCur.value;
    const sel = {
      x: Math.min(a.x, b.x),
      y: Math.min(a.y, b.y),
      w: Math.abs(b.x - a.x),
      h: Math.abs(b.y - a.y),
    };
    ctx.fillStyle = `rgba(0,0,0,${overlayAlpha.value})`;
    // 四块蒙版。
    ctx.fillRect(0, 0, c.width, sel.y);
    ctx.fillRect(0, sel.y, sel.x, sel.h);
    ctx.fillRect(sel.x + sel.w, sel.y, c.width - sel.x - sel.w, sel.h);
    ctx.fillRect(0, sel.y + sel.h, c.width, c.height - sel.y - sel.h);
    // 选区边框。
    ctx.strokeStyle = "#0078d7";
    ctx.lineWidth = 1.5;
    ctx.strokeRect(sel.x, sel.y, sel.w, sel.h);
    // 尺寸标注（物理像素）。
    ctx.fillStyle = "#fff";
    ctx.font = "13px sans-serif";
    ctx.fillText(
      `${Math.round(sel.w * scale)}×${Math.round(sel.h * scale)} px`,
      sel.x + 4,
      sel.y + sel.h + 14
    );
  }
  // 顶部提示。
  ctx.fillStyle = "rgba(0,0,0,0.6)";
  ctx.fillRect(0, 0, c.width, 32);
  ctx.fillStyle = "#fff";
  ctx.font = "14px sans-serif";
  ctx.textAlign = "center";
  ctx.fillText(status.value, c.width / 2, 21);
  ctx.textAlign = "left";
}
</script>

<template>
  <canvas
    ref="canvas"
    style="position: fixed; inset: 0; cursor: crosshair"
    @mousedown="onDown"
    @mousemove="onMove"
    @mouseup="onUp"
  />
</template>

<style scoped>
canvas {
  display: block;
}
</style>
