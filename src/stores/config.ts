// 配置状态：启动拉取 + 保存后更新。供 Settings/Home 共享。
import { defineStore } from "pinia";
import { api, type Config } from "../api";

export const useConfigStore = defineStore("config", {
  state: () => ({
    config: null as Config | null,
    translateReady: false,
    loading: true,
  }),
  actions: {
    async load() {
      this.loading = true;
      try {
        this.config = await api.getConfig();
        this.translateReady = await api.checkTranslateReady();
      } finally {
        this.loading = false;
      }
    },
    // 保存草稿，返回新配置是否让翻译可用。
    async save(draft: Config): Promise<boolean> {
      const ready = await api.saveConfig(draft);
      this.config = draft;
      this.translateReady = ready;
      return ready;
    },
  },
});
