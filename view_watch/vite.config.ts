
// view_watch/vite.config.ts
// 配置前端构建目标和开发服务代理.
import { defineConfig } from "vite";

export default defineConfig({
  build: { target: ["es2022", "safari15"] },
  server: {
    proxy: { "/rest": { target: process.env.WATCH_DEV_REST ?? "http://127.0.0.1:3000", rewrite: (path) => path.replace(/^\/rest/, "") } },
  },
});
