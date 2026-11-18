import { defineConfig, searchForWorkspaceRoot, type UserConfig } from "vite";
import solid from "vite-plugin-solid";
import { webos } from "./scripts/webos-build";

export default defineConfig(({ mode }): UserConfig => {
  const config: UserConfig = {
    plugins: [solid()],
    // The page colors are read from the one token source, outside this package.
    server: { host: "127.0.0.1", port: Number(process.env.PORT ?? 5173), strictPort: true, fs: { allow: [searchForWorkspaceRoot(process.cwd()), "../../design/tokens.json"] } },
    build: { target: "es2020" },
  };
  // The LG webOS TV build (plans/webos.md section 2): Chromium 79, served by the app's own service.
  return mode === "webos" ? webos(config) : config;
});
