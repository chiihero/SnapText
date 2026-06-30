// 配置状态：启动拉取 + 保存后更新。供 Settings/Home 共享。
import { defineStore } from "pinia";
import { api, type Config } from "../api";

export const useConfigStore = defineStore("config", {
  state: () => ({
    config: null as Config | null,
    translateReady: false,
    // 全局热键注册状态：null=已注册；非空字符串=注册失败（被占用等）。
    hotkeyError: null as string | null,
    loading: true,
  }),
  actions: {
    async load() {
      this.loading = true;
      try {
        this.config = await api.getConfig();
        this.translateReady = await api.checkTranslateReady();
        this.hotkeyError = await api.getHotkeyStatus().catch(() => null);
      } finally {
        this.loading = false;
      }
    },
    // 保存草稿，返回新配置是否让翻译可用。
    async save(draft: Config): Promise<boolean> {
      const ready = await api.saveConfig(draft);
      this.config = draft;
      this.translateReady = ready;
      // save_config 重注册热键后刷新状态（成功→null，失败→错误文案）。
      this.hotkeyError = await api.getHotkeyStatus().catch(() => null);
      return ready;
    },
  },
});
