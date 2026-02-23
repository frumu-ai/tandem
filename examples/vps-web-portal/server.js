import express from 'express';
import { createProxyMiddleware, fixRequestBody } from 'http-proxy-middleware';
import cors from 'cors';
import dotenv from 'dotenv';
import path from 'path';
import { fileURLToPath } from 'url';
import fs from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

dotenv.config();

const execFileAsync = promisify(execFile);

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const TANDEM_ENGINE_URL = process.env.VITE_TANDEM_ENGINE_URL || 'http://127.0.0.1:39731';
const SERVER_KEY = process.env.VITE_PORTAL_KEY || 'default-secret-key';
const PORT = process.env.PORT || 3000;
const DEBUG_PROXY_AUTH = process.env.DEBUG_PROXY_AUTH === '1';

const SYSTEM_CONTROL_MODE = process.env.TANDEM_SYSTEM_CONTROL_MODE || 'systemd';
const ENGINE_SERVICE_NAME = process.env.TANDEM_ENGINE_SERVICE_NAME || 'tandem-engine.service';
const ENGINE_CONTROL_SCRIPT = process.env.TANDEM_ENGINE_CONTROL_SCRIPT || '/usr/local/bin/tandem-engine-ctl';
const ARTIFACT_ROOTS = (process.env.TANDEM_ARTIFACT_READ_ROOTS || '/srv/tandem')
  .split(',')
  .map((v) => v.trim())
  .filter(Boolean);
const PORTAL_MAX_ARTIFACT_BYTES = Number.parseInt(
  process.env.TANDEM_PORTAL_MAX_ARTIFACT_BYTES || '1048576',
  10
);

app.use(cors());
app.use(express.json({ limit: '1mb' }));

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

const requireAuth = (req, res, next) => {
  const token = extractBearerOrQueryToken(req);

  if (token === SERVER_KEY) {
    req.portalAuthToken = token;
    if (DEBUG_PROXY_AUTH) {
      console.log(
        `[proxy-auth] allow ${req.method} ${req.originalUrl || req.url} source=${tokenSource(req)} token_len=${token.length}`
      );
    }
    next();
    return;
  }

  if (DEBUG_PROXY_AUTH) {
    console.log(
      `[proxy-auth] deny ${req.method} ${req.originalUrl || req.url} source=${tokenSource(req)} token_present=${!!token}`
    );
  }
  res.status(401).json({ error: 'Unauthorized: Invalid SERVER_KEY' });
};

app.options(/^\/engine(\/|$)/, cors());

const handleProxyReq = (proxyReq, req) => {
  const token = req.portalAuthToken || extractBearerOrQueryToken(req);
  if (token) {
    proxyReq.setHeader('Authorization', `Bearer ${token}`);
  }
  if (DEBUG_PROXY_AUTH) {
    const hasAuthHeader = !!proxyReq.getHeader('authorization');
    console.log(
      `[proxy-auth] forward ${req.method} ${req.originalUrl || req.url} token_present=${!!token} auth_header_set=${hasAuthHeader}`
    );
  }

  if (req.headers.accept && req.headers.accept === 'text/event-stream') {
    proxyReq.setHeader('Cache-Control', 'no-cache');
  }

  // express.json() consumes request streams; re-write JSON bodies for proxied PUT/PATCH/POST.
  fixRequestBody(proxyReq, req);
};

const runControlScript = async (action) => {
  if (SYSTEM_CONTROL_MODE !== 'systemd') {
    throw new Error(`Unsupported TANDEM_SYSTEM_CONTROL_MODE='${SYSTEM_CONTROL_MODE}'`);
  }

  const cmd = '/usr/bin/sudo';
  const args = [ENGINE_CONTROL_SCRIPT, action, ENGINE_SERVICE_NAME];
  const { stdout } = await execFileAsync(cmd, args, { timeout: 15000, maxBuffer: 1024 * 1024 });

  let parsed;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    parsed = { ok: true, action, raw: stdout.trim() };
  }
  return parsed;
};

const getControlCapabilities = async () => {
  try {
    await fs.access(ENGINE_CONTROL_SCRIPT);
    return {
      processControl: {
        enabled: SYSTEM_CONTROL_MODE === 'systemd',
        mode: SYSTEM_CONTROL_MODE,
        serviceName: ENGINE_SERVICE_NAME,
        scriptPath: ENGINE_CONTROL_SCRIPT,
      },
      artifactPreview: {
        enabled: true,
        roots: ARTIFACT_ROOTS,
        maxBytes: PORTAL_MAX_ARTIFACT_BYTES,
      },
    };
  } catch {
    return {
      processControl: {
        enabled: false,
        mode: SYSTEM_CONTROL_MODE,
        serviceName: ENGINE_SERVICE_NAME,
        scriptPath: ENGINE_CONTROL_SCRIPT,
        reason: 'control script missing',
      },
      artifactPreview: {
        enabled: true,
        roots: ARTIFACT_ROOTS,
        maxBytes: PORTAL_MAX_ARTIFACT_BYTES,
      },
    };
  }
};

