<script setup lang="ts">
// 结果窗口：原图→正在识别→原位显示原文→正在翻译→原位替换译文（两阶段渲染）。
//
// 三层命令分阶段：crop 已由选区窗完成。这里 onMounted 先拉裁剪图渲染原图，
// 再依次调 recognize_region（OCR，"正在识别"）、translate_region（翻译+落库，
// "正在翻译"）。OCR 行 bbox 是裁剪图内坐标，图即裁剪图本身，1:1 直接用。
import { computed, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import { NButton, NPopover, NTag, useMessage } from "naive-ui";
import { api, type OcrLine } from "../api";

const message = useMessage();
const phase = ref<"recognizing" | "translating" | "done">("recognizing");
const statusText = computed(() =>
  phase.value === "recognizing" ? "正在识别…" : phase.value === "translating" ? "正在翻译…" : ""
);
const shotPath = ref<string>("");
const ocrLines = ref<OcrLine[]>([]);
const translations = ref<string[]>([]);
const original = ref("");
const translated = ref("");
const provider = ref("");
// 阶段耗时（前端打点，单位 ms）。crop 在选区窗已完成，结果窗只感知 OCR/翻译。
const totalMs = ref(0);
const ocrMs = ref(0);
const translateMs = ref(0);

const canvas = ref<HTMLCanvasElement | null>(null);
const img = new Image();
const showAllOriginal = ref(false);
const perLineOriginal = ref<boolean[]>([]);
const fontSize = ref(14);

onMounted(async () => {
  const t0 = performance.now();
  // 1. 拉裁剪图渲染原图（crop_region 已写盘，OCR 之前就能显示）。
  try {
    const crop = await api.getLastCrop();
    shotPath.value = crop.shot_path;
    img.onload = () => draw();
    img.src = api.fileSrc(crop.shot_path);
  } catch (e) {
    message.error(`加载结果失败：${e}`);
    await getCurrentWindow().close();
    return;
  }

  // 2. OCR（正在识别）。
  phase.value = "recognizing";
  try {
    const tOcrStart = performance.now();
    const ocr = await api.recognizeRegion();
    ocrMs.value = Math.round(performance.now() - tOcrStart);
    ocrLines.value = ocr.ocr_lines;
    original.value = ocr.original;
    perLineOriginal.value = ocr.ocr_lines.map(() => false);
    // OCR 完成先在图上原位显示原文（中间态，等翻译再替换为译文）。
    showAllOriginal.value = true;
    draw();
  } catch (e) {
    message.error(`识别失败：${e}`);
    return;
  }

  // 3. 翻译（正在翻译）。
  phase.value = "translating";
  try {
    const tTrStart = performance.now();
    const tr = await api.translateRegion();
    translateMs.value = Math.round(performance.now() - tTrStart);
    translations.value = tr.translations;
    translated.value = tr.translated;
    provider.value = tr.provider;
    totalMs.value = Math.round(performance.now() - t0);
    phase.value = "done";
    // 默认显示译文（用户可点行切原文）。
    showAllOriginal.value = false;

    const cfg = await api.getConfig();
    fontSize.value = Math.max(10, Math.min(24, cfg.ui.card_font_size || 14));
    if (cfg.ui.auto_copy_translation && tr.translated) {
      await writeText(tr.translated).catch(() => {});
    }
    draw();
  } catch (e) {
    message.error(`翻译失败：${e}`);
  }
});

watch([showAllOriginal, perLineOriginal, ocrLines, translations, fontSize], () => draw(), { deep: true });

function draw() {
  const c = canvas.value;
  if (!c || !img.naturalWidth || ocrLines.value.length === 0) return;
  const ctx = c.getContext("2d")!;
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  ctx.drawImage(img, 0, 0);
  ctx.font = `${fontSize.value}px "Microsoft YaHei UI", "Microsoft YaHei", sans-serif`;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ocrLines.value.forEach((line, i) => {
    const b = line.bbox;
    // XOR：单行点击相对全局翻转。全局译文时点行→原文；全局原文时点行→译文。
    const showOrig = showAllOriginal.value !== perLineOriginal.value[i];
    // 翻译未完成时只显示原文；完成后默认译文、可点行切原文。
    const text = showOrig || translations.value.length === 0
      ? line.text
      : translations.value[i] ?? "";
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(b.x, b.y, b.w, b.h);
    ctx.fillStyle = "#1d2129";
    ctx.fillText(text, b.x + 2, b.y + b.h / 2, b.w - 4);
  });
}

function onClick(ev: MouseEvent) {
  const c = canvas.value!;
  const rect = c.getBoundingClientRect();
  const sx = c.width / rect.width;
  const sy = c.height / rect.height;
  const x = (ev.clientX - rect.left) * sx;
  const y = (ev.clientY - rect.top) * sy;
  ocrLines.value.forEach((line, i) => {
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
  if (!shotPath.value) return;
  const dst = await save({
    defaultPath: `snaptext-${Date.now()}.png`,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!dst) return;
  try {
    await api.saveImageCopy(shotPath.value, dst);
    message.success("已保存");
  } catch (e) {
    message.error(`保存失败：${e}`);
  }
}

function toggleAll() {
  showAllOriginal.value = !showAllOriginal.value;
  // 全局翻转后清零单行覆盖，让所有行统一到全局态（否则单行标记会粘住）。
  perLineOriginal.value = ocrLines.value.map(() => false);
}
function close() {
  getCurrentWindow().close();
}
</script>

<template>
  <div style="display: flex; flex-direction: column; height: 100vh">
    <!-- 顶部状态条：识别/翻译进度 -->
    <div
      v-if="statusText"
      style="
        padding: 6px 12px;
        background: var(--st-surface);
        border-bottom: 1px solid var(--st-border);
        color: #0078d7;
        font-size: 13px;
      "
    >
      {{ statusText }}
    </div>
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
      <n-button size="small" :disabled="phase !== 'done'" @click="toggleAll">
        {{ showAllOriginal ? "显示译文" : "显示原文" }}
      </n-button>
      <n-button size="small" :disabled="phase !== 'done'" @click="copyText(translated, '译文')">复制译文</n-button>
      <n-button size="small" @click="copyText(original, '原文')">复制原文</n-button>
      <n-button size="small" @click="saveImage">保存图片</n-button>
      <div style="margin-left: auto; display: flex; align-items: center; gap: 8px">
        <n-tag v-if="provider" size="small" type="info">{{ provider }}</n-tag>
        <n-popover v-if="totalMs" trigger="click" placement="bottom">
          <template #trigger>
            <n-tag size="small" type="info" style="cursor: pointer">
              总耗时 {{ totalMs }} ms
            </n-tag>
          </template>
          <div style="display: flex; flex-direction: column; gap: 4px; font-size: 13px; min-width: 160px">
            <div style="display: flex; justify-content: space-between"><span>识别 OCR</span><b>{{ ocrMs }} ms</b></div>
            <div style="display: flex; justify-content: space-between"><span>翻译</span><b>{{ translateMs }} ms</b></div>
            <div style="display: flex; justify-content: space-between; border-top: 1px solid #eee; padding-top: 4px"><span>合计</span><b>{{ totalMs }} ms</b></div>
          </div>
        </n-popover>
        <n-button size="small" quaternary @click="close">关闭</n-button>
      </div>
    </div>
    <!-- 译文叠加画布 -->
    <div style="flex: 1; overflow: auto; background: var(--st-bg); padding: 12px; text-align: center">
      <canvas
        v-if="shotPath"
        ref="canvas"
        @click="onClick"
        style="max-width: 100%; max-height: calc(100vh - 96px); box-shadow: var(--st-shadow); border-radius: 4px; cursor: pointer"
      />
    </div>
  </div>
</template>
