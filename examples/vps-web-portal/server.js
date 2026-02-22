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
const DEBUG_PROXY_AUTH = process.env.DEBUG_PROXY_AUTH === '1';

app.use(cors());

const extractBearerOrQueryToken = (req) => {
    const headerAuth = req.headers['authorization'];
    if (typeof headerAuth === 'string' && headerAuth.startsWith('Bearer ')) {
        return headerAuth.slice('Bearer '.length);
    }

    if (typeof req.query?.token === 'string' && req.query.token.length > 0) {
        return req.query.token;
    }

    const candidates = [req.originalUrl, req.url];
    for (const raw of candidates) {
        if (!raw || typeof raw !== 'string') continue;
        try {
            const parsed = new URL(raw, 'http://localhost');
            const token = parsed.searchParams.get('token');
            if (token) return token;
        } catch {
            // Ignore malformed values and continue.
        }
    }

    return null;
};

const tokenSource = (req) => {
    const headerAuth = req.headers['authorization'];
    if (typeof headerAuth === 'string' && headerAuth.startsWith('Bearer ')) {
        return 'authorization-header';
    }
    if (typeof req.query?.token === 'string' && req.query.token.length > 0) {
        return 'query-param';
    }
    return 'url-search';
};

// Simple Auth Middleware
const requireAuth = (req, res, next) => {
    // SSE requests might pass token in query param instead of header
    const token = extractBearerOrQueryToken(req);

    if (token === SERVER_KEY) {
        req.portalAuthToken = token;
        if (DEBUG_PROXY_AUTH) {
            console.log(`[proxy-auth] allow ${req.method} ${req.originalUrl || req.url} source=${tokenSource(req)} token_len=${token.length}`);
        }
        next();
    } else {
        if (DEBUG_PROXY_AUTH) {
            console.log(`[proxy-auth] deny ${req.method} ${req.originalUrl || req.url} source=${tokenSource(req)} token_present=${!!token}`);
        }
        res.status(401).json({ error: 'Unauthorized: Invalid SERVER_KEY' });
    }
};

// Global CORS preflight for all proxy requests
app.options(/^\/engine(\/|$)/, cors());

const handleProxyReq = (proxyReq, req) => {
    // Browser EventSource cannot set custom headers. Always forward the
    // validated token to engine as Authorization for consistency.
    const token = req.portalAuthToken || extractBearerOrQueryToken(req);
    if (token) {
        proxyReq.setHeader('Authorization', `Bearer ${token}`);
    }
    if (DEBUG_PROXY_AUTH) {
        const hasAuthHeader = !!proxyReq.getHeader('authorization');
        console.log(`[proxy-auth] forward ${req.method} ${req.originalUrl || req.url} token_present=${!!token} auth_header_set=${hasAuthHeader}`);
    }

    if (req.headers.accept && req.headers.accept === 'text/event-stream') {
        proxyReq.setHeader('Cache-Control', 'no-cache');
    }
};

// Proxy requests starting with /engine to the local tandem-engine
app.use('/engine', requireAuth, createProxyMiddleware({
    target: TANDEM_ENGINE_URL,
    changeOrigin: true,
    pathRewrite: { '^/engine': '' },
    // For http-proxy-middleware v3+
    on: {
        proxyReq: handleProxyReq,
    },
    // Backward compatibility for older versions.
    onProxyReq: handleProxyReq,
}));

// Serve built Vite frontend
app.use(express.static(path.join(__dirname, 'dist')));

// Fallback to React Router (Express 5 requires named wildcard params)
app.get('/{*path}', (req, res) => {
    res.sendFile(path.join(__dirname, 'dist', 'index.html'));
});

const server = app.listen(PORT, () => {
    console.log(`VPS Web Portal proxy running on port ${PORT}`);
    console.log(`Proxying /engine -> ${TANDEM_ENGINE_URL}`);
});

server.on('close', () => {
    console.warn('HTTP server closed');
});

server.on('error', (err) => {
    console.error('HTTP server error:', err);
    process.exit(1);
});

process.on('SIGTERM', () => {
    console.log('Received SIGTERM; shutting down portal');
    server.close(() => process.exit(0));
});
