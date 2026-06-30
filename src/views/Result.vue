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

onMounted(async () => {
  t0Base = performance.now();
  // 1. 拉裁剪图渲染原图（crop_region 已写盘，OCR 之前就能显示）。
  try {
    const crop = await api.getLastCrop();
    shotPath.value = crop.shot_path;
    img.onload = () => {
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

function draw() {
  const c = canvas.value;
  if (!c || !img.naturalWidth) return;
  const ctx = c.getContext("2d")!;
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  ctx.drawImage(img, 0, 0);
  // 原图态或尚无 OCR 结果：只画截图，不叠加任何文字。
  if (view.value === "image" || ocrLines.value.length === 0) return;
  ctx.font = `${fontSize.value}px "Microsoft YaHei UI", "Microsoft YaHei", sans-serif`;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ocrLines.value.forEach((line, i) => {
    const b = line.bbox;
    // original 态恒显示原文；translated 态按单行点击翻转（点行→该行切原文）。
    const showOrig =
      view.value === "original" ||
      (view.value === "translated" && perLineOriginal.value[i]);
    const text =
      showOrig || translations.value.length === 0
        ? line.text
        : translations.value[i] ?? "";
    // 背景：贴同位置模糊原图区块（弱化背景、不擦除），取代纯白硬擦。
    if (blurredCanvas.width > 0) {
      ctx.drawImage(
        blurredCanvas,
        b.x, b.y, b.w, b.h, // 源：模糊图同 bbox 区块
        b.x, b.y, b.w, b.h, // 目标：主 canvas 原位置
      );
    }
    // 文字：深色 + 半透明白描边，保证在任意模糊背景上都清晰（微信同款做法）。
    ctx.lineJoin = "round";
    ctx.lineWidth = Math.max(2, fontSize.value / 5);
    ctx.strokeStyle = "rgba(255,255,255,0.85)";
    ctx.strokeText(text, b.x + 2, b.y + b.h / 2, b.w - 4);
    ctx.fillStyle = "#1d2129";
    ctx.fillText(text, b.x + 2, b.y + b.h / 2, b.w - 4);
  });
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

function onClick(ev: MouseEvent) {
  // 仅 translated 视图支持单行点击切原文（与全局态 XOR）。
  if (view.value !== "translated" || translations.value.length === 0) return;
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

function close() {
  getCurrentWindow().close();
}
</script>

<template>
  <div style="display: flex; flex-direction: column; height: 100vh">
    <!-- 顶部状态条：识别/翻译进度或手动提示 -->
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
      <canvas
        v-if="shotPath"
        ref="canvas"
        @click="onClick"
        style="max-width: 100%; max-height: calc(100vh - 96px); box-shadow: var(--st-shadow); border-radius: 4px; cursor: pointer"
      />
    </div>
  </div>
</template>
