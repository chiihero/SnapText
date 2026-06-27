<script setup lang="ts">
// 选区窗口：全屏显示主屏截图 + Canvas 鼠标框选 + 抬起调 select_region。
//
// 截图在 Rust 端 trigger_capture_cmd 里"先截图再开窗"完成（避免窗口盖住桌面
// 截到白屏自己）。窗口打开后主动调 get_last_capture 拉取已缓存截图渲染。
// 框选完成后调 select_region，结果存 Pinia，然后创建结果窗口（label=result）。
import { onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api, type MonitorDto, type SelectResult } from "../api";
import { useCaptureStore } from "../stores/capture";

const canvas = ref<HTMLCanvasElement | null>(null);
const status = ref("加载截图…");
const primary = ref<MonitorDto | null>(null);
const img = new Image();
const dragStart = ref<{ x: number; y: number } | null>(null);
const dragCur = ref<{ x: number; y: number } | null>(null);
const store = useCaptureStore();

onMounted(async () => {
  // 主动拉取 Rust 端已缓存的截图（trigger_capture_cmd 先截图后开窗）。
  try {
    const monitors = await api.getLastCapture();
    primary.value = monitors.find((m) => m.primary) ?? monitors[0] ?? null;
    if (!primary.value) {
      status.value = "未找到显示器";
      return;
    }
    status.value = "拖动鼠标框选文字区域 · Esc 取消";
    img.onload = () => draw();
    img.src = api.fileSrc(primary.value.shot_path);
  } catch (e) {
    status.value = `加载截图失败：${e}`;
  }

  // Esc 关闭窗口。
  window.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") {
      getCurrentWindow().close();
    }
  });
});

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
  status.value = "识别中…";
  draw();
  try {
    const result: SelectResult = await api.selectRegion(primary.value.id, bbox);
    store.lastResult = result;
    // 打开结果窗口。
    await new WebviewWindow("result", {
      url: "index.html#/result",
      title: "SnapText 译文",
      width: Math.min(result.shot_path ? 800 : 480, 1200),
      height: 600,
      resizable: true,
      center: true,
    });
    // 关闭选区窗口。
    await getCurrentWindow().close();
  } catch (e) {
    status.value = `识别失败：${e}`;
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
    ctx.fillStyle = "rgba(0,0,0,0.4)";
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
