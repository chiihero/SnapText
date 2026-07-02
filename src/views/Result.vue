<script setup lang="ts">
// 结果窗口：原图→正在识别→原位显示原文→正在翻译→原位替换译文（两阶段渲染）。
//
// 三层命令分阶段：crop 已由选区窗完成。这里 onMounted 先拉裁剪图渲染原图，
// 再按配置 general.auto_ocr / auto_translate 决定是否自动跑 recognize_region /
// translate_region。关闭自动时停在对应视图，由工具栏三态按钮（原图/原文/译文）
// 手动触发：点"原文"触发 OCR，点"译文"触发翻译（按钮即动作）。
// OCR 行 bbox 是裁剪图内坐标，图即裁剪图本身，1:1 直接用。
import { computed, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import {
  NButton,
  NButtonGroup,
  NPopover,
  NTag,
  useMessage,
} from "naive-ui";
import { api, type OcrLine } from "../api";

type View = "image" | "original" | "translated";

const message = useMessage();
// idle：未自动 OCR 或等待手动触发；recognizing/translating：处理中；done：翻译完成。
const phase = ref<"idle" | "recognizing" | "translating" | "done">("idle");
const busy = computed(
  () => phase.value === "recognizing" || phase.value === "translating",
);
const statusText = computed(() => {
  if (phase.value === "recognizing") return "正在识别…";
  if (phase.value === "translating") return "正在翻译…";
  if (view.value === "image" && !ocrDone.value) return "点「原文」开始识别";
  if (view.value === "image" && ocrDone.value) return "已识别，点「原文」查看";
  if (view.value === "original" && !translateDone.value) return "点「译文」开始翻译";
  return "";
});

// 三态视图：image=纯原图；original=图上原位显示 OCR 原文；translated=图上原位显示译文。
const view = ref<View>("image");
const ocrDone = ref(false);
const translateDone = ref(false);

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
// onMounted 起始时刻基准，runTranslate 算总耗时用（跨函数，故提到模块作用域）。
let t0Base = 0;
// 自动复制译文开关：onMounted 读一次，runTranslate 用。
let autoCopyTranslation = false;

const canvas = ref<HTMLCanvasElement | null>(null);
const img = new Image();
// 离屏模糊原图缓存：img.onload 后渲染一次，draw() 每行贴对应 bbox 区块做"弱化背景"。
// 微信截图翻译同款思路——文字区域用同位置模糊原图做底，自然融合不突兀（非纯白硬擦）。
const blurredCanvas = document.createElement("canvas");
const perLineOriginal = ref<boolean[]>([]);
const fontSize = ref(14);
// 原图像素尺寸：img.onload 记录，DOM 文字层按 bbox 百分比定位时做分母。
const imgW = ref(0);
const imgH = ref(0);

onMounted(async () => {
  t0Base = performance.now();
  // 1. 拉裁剪图渲染原图（crop_region 已写盘，OCR 之前就能显示）。
  try {
    const crop = await api.getLastCrop();
    shotPath.value = crop.shot_path;
    img.onload = () => {
      imgW.value = img.naturalWidth;
      imgH.value = img.naturalHeight;
      renderBlurred();
      draw();
    };
    img.src = api.fileSrc(crop.shot_path);
  } catch (e) {
    message.error(`加载结果失败：${e}`);
    await getCurrentWindow().close();
    return;
  }

  // 读配置：字号 + 自动复制（runTranslate 用）+ 自动 OCR/翻译开关。
  const cfg = await api.getConfig();
  fontSize.value = Math.max(10, Math.min(24, cfg.ui.card_font_size || 14));
  autoCopyTranslation = cfg.ui.auto_copy_translation;

  // 2. 自动 OCR？关闭则停在原图态等手动。
  if (!cfg.general.auto_ocr) {
    phase.value = "idle";
    view.value = "image";
    return;
  }
  if (!(await runOcr())) return;

  // 3. 自动翻译？关闭则停在原文态等手动。
  if (!cfg.general.auto_translate) {
    phase.value = "done";
    view.value = "original";
    draw();
    return;
  }
  await runTranslate();
});

watch([view, perLineOriginal, ocrLines, translations, fontSize], () => draw(), { deep: true });

// 渲染离屏模糊原图（只在 img 加载后做一次）。draw() 每行按 bbox 从这里取对应区块贴回主 canvas。
function renderBlurred() {
  if (!img.naturalWidth) return;
  blurredCanvas.width = img.naturalWidth;
  blurredCanvas.height = img.naturalHeight;
  const ctx = blurredCanvas.getContext("2d")!;
  // blur 半径按图宽自适应（小图轻模糊，大图略强），3px 是微信级柔化手感。
  const r = Math.max(2, Math.round(img.naturalWidth / 400));
  ctx.filter = `blur(${r}px)`;
  ctx.drawImage(img, 0, 0);
  ctx.filter = "none";
}

// draw 只画 canvas 底图 + 模糊区块；文字交给 DOM 文字层（按 bbox 绝对定位），可选中复制。
function draw() {
  const c = canvas.value;
  if (!c || !img.naturalWidth) return;
  const ctx = c.getContext("2d")!;
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  ctx.drawImage(img, 0, 0);
  // 原图态或尚无 OCR 结果：只画截图，不叠模糊区块。
  if (view.value === "image" || ocrLines.value.length === 0) return;
  // 每行按 bbox 贴同位置模糊原图区块（弱化背景、不擦除），取代纯白硬擦。
  if (blurredCanvas.width === 0) return;
  ocrLines.value.forEach((line) => {
    const b = line.bbox;
    ctx.drawImage(
      blurredCanvas,
      b.x, b.y, b.w, b.h, // 源：模糊图同 bbox 区块
      b.x, b.y, b.w, b.h, // 目标：主 canvas 原位置
    );
  });
}

// DOM 文字层：按 OCR 行 bbox 百分比定位，使文字随 canvas 缩放同步对齐（与背景图同 relative 容器）。
// 容器为 inline-block，自动包裹 canvas 渲染尺寸；文字层 inset:0 覆盖容器即等于 canvas 可见区。
function lineStyle(line: OcrLine): Record<string, string> {
  const w = imgW.value || 1;
  const h = imgH.value || 1;
  return {
    left: `${(line.bbox.x / w) * 100}%`,
    top: `${(line.bbox.y / h) * 100}%`,
    width: `${(line.bbox.w / w) * 100}%`,
    height: `${(line.bbox.h / h) * 100}%`,
    fontSize: `${fontSize.value}px`,
  };
}

// 第 i 行显示的文字：original 态恒原文；translated 态被翻转行显示原文，否则译文。
function lineText(i: number): string {
  const showOrig =
    view.value === "original" ||
    (view.value === "translated" && perLineOriginal.value[i]);
  return showOrig || translations.value.length === 0
    ? ocrLines.value[i].text
    : translations.value[i] ?? "";
}

// OCR 阶段：调 recognize_region，成功后切到 original 视图。供 onMounted 自动与手动按钮复用。
async function runOcr(): Promise<boolean> {
  phase.value = "recognizing";
  try {
    const tOcrStart = performance.now();
    const ocr = await api.recognizeRegion();
    ocrMs.value = Math.round(performance.now() - tOcrStart);
    ocrLines.value = ocr.ocr_lines;
    original.value = ocr.original;
    perLineOriginal.value = ocr.ocr_lines.map(() => false);
    ocrDone.value = true;
    view.value = "original";
    phase.value = "done";
    draw();
    return true;
  } catch (e) {
    message.error(`识别失败：${e}`);
    phase.value = "idle";
    return false;
  }
}

// 翻译阶段：调 translate_region + 自动复制 + 落库（后端），成功后切到 translated 视图。
async function runTranslate(): Promise<boolean> {
  phase.value = "translating";
  try {
    const tTrStart = performance.now();
    const tr = await api.translateRegion();
    translateMs.value = Math.round(performance.now() - tTrStart);
    translations.value = tr.translations;
    translated.value = tr.translated;
    provider.value = tr.provider;
    totalMs.value = Math.round(performance.now() - t0Base);
    translateDone.value = true;
    phase.value = "done";
    view.value = "translated";
    if (autoCopyTranslation && tr.translated) {
      await writeText(tr.translated).catch(() => {});
    }
    draw();
    return true;
  } catch (e) {
    message.error(`翻译失败：${e}`);
    phase.value = "done";
    return false;
  }
}

// 三态视图切换 + 手动触发：
// - 切到 original 且 OCR 未跑 → 触发 OCR；
// - 切到 translated 且翻译未跑 → 先确保 OCR 再触发翻译；
// - 已跑过或切 image → 纯视图切换。
async function selectView(v: View) {
  if (v === view.value || busy.value) return;
  if (v === "original" && !ocrDone.value) {
    await runOcr();
    return;
  }
  if (v === "translated" && !translateDone.value) {
    if (!ocrDone.value) {
      if (!(await runOcr())) return;
    }
    await runTranslate();
    return;
  }
  view.value = v;
  perLineOriginal.value = ocrLines.value.map(() => false);
}

// 单行点击：仅 translated 态翻转该行原文/译文。
// 拖选文字时 mouseup 也会触发 click——有非空选区则视为选择操作，不翻转。
function onLineClick(i: number) {
  if (view.value !== "translated" || translations.value.length === 0) return;
  const sel = window.getSelection?.()?.toString() ?? "";
  if (sel) return; // 用户在拖选文字，忽略此次点击
  perLineOriginal.value[i] = !perLineOriginal.value[i];
}

// 选中即复制：mouseup 时若文字层内有非空选区，写入剪贴板并提示。Ctrl+C 仍可用（浏览器原生）。
async function onTextMouseUp() {
  const sel = window.getSelection?.()?.toString() ?? "";
  if (!sel) return;
  await writeText(sel).catch(() => {});
  message.success("已复制选中文字");
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

function close() {
  getCurrentWindow().close();
}
</script>

<template>
  <div style="display: flex; flex-direction: column; height: 100vh">
    <!-- 顶部状态条：识别/翻译进度或手动提示（始终占位锁死下方工具栏位置，避免按钮跳动） -->
    <div
      style="
        padding: 6px 12px;
        background: var(--st-surface);
        border-bottom: 1px solid var(--st-border);
        color: #0078d7;
        font-size: 13px;
        line-height: 1.2;
        min-height: 20px;
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
      <n-button-group size="small">
        <n-button
          :type="view === 'image' ? 'primary' : 'default'"
          :disabled="busy"
          @click="selectView('image')"
        >
          原图
        </n-button>
        <n-button
          :type="view === 'original' ? 'primary' : 'default'"
          :disabled="busy"
          @click="selectView('original')"
        >
          原文
        </n-button>
        <n-button
          :type="view === 'translated' ? 'primary' : 'default'"
          :disabled="busy"
          @click="selectView('translated')"
        >
          译文
        </n-button>
      </n-button-group>
      <n-button size="small" :disabled="!translateDone" @click="copyText(translated, '译文')">复制译文</n-button>
      <n-button size="small" :disabled="!ocrDone" @click="copyText(original, '原文')">复制原文</n-button>
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
      <div v-if="shotPath" class="result-stage">
        <canvas ref="canvas" class="result-canvas" />
        <!-- 文字层：盖在 canvas 上，按 OCR 行 bbox 绝对定位，可鼠标选中复制（PDF.js 文字层同款） -->
        <div
          v-if="view !== 'image' && ocrLines.length"
          class="text-layer"
          @mouseup="onTextMouseUp"
        >
          <div
            v-for="(line, i) in ocrLines"
            :key="i"
            class="ocr-line"
            :style="lineStyle(line)"
            @click="onLineClick(i)"
          >{{ lineText(i) }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* relative 容器：canvas 与文字层都按它定位，缩放同步。
   inline-block 自动包裹 canvas 渲染尺寸；line-height:0 消除基线间隙避免容器多出几像素。 */
.result-stage {
  position: relative;
  display: inline-block;
  margin: 0 auto;
  line-height: 0;
}
.result-canvas {
  display: block;
  max-width: 100%;
  max-height: calc(100vh - 96px);
  box-shadow: var(--st-shadow);
  border-radius: 4px;
}
/* 文字层：绝对定位铺满容器，不挡背景（pointer-events:none），命中交给行。 */
.text-layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
/* 每行 OCR 文字：按 bbox 百分比定位（lineStyle），DOM 真文本可拖选复制。
   深色字 + 白描边复刻原 canvas strokeText 视觉（paint-order 保证描边在填充之下）。 */
.ocr-line {
  position: absolute;
  display: flex;
  align-items: center;
  pointer-events: auto;
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
  color: #1d2129;
  -webkit-text-stroke: 2px rgba(255, 255, 255, 0.85);
  paint-order: stroke fill;
  font-family: "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
  line-height: 1;
  white-space: nowrap;
  overflow: hidden;
  /* 与原 draw() 一致：文字左侧贴 bbox 左缘 +2px，垂直居中 */
  padding-left: 2px;
}
</style>
