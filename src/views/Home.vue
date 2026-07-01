<script setup lang="ts">
// 主窗口首页：状态卡 + 操作入口（截图/设置/历史）。
// 设置/历史开新窗口（独立 OS 窗口），截图走热键（也提供按钮提示）。
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NCard, NButton, NSpace, NTag, useMessage } from "naive-ui";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api } from "../api";
import { useConfigStore } from "../stores/config";

const message = useMessage();
const store = useConfigStore();
const router = useRouter();
const modelsReady = ref(false);
const opening = ref(false);

onMounted(async () => {
  await store.load();
  // 首启引导未完成 → 跳引导页（单标志位保证：中途关闭/崩溃仍 false → 下次重进）。
  if (store.config?.general.onboarding_completed === false) {
    router.replace("/onboarding");
    return;
  }
  modelsReady.value = await api.modelsReady(store.config?.ocr.tier ?? "medium").catch(() => false);
  // 热键注册失败（被其他程序占用等）：一次性引导用户去设置修改。
  if (store.hotkeyError) {
    message.warning("全局热键注册失败，可能被其他程序占用，请前往设置修改");
  }
});

async function openSettings() {
  await openWin("settings", "SnapText 设置", "#/settings", 720, 560);
}
async function openHistory() {
  await openWin("history", "SnapText 历史记录", "#/history", 880, 600);
}

async function openWin(label: string, title: string, hash: string, w: number, h: number) {
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.show();
    await existing.setFocus();
    return;
  }
  new WebviewWindow(label, {
    url: `index.html${hash}`,
    title,
    width: w,
    height: h,
    resizable: true,
    center: true,
  });
}

async function triggerCapture() {
  // 主面板按钮触发截图：调 Tauri 命令打开全屏选区窗口（与热键同路径）。
  opening.value = true;
  try {
    await api.triggerCapture();
  } catch (e) {
    message.error(`打开截图失败：${e}`);
  } finally {
    opening.value = false;
  }
}
</script>

<template>
  <div style="padding: 24px; height: 100vh; display: flex; flex-direction: column">
    <n-space vertical :size="16" style="flex: 1">
      <div>
        <h2 style="margin: 0 0 4px">SnapText</h2>
        <p style="margin: 0; color: var(--st-text-weak); font-size: 13px">截图 OCR + 翻译</p>
      </div>

      <!-- 状态卡 -->
      <n-card size="small">
        <n-space :size="8" align="center">
          <n-tag :type="modelsReady ? 'success' : 'warning'" size="small" round>
            模型：{{ modelsReady ? "就绪" : "缺失" }}
          </n-tag>
          <n-tag :type="store.translateReady ? 'success' : 'error'" size="small" round>
            翻译：{{ store.translateReady ? "就绪" : "未配置" }}
          </n-tag>
          <span style="color: var(--st-text-weak); font-size: 12px">
            热键：{{ store.config?.hotkey.trigger ?? "Ctrl+Alt+Q" }}
          </span>
        </n-space>
      </n-card>

      <!-- 操作入口 -->
      <n-card size="small" title="操作">
        <n-space vertical :size="10">
          <n-button type="primary" block :loading="opening" @click="triggerCapture">
            开始截图（{{ store.config?.hotkey.trigger ?? "Ctrl+Alt+Q" }}）
          </n-button>
          <n-space>
            <n-button @click="openSettings">设置</n-button>
            <n-button @click="openHistory">历史记录</n-button>
          </n-space>
        </n-space>
      </n-card>

      <p style="color: var(--st-text-weak); font-size: 12px; margin: 0">
        按热键截图，在屏幕上框选文字区域，译文会叠加在原文位置。
        {{ store.translateReady ? "" : "（翻译未配置，请先到设置填写 API Key）" }}
      </p>
    </n-space>
  </div>
</template>
