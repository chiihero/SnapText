import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import { api } from "./api";
import "./styles/global.css";

const app = createApp(App);

// 诊断：把 Vue 渲染错误 + 未捕获异常转发到后端日志（排查白屏用，定位后移除）。
const report = (where: string, e: unknown) => {
  const msg = e instanceof Error ? `${e.name}: ${e.message}\n${e.stack ?? ""}` : String(e);
  api.logDiag("ui_error", `[${where}] ${msg}`).catch(() => {});
};
app.config.errorHandler = (err, _instance, info) => report("vue", `${err} @ ${info}`);
window.addEventListener("error", (ev) => report("window", ev.error ?? ev.message));
window.addEventListener("unhandledrejection", (ev) => report("promise", ev.reason));

app.use(createPinia()).use(router).mount("#app");