const ensureArtifactPathAllowed = async (uri) => {
  if (!uri || typeof uri !== 'string' || !uri.startsWith('file://')) {
    throw new Error('Only file:// artifact URIs are supported for preview');
  }

  const filePath = decodeURIComponent(uri.slice('file://'.length));
  const realPath = await fs.realpath(filePath);

  const allowed = ARTIFACT_ROOTS.some((root) => realPath.startsWith(root));
  if (!allowed) {
    throw new Error('Artifact path is outside configured TANDEM_ARTIFACT_READ_ROOTS');
  }

  return { filePath, realPath };
};

const toEnvStyle = (providerId) =>
  String(providerId || '')
    .trim()
    .replace(/[^a-zA-Z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toUpperCase();

const resolveProviderKeyCandidates = (providerId) => {
  const normalized = toEnvStyle(providerId);
  const aliases = {
    OPENROUTER: ['OPENROUTER_API_KEY'],
    OPENAI: ['OPENAI_API_KEY'],
    ANTHROPIC: ['ANTHROPIC_API_KEY'],
    GOOGLE: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
    GEMINI: ['GEMINI_API_KEY', 'GOOGLE_API_KEY'],
    XAI: ['XAI_API_KEY'],
    GROQ: ['GROQ_API_KEY'],
    COHERE: ['COHERE_API_KEY'],
    MISTRAL: ['MISTRAL_API_KEY'],
    TOGETHER: ['TOGETHER_API_KEY'],
    PERPLEXITY: ['PERPLEXITY_API_KEY'],
    DEEPSEEK: ['DEEPSEEK_API_KEY'],
  };

  const mapped = aliases[normalized] || [];
  const fallback = normalized ? [`${normalized}_API_KEY`] : [];
  return Array.from(new Set([...mapped, ...fallback]));
};

const maskKeyPreview = (value) => {
  if (!value) return '';
  if (value.length <= 6) return `${value.slice(0, 2)}...`;
  return `${value.slice(0, 6)}...`;
};

app.get('/portal/system/capabilities', requireAuth, async (_req, res) => {
  const caps = await getControlCapabilities();
  res.json(caps);
});

app.get('/portal/system/engine/status', requireAuth, async (_req, res) => {
  try {
    const status = await runControlScript('status');
    res.json(status);
  } catch (error) {
    res.status(500).json({ ok: false, error: String(error.message || error) });
  }
});

app.post('/portal/system/engine/:action', requireAuth, async (req, res) => {
  const action = req.params.action;
  if (!['start', 'stop', 'restart'].includes(action)) {
    res.status(400).json({ ok: false, error: 'Unsupported action' });
    return;
  }

  try {
    const result = await runControlScript(action);
    const status = await runControlScript('status');
    res.json({ ok: true, action, result, status });
  } catch (error) {
    res.status(500).json({ ok: false, action, error: String(error.message || error) });
  }
});

app.get('/portal/artifacts/content', requireAuth, async (req, res) => {
  try {
    const uri = typeof req.query.uri === 'string' ? req.query.uri : '';
    const { realPath } = await ensureArtifactPathAllowed(uri);
    const stat = await fs.stat(realPath);

    const full = await fs.readFile(realPath);
    const truncated = full.byteLength > PORTAL_MAX_ARTIFACT_BYTES;
    const body = truncated ? full.subarray(0, PORTAL_MAX_ARTIFACT_BYTES) : full;

    const ext = path.extname(realPath).toLowerCase();
    const kind = ext === '.json' ? 'json' : ext === '.md' ? 'markdown' : 'text';

    res.json({
      ok: true,
      uri,
      path: realPath,
      kind,
      truncated,
      size: stat.size,
      content: body.toString('utf8'),
    });
  } catch (error) {
    res.status(400).json({ ok: false, error: String(error.message || error) });
  }
});

app.get('/portal/provider/key-preview', requireAuth, async (req, res) => {
  const providerId = String(req.query.providerId || '').trim();
  if (!providerId) {
    res.status(400).json({ ok: false, error: 'providerId is required' });
    return;
  }

  const candidates = resolveProviderKeyCandidates(providerId);
  for (const envVar of candidates) {
    const value = process.env[envVar];
    if (typeof value === 'string' && value.trim().length > 0) {
      res.json({
        ok: true,
        present: true,
        envVar,
        preview: maskKeyPreview(value.trim()),
      });
      return;
    }
  }

  res.json({
    ok: true,
    present: false,
    envVar: candidates[0] || null,
    preview: '',
  });
});

app.use(
  '/engine',
  requireAuth,
  createProxyMiddleware({
    target: TANDEM_ENGINE_URL,
    changeOrigin: true,
    pathRewrite: { '^/engine': '' },
    on: {
      proxyReq: handleProxyReq,
    },
  })
);

app.use(express.static(path.join(__dirname, 'dist')));

app.get('/{*path}', (_req, res) => {
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
