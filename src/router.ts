import { createRouter, createWebHashHistory, type RouteRecordRaw } from "vue-router";

// 路由设计：多窗口共用一套路由表，靠 query.window 区分窗口类型。
// 实际窗口内容由 windowKind 决定渲染哪个视图（见各 view 内的判断）。
const routes: RouteRecordRaw[] = [
  { path: "/", redirect: "/home" },
  { path: "/home", component: () => import("./views/Home.vue") },
  { path: "/onboarding", component: () => import("./views/Onboarding.vue") },
  { path: "/settings", component: () => import("./views/Settings.vue") },
  { path: "/history", component: () => import("./views/History.vue") },
  { path: "/capture", component: () => import("./views/Capture.vue") },
  { path: "/result", component: () => import("./views/Result.vue") },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});
