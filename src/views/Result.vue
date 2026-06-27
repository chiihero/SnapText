<script setup lang="ts">
// 结果窗口：选区截图 + 译文按 OCR 行 bbox 原位覆盖 + 工具栏。
// bbox 是裁剪图内坐标，图即裁剪图本身，1:1 直接用。
import { onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import { NButton, NSpace, NTag, useMessage } from "naive-ui";
import { api, type SelectResult } from "../api";
import { useCaptureStore } from "../stores/capture";

const message = useMessage();
const store = useCaptureStore();
const result = ref<SelectResult | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const img = new Image();
const showAllOriginal = ref(false);
const perLineOriginal = ref<boolean[]>([]);
const fontSize = ref(14);

onMounted(async () => {
  result.value = store.lastResult;
  if (!result.value) {
    await getCurrentWindow().close();
    return;
  }
  perLineOriginal.value = result.value.ocr_lines.map(() => false);
  img.onload = () => draw();
  img.src = api.fileSrc(result.value.shot_path);
  const cfg = await api.getConfig();
  fontSize.value = Math.max(10, Math.min(24, cfg.ui.card_font_size || 14));
  if (cfg.ui.auto_copy_translation && result.value.translated) {
    await writeText(result.value.translated).catch(() => {});
  }
});

watch([showAllOriginal, perLineOriginal, result], () => draw(), { deep: true });

function draw() {
  const c = canvas.value;
  const r = result.value;
  if (!c || !r || !img.naturalWidth) return;
  const ctx = c.getContext("2d")!;
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  ctx.drawImage(img, 0, 0);
  ctx.font = `${fontSize.value}px "Microsoft YaHei UI", "Microsoft YaHei", sans-serif`;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  r.ocr_lines.forEach((line, i) => {
    const b = line.bbox;
    const showOrig = showAllOriginal.value || perLineOriginal.value[i];
    const text = showOrig ? line.text : r!.translations[i] ?? "";
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(b.x, b.y, b.w, b.h);
    ctx.fillStyle = "#1d2129";
    ctx.fillText(text, b.x + 2, b.y + b.h / 2, b.w - 4);
  });
}

function onClick(ev: MouseEvent) {
  const c = canvas.value!;
  const r = result.value!;
  const rect = c.getBoundingClientRect();
  const sx = c.width / rect.width;
  const sy = c.height / rect.height;
  const x = (ev.clientX - rect.left) * sx;
  const y = (ev.clientY - rect.top) * sy;
  r.ocr_lines.forEach((line, i) => {
    const b = line.bbox;
    if (x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) {
      perLineOriginal.value[i] = !perLineOriginal.value[i];
    }
  });
}

async function copyText(t: string, label: string) {
  await writeText(t).catch(() => {});
  message.success(`${label}已复制`);
}

async function saveImage() {
  const r = result.value;
  if (!r) return;
  const dst = await save({
    defaultPath: `snaptext-${Date.now()}.png`,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!dst) return;
  try {
    await api.saveImageCopy(r.shot_path, dst);
    message.success("已保存");
  } catch (e) {
    message.error(`保存失败：${e}`);
  }
}

function toggleAll() {
  showAllOriginal.value = !showAllOriginal.value;
}
function close() {
  getCurrentWindow().close();
}
</script>

<template>
  <div style="display: flex; flex-direction: column; height: 100vh">
    <!-- 工具栏 -->
    <div
      style="
        display: flex;
        gap: 8px;
        padding: 8px 12px;
        border-bottom: 1px solid var(--st-border);
        background: var(--st-surface);
        align-items: center;
      "
    >
      <n-button size="small" @click="toggleAll">
        {{ showAllOriginal ? "显示译文" : "显示原文" }}
      </n-button>
      <n-button size="small" @click="copyText(result?.translated ?? '', '译文')">复制译文</n-button>
      <n-button size="small" @click="copyText(result?.original ?? '', '原文')">复制原文</n-button>
      <n-button size="small" @click="saveImage">保存图片</n-button>
      <div style="margin-left: auto; display: flex; align-items: center; gap: 8px">
        <n-tag v-if="result" size="small" type="info">{{ result.provider }}</n-tag>
        <n-tag v-if="result" size="small">{{ result.elapsed_ms }} ms</n-tag>
        <n-button size="small" quaternary @click="close">关闭</n-button>
      </div>
    </div>
    <!-- 译文叠加画布 -->
    <div style="flex: 1; overflow: auto; background: var(--st-bg); padding: 12px; text-align: center">
      <canvas
        v-if="result"
        ref="canvas"
        @click="onClick"
        style="max-width: 100%; max-height: calc(100vh - 96px); box-shadow: var(--st-shadow); border-radius: 4px; cursor: pointer"
      />
    </div>
  </div>
</template>
