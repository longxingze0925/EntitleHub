import { createServer } from "vite";

const host = process.env.HOST ?? "127.0.0.1";
const port = Number(process.env.PORT ?? "5173");
const backendTarget = process.env.BACKEND_URL ?? "http://127.0.0.1:18080";

const server = await createServer({
  server: {
    host,
    port,
    strictPort: true,
    proxy: {
      "/api": {
        target: backendTarget,
        changeOrigin: true
      }
    }
  }
});

await server.listen();
server.printUrls();

process.on("SIGINT", async () => {
  await server.close();
  process.exit(0);
});

process.on("SIGTERM", async () => {
  await server.close();
  process.exit(0);
});

setInterval(() => undefined, 2 ** 30);
