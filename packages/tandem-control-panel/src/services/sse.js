const channels = new Map();

const _metrics = {
  channels_open: 0,
  channels_total: 0,
  events_received: 0,
  errors_total: 0,
};

function buildKey(url, withCredentials) {
  return `${withCredentials ? "cred" : "anon"}:${url}`;
}

function recomputeMetrics() {
  let open = 0;
  for (const ch of channels.values()) {
    if (!ch.closed) open += 1;
  }
  _metrics.channels_open = open;
  _metrics.channels_total = channels.size;
}

function ensureChannel(url, withCredentials = true) {
  const key = buildKey(url, withCredentials);
  const existing = channels.get(key);
  if (existing && !existing.closed) return existing;

  const source = new EventSource(url, { withCredentials });
  const channel = {
    source,
    listeners: new Set(),
    errorListeners: new Set(),
    refs: 0,
    closed: false,
  };

  source.onmessage = (event) => {
    _metrics.events_received += 1;
    for (const listener of [...channel.listeners]) {
      try {
        listener(event);
      } catch {
        // listener failures are isolated
      }
    }
  };

  source.onerror = (event) => {
    _metrics.errors_total += 1;
    for (const listener of [...channel.errorListeners]) {
      try {
        listener(event);
      } catch {
        // ignore callback failures
      }
    }
    if (source.readyState === EventSource.CLOSED) {
      channel.closed = true;
      channels.delete(key);
      recomputeMetrics();
    }
  };

  channels.set(key, channel);
  recomputeMetrics();
  return channel;
}

export function subscribeSse(url, onMessage, options = {}) {
  const withCredentials = options.withCredentials !== false;
  const key = buildKey(url, withCredentials);
  const channel = ensureChannel(url, withCredentials);
  channel.refs += 1;
  channel.listeners.add(onMessage);
  if (typeof options.onError === "function") channel.errorListeners.add(options.onError);

  return () => {
    const current = channels.get(key);
    if (!current) return;
    current.listeners.delete(onMessage);
    if (typeof options.onError === "function") current.errorListeners.delete(options.onError);
    current.refs = Math.max(0, current.refs - 1);
    if (current.refs === 0) {
      current.closed = true;
      current.source.close();
      channels.delete(key);
      recomputeMetrics();
    }
  };
}

export function closeAllSseChannels() {
  for (const channel of channels.values()) {
    channel.closed = true;
    channel.source.close();
  }
  channels.clear();
  recomputeMetrics();
}

export function getSseMetrics() {
  return {
    ..._metrics,
    channels: {
      open: _metrics.channels_open,
      total: _metrics.channels_total,
    },
  };
}
