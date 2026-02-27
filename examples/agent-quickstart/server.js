import express from "express";
import { createProxyMiddleware } from "http-proxy-middleware";
import { createRequire } from "module";
import path from "path";
import { fileURLToPath } from "url";

const require = createRequire(import.meta.url);
const dotenv = require("dotenv");
dotenv.config();

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const app = express();

const ENGINE_URL = process.env.TANDEM_ENGINE_URL || "http://127.0.0.1:39731";
const PORT = parseInt(process.env.PORT || "3030", 10);
const PORTAL_KEY = process.env.PORTAL_KEY;

/* ── Bearer auth middleware ── */
if (PORTAL_KEY) {
    app.use((req, res, next) => {
        // Protect engine proxy only; keep UI pages/assets publicly reachable.
        if (!req.path.startsWith("/engine")) { next(); return; }
        // Allow preflight and health passthrough
        if (req.method === "OPTIONS" || req.path === "/health") { next(); return; }
        const auth = req.headers.authorization || "";
        if (auth.startsWith("Bearer ") && auth.slice(7) === PORTAL_KEY) { next(); return; }
        res.status(401).json({ error: "Unauthorized" });
    });
}

/* ── Engine proxy ── */
app.use(
    "/engine",
    createProxyMiddleware({
        target: ENGINE_URL,
        changeOrigin: true,
        pathRewrite: { "^/engine": "" },
        on: {
            error: (_err, _req, res) => {
                if (res && "status" in res) {
                    res.status(502).json({ error: "Engine unreachable." });
                }
            },
        },
    })
);

/* ── Static (production build) ── */
const distDir = path.join(__dirname, "dist");
app.use(express.static(distDir));
app.get(/.*/, (_req, res) => {
    res.sendFile(path.join(distDir, "index.html"));
});

/* ── Health ── */
app.get("/health", (_req, res) => { res.json({ ok: true }); });

app.listen(PORT, () => {
    console.log(`\n🚀  Tandem Agent Quickstart`);
    console.log(`   Portal:  http://localhost:${PORT}`);
    console.log(`   Engine:  ${ENGINE_URL}`);
    if (!PORTAL_KEY) console.warn(`   ⚠  PORTAL_KEY not set — no auth protection.`);
    console.log();
});
