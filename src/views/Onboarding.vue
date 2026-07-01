<script setup lang="ts">
// 首启引导：欢迎+热键 → 下载 OCR 模型 → 翻译配置（可选）→ 完成。
// 持久化靠 config.general.onboarding_completed（单标志位）：
//   仅"完成"时 complete_onboarding 置 true，中途关闭/崩溃仍为 false → 下次重进。
// 模型下载强制：未就绪不能进入下一步（SnapText 核心是 OCR，没模型等于半残）。
//   下载失败可重试，但不允许跳过。
// tier 选择在下载前即时 save_config 落盘（下载/reload 都读 config.tier），
// 其余配置（热键/翻译Key）末尾统一保存一次（避免分步多次触发后端重注册热键）。
import { onMounted, onBeforeUnmount, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NSteps,
  NStep,
  NCard,
  NSpace,
  NButton,
  NInput,
  NForm,
  NFormItem,
  NRadioGroup,
  NRadio,
  NSelect,
  NProgress,
  NAlert,
  NTag,
  useMessage,
} from "naive-ui";
import { api, type Config, type Tier, type Lang, type ProviderKind } from "../api";
import { useConfigStore } from "../stores/config";

const router = useRouter();
const message = useMessage();
const store = useConfigStore();

const draft = reactive<Config>({} as Config);
const loaded = ref(false); // config 灌入 draft 前不渲染表单（避免 draft.hotkey 等为 undefined 崩）
const current = ref(1); // 步骤序号 1/2/3（n-steps current 从 1 起）

// 模型下载状态
const modelsReady = ref(false);
const downloading = ref(false);
// 0~100，按 role(det/rec/dict) 分 3 段折算：det 0~33、rec 33~80、dict 80~100。
const progress = ref(0);
const downloadError = ref<string>("");
// 进度条不确定态：后端 total 为 null（无 Content-Length）时显示脉冲动画。
const progressActive = ref(false);

const langOptions = [
  { label: "中文", value: "zh" as Lang },
  { label: "英文", value: "en" as Lang },
  { label: "日文", value: "ja" as Lang },
];

const tierOptions = [
  { label: "medium（精度优先，约 30MB）", value: "medium" as Tier },
  { label: "small（速度优先，约 15MB）", value: "small" as Tier },
];

// tier 切换：下载/reload 都读 config.tier，必须下载前即时落盘。
// 下载中途禁止切换（避免半途换档位）。
async function onTierChange() {
  if (downloading.value) return;
  try {
    await store.save(JSON.parse(JSON.stringify(draft)));
    await checkModels(); // 换档位后重新检查该档位是否已就绪
  } catch (e) {
    message.error(`保存档位失败：${e}`);
  }
}

// 事件监听句柄（页面卸载时必须清理，避免回调引用已销毁的响应式变量）
let unProgress: UnlistenFn | null = null;
let unDone: UnlistenFn | null = null;

onMounted(async () => {
  if (!store.config) await store.load();
  Object.assign(draft, JSON.parse(JSON.stringify(store.config)));
  loaded.value = true;
  await checkModels();
});

onBeforeUnmount(() => {
  unProgress?.();
  unDone?.();
});

async function checkModels() {
  modelsReady.value = await api.modelsReady(draft.ocr.tier).catch(() => false);
  if (modelsReady.value) {
    progress.value = 100;
  }
}

// 进度折算：role 三段权重 det:33 / rec:47 / dict:20。
function roleBase(role: string): number {
  if (role === "det") return 0;
  if (role === "rec") return 33;
  return 80;
}
function roleWeight(role: string): number {
  if (role === "det") return 33;
  if (role === "rec") return 47;
  return 20;
}

async function startDownload() {
  downloadError.value = "";
  downloading.value = true;
  progress.value = 0;
  progressActive.value = false;

  // 先注册监听再触发下载，避免首条事件丢失。
  unProgress = await listen<{ role: string; received: number; total: number | null }>(
    "download-progress",
    (e) => {
      const { role, received, total } = e.payload;
      const weight = roleWeight(role);
      const base = roleBase(role);
      if (total && total > 0) {
        const within = (received / total) * weight;
        progress.value = Math.min(100, Math.round(base + within));
        progressActive.value = false;
      } else {
        // 无 Content-Length：不确定态，至少显示有进度在动。
        progressActive.value = true;
      }
    },
  );
  unDone = await listen<{ ok: boolean; error: string }>("download-done", async (e) => {
    downloading.value = false;
    if (e.payload.ok) {
      progress.value = 100;
      progressActive.value = false;
      await checkModels();
      // 下载成功后重建 OCR Provider（启动时模型缺失降级为 None，此处即时生效）。
      try {
        await api.reloadOcrProvider();
      } catch (e) {
        console.error("重建 OCR Provider 失败", e);
      }
      message.success("模型下载完成");
    } else {
      downloadError.value = e.payload.error || "下载失败";
      message.error(`模型下载失败：${e.payload.error}`);
    }
  });

  try {
    await api.downloadModels(draft.ocr.tier);
  } catch (e) {
    downloading.value = false;
    downloadError.value = String(e);
    message.error(`触发下载失败：${e}`);
  }
}

// 完成或跳过：保存草稿 + 置 onboarding_completed → 回首页。
async function finish() {
  try {
    await store.save(JSON.parse(JSON.stringify(draft)));
    await api.completeOnboarding();
    router.replace("/home");
  } catch (e) {
    message.error(`完成引导失败：${e}`);
  }
}

