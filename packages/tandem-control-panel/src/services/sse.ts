type Listener = (event: MessageEvent<string>) => void;

type Channel = {
  source: EventSource;
  listeners: Set<Listener>;
  errorListeners: Set<(event: Event) => void>;
  refs: number;
  closed: boolean;
};

const channels = new Map<string, Channel>();

function buildKey(url: string, withCredentials: boolean) {
  return `${withCredentials ? "cred" : "anon"}:${url}`;
}

function ensureChannel(url: string, withCredentials = true) {
  const key = buildKey(url, withCredentials);
  const existing = channels.get(key);
  if (existing && !existing.closed) return existing;

  const source = new EventSource(url, { withCredentials });
  const channel: Channel = {
    source,
    listeners: new Set(),
    errorListeners: new Set(),
    refs: 0,
    closed: false,
  };

  source.onmessage = (event) => {
    for (const listener of [...channel.listeners]) {
      try {
        listener(event);
      } catch {
        // listener failures are isolated
      }
    }
  };

  source.onerror = (event) => {
    for (const listener of [...channel.errorListeners]) {
      try {
        listener(event);
      } catch {
        // ignore callback failures
      }
    }
  };

  channels.set(key, channel);
  return channel;
}

export function subscribeSse(
  url: string,
  onMessage: Listener,
  options: { withCredentials?: boolean; onError?: (event: Event) => void } = {}
) {
  const withCredentials = options.withCredentials !== false;
  const key = buildKey(url, withCredentials);
  const channel = ensureChannel(url, withCredentials);
  channel.refs += 1;
  channel.listeners.add(onMessage);
  if (options.onError) channel.errorListeners.add(options.onError);

  return () => {
    const current = channels.get(key);
    if (!current) return;
    current.listeners.delete(onMessage);
    if (options.onError) current.errorListeners.delete(options.onError);
    current.refs = Math.max(0, current.refs - 1);
    if (current.refs === 0) {
      current.closed = true;
      current.source.close();
      channels.delete(key);
    }
  };
}

export function closeAllSseChannels() {
  for (const channel of channels.values()) {
    channel.closed = true;
    channel.source.close();
  }
  channels.clear();
}
