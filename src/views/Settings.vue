<script setup lang="ts">
// 设置面板：8 分类（通用/快捷键/截图/OCR/翻译/界面/历史/关于）。
// 草稿机制：深拷贝 config 编辑，保存时整体写回 + 重建翻译 Provider。
import { onMounted, reactive, ref, watch } from "vue";
import {
  NLayout,
  NLayoutSider,
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
  useMessage,
  type MenuOption,
} from "naive-ui";
import { api, type Config, type Lang, type ProviderKind, type Tier } from "../api";
import { useConfigStore } from "../stores/config";

const message = useMessage();
const store = useConfigStore();
const draft = reactive<Config>({} as Config);
const activeTab = ref("translate");

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
  { label: "medium（精度优先）", value: "Medium" as Tier },
  { label: "small（速度优先）", value: "Small" as Tier },
];

onMounted(async () => {
  if (!store.config) await store.load();
  Object.assign(draft, JSON.parse(JSON.stringify(store.config)));
});

async function save() {
  try {
    const ready = await store.save(JSON.parse(JSON.stringify(draft)));
    message.success(ready ? "配置已保存" : "配置已保存（翻译未就绪，请检查 API Key）");
  } catch (e) {
    message.error(`保存失败：${e}`);
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
              <n-form-item label="关闭时最小化到托盘">
                <n-switch v-model:value="draft.ui.minimize_to_tray_on_close" />
              </n-form-item>
            </n-form>
          </n-card>
        </template>

        <!-- 快捷键 -->
        <template v-if="activeTab === 'hotkey'">
          <n-card title="快捷键">
            <n-form label-placement="left" :label-width="120">
              <n-form-item label="触发截图">
                <n-input v-model:value="draft.hotkey.trigger" placeholder="Ctrl+Alt+Q" />
              </n-form-item>
              <n-form-item label="取消选区">
                <n-input v-model:value="draft.hotkey.cancel" placeholder="Escape" />
              </n-form-item>
            </n-form>
            <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
              格式如 Ctrl+Alt+Q；保存后即时生效。档位切换需重启。
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
                <n-select v-model:value="draft.ocr.tier" :options="tierOptions" />
              </n-form-item>
              <n-form-item label="结果后处理">
                <n-switch v-model:value="draft.ocr.postprocess" />
                <span style="color: var(--st-text-weak); font-size: 12px; margin-left: 8px">
                  去空格 / 合并换行
                </span>
              </n-form-item>
            </n-form>
            <p style="color: var(--st-text-weak); font-size: 12px">档位切换需重启生效。</p>
          </n-card>
        </template>

        <!-- 翻译 -->
        <template v-if="activeTab === 'translate'">
          <n-card title="翻译引擎">
            <n-form label-placement="left" :label-width="100">
              <n-form-item label="引擎">
                <n-radio-group v-model:value="draft.translate.provider">
                  <n-radio value="DeepSeek" label="DeepSeek" />
                  <n-radio value="DeepL" label="DeepL" />
                  <n-radio value="Microsoft" label="Microsoft" />
                </n-radio-group>
              </n-form-item>
              <n-form-item label="目标语言">
                <n-select v-model:value="draft.translate.target_lang" :options="langOptions" />
              </n-form-item>

              <template v-if="draft.translate.provider === 'DeepSeek'">
                <n-form-item label="Base URL">
                  <n-input v-model:value="draft.translate.deepseek.base_url" />
                </n-form-item>
                <n-form-item label="模型">
                  <n-input v-model:value="draft.translate.deepseek.model" />
                </n-form-item>
                <n-form-item label="API Key">
                  <n-input
                    v-model:value="draft.translate.deepseek.api_key"
                    type="password"
                    show-password-on="click"
                    placeholder="sk-..."
                  />
                </n-form-item>
              </template>
              <template v-else-if="draft.translate.provider === 'DeepL'">
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
              <template v-else-if="draft.translate.provider === 'Microsoft'">
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
              <n-form-item label="点击行显示原文">
                <n-switch v-model:value="draft.ui.show_original" />
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
            <p><strong>SnapText</strong></p>
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
