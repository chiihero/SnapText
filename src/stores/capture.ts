// 选区/结果共享状态：Capture.vue 写入 lastResult，Result.vue 读取渲染。
import { defineStore } from "pinia";
import type { SelectResult } from "../api";

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    lastResult: null as SelectResult | null,
    // 显示态：每行是否显示原文（index → bool）。Result.vue 维护。
    showOriginalPerLine: [] as boolean[],
    // 全局切换：全部行显示原文 / 全部显示译文。
    showAllOriginal: false,
  }),
});
