<script setup lang="ts">
// 历史面板：左侧列表 + 右侧详情（截图缩略图 + 原文/译文）。
// 搜索/刷新/单删/清空。数据经 api.historyList 等命令。
import { onMounted, ref } from "vue";
import {
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NInput,
  NButton,
  NSpace,
  NEmpty,
  NPopconfirm,
  NTag,
  NImage,
  NScrollbar,
  useMessage,
} from "naive-ui";
import { api, type HistoryDto } from "../api";

const message = useMessage();
const records = ref<HistoryDto[]>([]);
const selectedId = ref<number | null>(null);
const keyword = ref("");
const loading = ref(false);
const shotUrl = ref<string | null>(null);

const selected = () => records.value.find((r) => r.id === selectedId.value) ?? null;

async function reload() {
  loading.value = true;
  try {
    records.value = keyword.value.trim()
      ? await api.historySearch(100, keyword.value.trim())
      : await api.historyList(100);
    if (records.value.length && selectedId.value === null) {
      selectedId.value = records.value[0].id;
      await loadShot();
    } else if (selectedId.value !== null) {
      await loadShot();
    }
  } catch (e) {
    message.error(`加载失败：${e}`);
  } finally {
    loading.value = false;
  }
}

async function loadShot() {
  const s = selected();
  if (!s) {
    shotUrl.value = null;
    return;
  }
  shotUrl.value = s.has_screenshot ? await api.historyGetScreenshot(s.id) : null;
}

async function onSelect(r: HistoryDto) {
  selectedId.value = r.id;
  await loadShot();
}

async function onSearch() {
  await reload();
}

async function del(id: number) {
  try {
    await api.historyDelete(id);
    message.success("已删除");
    if (selectedId.value === id) selectedId.value = null;
    await reload();
  } catch (e) {
    message.error(`删除失败：${e}`);
  }
}

async function clearAll() {
  try {
    const n = await api.historyClear();
    message.success(`已清空 ${n} 条`);
    selectedId.value = null;
    shotUrl.value = null;
    await reload();
  } catch (e) {
    message.error(`清空失败：${e}`);
  }
}

function fmt(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function preview(r: HistoryDto): string {
  return r.original_text.replace(/\n/g, " ").slice(0, 28);
}

onMounted(reload);
</script>

<template>
  <n-layout has-sider style="height: 100vh">
    <n-layout-sider bordered :width="300" content-style="display: flex; flex-direction: column">
      <!-- 顶部搜索/操作 -->
      <div style="padding: 10px; border-bottom: 1px solid var(--st-border); display: flex; gap: 6px">
        <n-input
          v-model:value="keyword"
          size="small"
          placeholder="搜索原文/译文…"
          @keyup.enter="onSearch"
        />
        <n-button size="small" @click="reload" :loading="loading">刷新</n-button>
      </div>
      <div style="padding: 6px 10px; border-bottom: 1px solid var(--st-border)">
        <n-popconfirm @positive-click="clearAll">
          <template #trigger>
            <n-button size="tiny" quaternary type="error">清空全部</n-button>
          </template>
          确认清空全部历史？
        </n-popconfirm>
      </div>
      <!-- 列表 -->
      <n-scrollbar style="flex: 1">
        <n-empty v-if="!records.length" description="暂无历史记录" style="padding: 24px" />
        <div
          v-for="r in records"
          :key="r.id"
          @click="onSelect(r)"
          :style="{
            padding: '8px 12px',
            cursor: 'pointer',
            borderBottom: '1px solid var(--st-border)',
            background: selectedId === r.id ? 'var(--st-accent-soft)' : 'transparent',
          }"
        >
          <div style="font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis">
            {{ preview(r) || "（空）" }}
          </div>
          <div style="font-size: 12px; color: var(--st-text-weak); margin-top: 2px">
            {{ fmt(r.created_at_ms) }} · {{ r.provider }}
          </div>
        </div>
      </n-scrollbar>
    </n-layout-sider>

    <n-layout-content content-style="padding: 16px 20px; overflow: auto">
      <template v-if="selected()">
        <n-space vertical :size="12">
          <div style="display: flex; align-items: center; justify-content: space-between">
            <h3 style="margin: 0">详情</h3>
            <n-popconfirm @positive-click="del(selected()!.id)">
              <template #trigger>
                <n-button size="small" type="error" ghost>删除此条</n-button>
              </template>
              确认删除？
            </n-popconfirm>
          </div>

          <div v-if="shotUrl" style="text-align: center; background: var(--st-surface); border-radius: var(--st-radius); padding: 8px">
            <n-image :src="shotUrl" style="max-width: 100%; max-height: 360px" />
          </div>

          <div style="background: var(--st-surface); border-radius: var(--st-radius); padding: 12px; box-shadow: var(--st-shadow)">
            <div style="font-weight: 600; margin-bottom: 6px">原文</div>
            <div style="white-space: pre-wrap">{{ selected()?.original_text }}</div>
          </div>
          <div style="background: var(--st-surface); border-radius: var(--st-radius); padding: 12px; box-shadow: var(--st-shadow)">
            <div style="font-weight: 600; margin-bottom: 6px">译文</div>
            <div style="white-space: pre-wrap">{{ selected()?.translated_text }}</div>
          </div>
          <div>
            <n-tag size="small" type="info" style="margin-right: 6px">
              {{ selected()?.source_lang }} → {{ selected()?.target_lang }}
            </n-tag>
            <n-tag size="small">{{ selected()?.provider }}</n-tag>
            <n-tag v-if="selected()?.model" size="small">{{ selected()?.model }}</n-tag>
          </div>
        </n-space>
      </template>
      <n-empty v-else description="← 在左侧选择一条记录" style="margin-top: 80px" />
    </n-layout-content>
  </n-layout>
</template>
