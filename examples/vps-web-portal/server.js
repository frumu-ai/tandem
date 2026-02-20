import express from 'express';
import { createProxyMiddleware } from 'http-proxy-middleware';
import cors from 'cors';
import dotenv from 'dotenv';
import path from 'path';
import { fileURLToPath } from 'url';

dotenv.config();

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const TANDEM_ENGINE_URL = process.env.VITE_TANDEM_ENGINE_URL || 'http://127.0.0.1:39731';
const SERVER_KEY = process.env.VITE_PORTAL_KEY || 'default-secret-key';
const PORT = process.env.PORT || 3000;

app.use(cors());

// Simple Auth Middleware
const requireAuth = (req, res, next) => {
    // SSE requests might pass token in query param instead of header
    const token = req.headers['authorization']?.replace('Bearer ', '') || req.query.token;

    if (token === SERVER_KEY) {
        next();
    } else {
        res.status(401).json({ error: 'Unauthorized: Invalid SERVER_KEY' });
    }
};

// Global CORS preflight for all proxy requests
app.options('/engine/*', cors());

// Proxy requests starting with /engine to the local tandem-engine
app.use('/engine', requireAuth, createProxyMiddleware({
    target: TANDEM_ENGINE_URL,
    changeOrigin: true,
    pathRewrite: { '^/engine': '' },
    // Enable proxying of Server-Sent Events (SSE)
    onProxyReq: (proxyReq, req, res) => {
        if (req.headers.accept && req.headers.accept === 'text/event-stream') {
            proxyReq.setHeader('Cache-Control', 'no-cache');
        }
    }
}));

// Serve built Vite frontend
app.use(express.static(path.join(__dirname, 'dist')));

// Fallback to React Router
app.get('*', (req, res) => {
    res.sendFile(path.join(__dirname, 'dist', 'index.html'));
});

app.listen(PORT, () => {
    console.log(`VPS Web Portal proxy running on port ${PORT}`);
    console.log(`Proxying /engine -> ${TANDEM_ENGINE_URL}`);
});
