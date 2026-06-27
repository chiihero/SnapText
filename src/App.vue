<script setup lang="ts">
// 根组件：Naive UI 主题 + 中文全局配置；router-view 渲染各窗口视图。
import { darkTheme, zhCN, dateZhCN } from "naive-ui";
import { computed } from "vue";

// 工具类应用统一浅色（darkTheme 保留 import 备用）。
const theme = computed(() => null);
const _keepDark = darkTheme; // 引用避免 unused
</script>

<template>
  <n-config-provider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-dialog-provider>
        <router-view />
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
