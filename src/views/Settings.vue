<script setup lang="ts">
// 设置面板：8 分类（通用/快捷键/截图/OCR/翻译/界面/历史/关于）。
// 草稿机制：深拷贝 config 编辑，保存时整体写回 + 重建翻译 Provider。
import { onMounted, onBeforeUnmount, reactive, ref, watch } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NForm,
  NFormItem,
  NInput,
  NSwitch,
  NSlider,
  NRadioGroup,
  NRadio,
  NSelect,
  NButton,
  NSpace,
  NCard,
  NAlert,
  NTag,
  NProgress,
  useMessage,
  type MenuOption,
} from "naive-ui";
import { api, type Config, type Lang, type ProviderKind, type Tier } from "../api";
import { useConfigStore } from "../stores/config";

const message = useMessage();
const store = useConfigStore();
const draft = reactive<Config>({} as Config);
const activeTab = ref("translate");

// 系统默认 prompt 模板——onMounted 时从后端 get_default_prompt 命令拉取
// （单一数据源，前端零硬编码，避免与后端 DEFAULT_PROMPT_TEMPLATE 不同步）。
const defaultPrompt = ref("");

// 应用版本号——Tauri 内置 getVersion() 读 tauri.conf.json 的 version 字段。
const appVersion = ref("");

// 占位符说明——字面双花括号，放 script 里避免被 Vue 模板当插值解析。
const PROMPT_PLACEHOLDER_HINT =
  "可用占位符：{{source}} 源语言 · {{target}} 目标语言 · {{input}} 原文（缺失时自动追加兜底）。仅对 LLM 引擎生效。";

const tabs: { key: string; label: string }[] = [
  { key: "general", label: "通用" },
  { key: "hotkey", label: "快捷键" },
  { key: "capture", label: "截图" },
  { key: "ocr", label: "文字识别" },
  { key: "translate", label: "翻译" },
  { key: "ui", label: "界面显示" },
  { key: "history", label: "历史记录" },
  { key: "about", label: "关于" },
];
const menuOptions: MenuOption[] = tabs.map((t) => ({
  key: t.key,
  label: t.label,
}));

const langOptions = [
  { label: "中文", value: "zh" as Lang },
  { label: "英文", value: "en" as Lang },
  { label: "日文", value: "ja" as Lang },
];
const tierOptions = [
  { label: "medium（精度优先）", value: "medium" as Tier },
  { label: "small（速度优先）", value: "small" as Tier },
];

// DeepSeek 模型下拉：填 Key 后点"刷新"动态拉取（GET /v1/models），可手输兜底。
const deepseekModelOptions = ref<{ label: string; value: string }[]>([]);
const loadingModels = ref(false);
async function refreshDeepseekModels() {
  const dc = draft.translate.deepseek;
  const key = dc.api_key?.trim();
  if (!key) {
    message.warning("请先填写 API Key");
    return;
  }
  loadingModels.value = true;
  try {
    const ids = await api.listDeepseekModels(dc.base_url, key);
    deepseekModelOptions.value = ids.map((id) => ({ label: id, value: id }));
    message.success(`已拉取 ${ids.length} 个模型`);
  } catch (e) {
    message.error(`拉取模型失败：${e}`);
  } finally {
    loadingModels.value = false;
  }
}

// prompt 模式切换：切到"自定义"时若 prompt_template 为空则预填系统默认值作为编辑起点；
// 切到"系统默认"时不清空 prompt_template（保留用户上次自定义，切回来不丢失）。
function onPromptModeChange(useCustom: boolean) {
  if (useCustom && !draft.translate.prompt_template && defaultPrompt.value) {
    draft.translate.prompt_template = defaultPrompt.value;
  }
}

onMounted(async () => {
  if (!store.config) await store.load();
  Object.assign(draft, JSON.parse(JSON.stringify(store.config)));
  // 拉取系统默认 prompt（只读展示 + 切换预填用）。
  try {
    defaultPrompt.value = await api.getDefaultPrompt();
  } catch (e) {
    console.error("拉取默认 prompt 失败", e);
  }
  // 拉取应用版本号（关于页展示用）。
  try {
    appVersion.value = await getVersion();
  } catch (e) {
    console.error("获取版本号失败", e);
  }
  // 检查 OCR 模型就绪态（OCR 分类展示状态 tag + 兜底下载入口）。
  await checkModels();
});

