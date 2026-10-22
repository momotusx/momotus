import { defineConfig, searchForWorkspaceRoot } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  // The page colors are read from the one token source, outside this package.
  server: { host: "127.0.0.1", port: Number(process.env.PORT ?? 5173), strictPort: true, fs: { allow: [searchForWorkspaceRoot(process.cwd()), "../../design/tokens.json"] } },
  build: { target: "es2020" },
});
