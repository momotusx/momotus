import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: { host: "127.0.0.1", port: Number(process.env.PORT ?? 5173), strictPort: true },
  build: { target: "es2020" },
});