onBeforeUnmount(() => {
  unProgress?.();
  unDone?.();
});

async function save() {
  try {
    const ready = await store.save(JSON.parse(JSON.stringify(draft)));
    message.success(ready ? "配置已保存" : "配置已保存（翻译未就绪，请检查 API Key）");
  } catch (e) {
    message.error(`保存失败：${e}`);
  }
}

// ===== OCR 模型下载兜底入口（设置页）=====
// 引导页外补一个恢复通道：模型被删/损坏/首次跳过 onboarding_completed 仍为 true 时，
// 用户无处下载（引导页进不去），此处兜底。逻辑同 Onboarding.vue（接受重复换体验）。
const modelsReady = ref(false);
const downloading = ref(false);
// 0~100，按 role(det/rec/dict) 分 3 段折算：det 0~33、rec 33~80、dict 80~100。
const progress = ref(0);
const downloadError = ref<string>("");
// 后端 total 为 null（无 Content-Length）时显示脉冲动画。
const progressActive = ref(false);
let unProgress: UnlistenFn | null = null;
let unDone: UnlistenFn | null = null;

async function checkModels() {
  modelsReady.value = await api.modelsReady(draft.ocr.tier).catch(() => false);
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

// 下载前先落盘草稿：download_models 用入参 tier，但 reload_ocr_provider 读 state.config.ocr.tier，
// 若用户改了档位没保存会导致两者错配（下载新档位、reload 用旧档位）。故先 save 再下载。
async function downloadModels() {
  downloadError.value = "";
  downloading.value = true;
  progress.value = 0;
  progressActive.value = false;

  try {
    await store.save(JSON.parse(JSON.stringify(draft)));
  } catch (e) {
    downloading.value = false;
    message.error(`保存档位失败：${e}`);
    return;
  }

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
      // 重建 OCR Provider：启动时模型缺失降级为 None，此处即时生效无需重启。
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
</script>

<template>
  <n-layout has-sider style="height: 100vh">
    <n-layout-sider bordered :width="160" content-style="padding: 12px 0">
      <n-menu v-model:value="activeTab" :options="menuOptions" />
    </n-layout-sider>
    <n-layout-content content-style="padding: 20px 24px; overflow: auto">
      <n-space vertical :size="16">
        <!-- 通用 -->
        <template v-if="activeTab === 'general'">
          <n-card title="通用">
            <n-form label-placement="left" :label-width="160">
              <n-form-item label="框选后自动识别">
                <n-switch v-model:value="draft.general.auto_ocr" />
              </n-form-item>
              <n-form-item label="识别后自动翻译">
                <n-switch v-model:value="draft.general.auto_translate" />
              </n-form-item>
              <n-form-item label="关闭时最小化到托盘">
                <n-switch v-model:value="draft.ui.minimize_to_tray_on_close" />
              </n-form-item>
            </n-form>
            <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
              关闭自动识别/翻译后，可在结果窗手动点按钮触发。
            </p>
          </n-card>
        </template>

        <!-- 快捷键 -->
        <template v-if="activeTab === 'hotkey'">
          <n-card title="快捷键">
            <n-form label-placement="left" :label-width="120">
              <n-form-item label="触发截图">
                <n-input v-model:value="draft.hotkey.trigger" placeholder="Ctrl+Alt+Q" />
              </n-form-item>
            </n-form>
            <n-alert
              v-if="store.hotkeyError"
              type="error"
              :show-icon="true"
              style="margin-bottom: 8px"
            >
              {{ store.hotkeyError }}
            </n-alert>
            <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
              格式如 Ctrl+Alt+Q；保存后即时生效。选区中按 Esc 取消（固定）。档位切换需重启。
            </p>
          </n-card>
        </template>

        <!-- 截图 -->
        <template v-if="activeTab === 'capture'">
          <n-card title="截图">
            <n-form label-placement="left" :label-width="160">
              <n-form-item label="选区蒙版不透明度">
                <n-slider v-model:value="draft.ui.overlay_dim_alpha" :min="0" :max="1" :step="0.05" />
              </n-form-item>
            </n-form>
          </n-card>
        </template>

        <!-- OCR -->
        <template v-if="activeTab === 'ocr'">
          <n-card title="文字识别">
            <n-form label-placement="left" :label-width="120">
              <n-form-item label="档位">
                <n-select
                  v-model:value="draft.ocr.tier"
                  :options="tierOptions"
                  :disabled="downloading"
                />
              </n-form-item>
              <n-form-item label="结果后处理">
                <n-switch v-model:value="draft.ocr.postprocess" />
                <span style="color: var(--st-text-weak); font-size: 12px; margin-left: 8px">
                  去空格 / 合并换行
                </span>
              </n-form-item>

              <n-form-item label="模型">
                <div style="display: flex; flex-direction: column; gap: 8px; width: 100%">
                  <n-tag :type="modelsReady ? 'success' : 'warning'" size="small" round>
                    模型：{{ modelsReady ? "就绪" : "未下载" }}
                  </n-tag>
                  <div v-if="!modelsReady">
                    <n-space vertical :size="8">
                      <n-button
                        type="primary"
                        :loading="downloading"
                        :disabled="downloading"
                        @click="downloadModels"
                      >
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
                  <span v-else style="color: var(--st-text-weak); font-size: 13px">
                    ✓ 模型已就绪。
                  </span>
                </div>
              </n-form-item>
            </n-form>
            <p style="color: var(--st-text-weak); font-size: 12px">
              模型来自 ModelScope（PP-OCRv6），须下载后才能使用截图识别。档位切换需重启生效。
            </p>
          </n-card>
        </template>

        <!-- 翻译 -->
        <template v-if="activeTab === 'translate'">
          <n-card title="翻译引擎">
            <n-form label-placement="left" :label-width="100">
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

              <template v-if="draft.translate.provider === 'deepseek'">
                <n-form-item label="Base URL">
                  <n-input v-model:value="draft.translate.deepseek.base_url" />
                </n-form-item>
                <n-form-item label="API Key">
                  <n-input
                    v-model:value="draft.translate.deepseek.api_key"
                    type="password"
                    show-password-on="click"
                    placeholder="sk-..."
                  />
                </n-form-item>
                <n-form-item label="模型">
                  <div style="display: flex; gap: 8px; width: 100%">
                    <n-select
                      v-model:value="draft.translate.deepseek.model"
                      :options="deepseekModelOptions"
                      filterable
                      tag
                      placeholder="填 Key 后点刷新拉取，或手动输入模型名"
                      style="flex: 1"
                    />
                    <n-button size="small" :loading="loadingModels" @click="refreshDeepseekModels">刷新</n-button>
                  </div>
                </n-form-item>
                <n-form-item label="思考模式">
                  <div style="display: flex; align-items: center; gap: 16px">
                    <n-switch v-model:value="draft.translate.deepseek.reasoning_enabled" />
                    <n-radio-group
                      v-model:value="draft.translate.deepseek.reasoning_effort"
                      :disabled="!draft.translate.deepseek.reasoning_enabled"
                    >
                      <n-radio value="high" label="high（默认）" />
                      <n-radio value="max" label="max（最强）" />
                    </n-radio-group>
                  </div>
                </n-form-item>
              </template>
              <template v-else-if="draft.translate.provider === 'deepl'">
                <n-form-item label="套餐">
                  <n-radio-group v-model:value="draft.translate.deepl.plan">
                    <n-radio value="Free" label="Free" />
                    <n-radio value="Pro" label="Pro" />
                  </n-radio-group>
                </n-form-item>
                <n-form-item label="API Key">
                  <n-input v-model:value="draft.translate.deepl.api_key" type="password" show-password-on="click" />
                </n-form-item>
              </template>
              <template v-else-if="draft.translate.provider === 'microsoft'">
                <n-form-item label="区域">
                  <n-input v-model:value="draft.translate.microsoft.region" placeholder="southeastasia" />
                </n-form-item>
                <n-form-item label="API Key">
                  <n-input
                    v-model:value="draft.translate.microsoft.api_key"
                    type="password"
                    show-password-on="click"
                  />
                </n-form-item>
              </template>

              <n-form-item label="翻译提示词">
                <div style="width: 100%">
                  <n-radio-group
                    :value="draft.translate.prompt_use_custom"
                    @update:value="(v: boolean) => { draft.translate.prompt_use_custom = v; onPromptModeChange(v); }"
                    style="margin-bottom: 8px"
                  >
                    <n-radio :value="false" label="系统默认（只读）" />
                    <n-radio :value="true" label="自定义" />
                  </n-radio-group>
                  <n-input
                    :value="draft.translate.prompt_use_custom
                      ? draft.translate.prompt_template
                      : defaultPrompt"
                    :disabled="!draft.translate.prompt_use_custom"
                    type="textarea"
                    :rows="8"
                    :placeholder="defaultPrompt"
                    @update:value="(v: string) => { if (draft.translate.prompt_use_custom) draft.translate.prompt_template = v; }"
                  />
                  <div
                    style="
                      display: flex;
                      justify-content: space-between;
                      align-items: center;
                      margin-top: 6px;
                    "
                  >
                    <span style="color: var(--st-text-weak); font-size: 12px">
                      {{ PROMPT_PLACEHOLDER_HINT }}
                    </span>
                    <n-button
                      v-if="draft.translate.prompt_use_custom"
                      size="small"
                      @click="draft.translate.prompt_template = defaultPrompt"
                    >
                      重置为默认值
                    </n-button>
                  </div>
                </div>
              </n-form-item>

              <n-form-item label="译文后处理">
                <n-switch v-model:value="draft.translate.postprocess" />
                <span style="color: var(--st-text-weak); font-size: 12px; margin-left: 8px">
                  去引号 / 去前缀 / trim
                </span>
              </n-form-item>
            </n-form>
          </n-card>
        </template>

        <!-- 界面 -->
        <template v-if="activeTab === 'ui'">
          <n-card title="界面显示">
            <n-form label-placement="left" :label-width="160">
              <n-form-item label="自动复制译文">
                <n-switch v-model:value="draft.ui.auto_copy_translation" />
              </n-form-item>
              <n-form-item label="图上翻译字号">
                <n-slider v-model:value="draft.ui.card_font_size" :min="10" :max="24" :step="1" />
              </n-form-item>
            </n-form>
          </n-card>
        </template>

        <!-- 历史 -->
        <template v-if="activeTab === 'history'">
          <n-card title="历史记录">
            <n-form label-placement="left" :label-width="120">
              <n-form-item label="保留天数">
                <n-slider v-model:value="draft.history.retention_days" :min="1" :max="365" />
              </n-form-item>
              <n-form-item label="最大记录数">
                <n-slider v-model:value="draft.history.max_records" :min="100" :max="20000" :step="100" />
              </n-form-item>
              <n-form-item label="启动时自动清理">
                <n-switch v-model:value="draft.history.auto_clean_on_start" />
              </n-form-item>
            </n-form>
          </n-card>
        </template>

        <!-- 关于 -->
        <template v-if="activeTab === 'about'">
          <n-card title="关于">
            <p><strong>SnapText</strong> <span style="color: var(--st-text-weak)">v{{ appVersion }}</span></p>
            <p style="color: var(--st-text-weak)">Windows 截图 OCR + 翻译工具</p>
            <p style="color: var(--st-text-weak); font-size: 12px">
              Tauri 2 + Vue 3 · snaptext-core
            </p>
          </n-card>
        </template>
      </n-space>

      <!-- 底部保存 -->
      <div style="margin-top: 24px; display: flex; gap: 8px; justify-content: flex-end">
        <n-button @click="Object.assign(draft, JSON.parse(JSON.stringify(store.config)))">
          重置
        </n-button>
        <n-button type="primary" @click="save">保存</n-button>
      </div>
    </n-layout-content>
  </n-layout>
</template>