function next() {
  if (current.value < 3) current.value++;
}
function prev() {
  if (current.value > 1) current.value--;
}
</script>

<template>
  <div style="padding: 24px; height: 100vh; display: flex; flex-direction: column">
    <n-space v-if="loaded" vertical :size="16" style="flex: 1">
      <h2 style="margin: 0">欢迎使用 SnapText</h2>
      <p style="margin: -8px 0 0; color: var(--st-text-weak); font-size: 13px">
        几步简单配置即可开始截图 OCR 与翻译。
      </p>

      <n-steps :current="current" size="small">
        <n-step title="快捷键" />
        <n-step title="下载 OCR 模型" />
        <n-step title="翻译配置（可选）" />
      </n-steps>

      <!-- 步骤 1：快捷键 -->
      <n-card v-if="current === 1" title="设置截图快捷键">
        <n-form label-placement="left" :label-width="120">
          <n-form-item label="触发截图">
            <n-input v-model:value="draft.hotkey.trigger" placeholder="Ctrl+Alt+Q" />
          </n-form-item>
        </n-form>
        <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
          默认 Ctrl+Alt+Q，格式如 Ctrl+Alt+Q；保存后即时生效。选区中按 Esc 取消。
        </p>
        <template #footer>
          <n-space justify="end">
            <n-button type="primary" @click="next">下一步</n-button>
          </n-space>
        </template>
      </n-card>

      <!-- 步骤 2：下载模型 -->
      <n-card v-else-if="current === 2" title="下载 OCR 模型">
        <n-space vertical :size="12">
          <n-form-item label="模型档位" :label-width="80">
            <n-select
              v-model:value="draft.ocr.tier"
              :options="tierOptions"
              :disabled="downloading"
              @update:value="onTierChange"
            />
          </n-form-item>

          <n-space align="center">
            <n-tag :type="modelsReady ? 'success' : 'warning'" size="small" round>
              模型：{{ modelsReady ? "就绪" : "未下载" }}
            </n-tag>
          </n-space>

          <div v-if="modelsReady">
            <p style="margin: 0; color: var(--st-text-weak); font-size: 13px">
              ✓ 模型已就绪，无需下载。
            </p>
          </div>
          <div v-else>
            <n-space vertical :size="8">
              <n-button type="primary" :loading="downloading" :disabled="downloading" @click="startDownload">
                {{ downloading ? "下载中…" : "开始下载" }}
              </n-button>
              <n-progress
                v-if="downloading || progress > 0"
                type="line"
                :percentage="progress"
                :indicator-placement="'inside'"
                :status="downloadError ? 'error' : 'success'"
                :processing="progressActive"
              />
              <n-alert v-if="downloadError" type="error" :show-icon="true">
                {{ downloadError }}（请重试）
              </n-alert>
            </n-space>
          </div>

          <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
            模型用于本地 OCR 识别（PP-OCRv6），来自 ModelScope，须完成下载才能使用截图识别。
          </p>
        </n-space>
        <template #footer>
          <n-space justify="space-between">
            <n-button @click="prev">上一步</n-button>
            <n-button type="primary" :disabled="!modelsReady" @click="next">下一步</n-button>
          </n-space>
        </template>
      </n-card>

      <!-- 步骤 3：翻译配置（可选） -->
      <n-card v-else title="配置翻译引擎（可选）">
        <n-form label-placement="left" :label-width="120">
          <n-form-item label="引擎">
            <n-radio-group v-model:value="draft.translate.provider">
              <n-radio value="deepseek" label="DeepSeek" />
              <n-radio value="deepl" label="DeepL" />
              <n-radio value="microsoft" label="Microsoft" />
            </n-radio-group>
          </n-form-item>
          <n-form-item label="目标语言">
            <n-select v-model:value="draft.translate.target_lang" :options="langOptions" />
          </n-form-item>

          <!-- DeepSeek -->
          <template v-if="draft.translate.provider === 'deepseek'">
            <n-form-item label="API Key">
              <n-input
                v-model:value="draft.translate.deepseek.api_key"
                type="password"
                show-password-on="click"
                placeholder="sk-..."
              />
            </n-form-item>
            <p style="color: var(--st-text-weak); font-size: 12px; margin: 0 0 0 120px">
              模型可在设置页填 Key 后动态拉取，此处先跳过。
            </p>
          </template>

          <!-- DeepL -->
          <template v-else-if="draft.translate.provider === 'deepl'">
            <n-form-item label="API Key">
              <n-input
                v-model:value="draft.translate.deepl.api_key"
                type="password"
                show-password-on="click"
              />
            </n-form-item>
          </template>

          <!-- Microsoft -->
          <template v-else>
            <n-form-item label="API Key">
              <n-input
                v-model:value="draft.translate.microsoft.api_key"
                type="password"
                show-password-on="click"
              />
            </n-form-item>
          </template>
        </n-form>
        <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
          翻译为可选项，可稍后在设置中配置。不配置时仍可使用 OCR 识别。
        </p>
        <template #footer>
          <n-space justify="space-between">
            <n-button @click="prev">上一步</n-button>
            <n-button type="primary" @click="finish">完成</n-button>
          </n-space>
        </template>
      </n-card>
    </n-space>
  </div>
</template>
