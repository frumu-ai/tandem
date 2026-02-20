export class EngineAPI {
  private baseUrl: string;
  private token: string | null;

  constructor(token: string | null = null) {
    this.baseUrl = "/engine";
    this.token = token;
  }

  setToken(token: string) {
    this.token = token;
  }

  get isConfigured() {
    return !!this.token;
  }

  private get headers() {
    return {
      "Content-Type": "application/json",
      ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
    };
  }

  async createSession(title: string = "Web Portal Session"): Promise<string> {
    const res = await fetch(`${this.baseUrl}/session`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({ title, directory: "." }),
    });
    if (!res.ok) throw new Error(`Failed to create session: ${res.statusText}`);
    const data = await res.json();
    return data.id;
  }

  async sendMessage(sessionId: string, text: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/session/${sessionId}/message`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({ parts: [{ type: "text", text }] }),
    });
    if (!res.ok) throw new Error(`Failed to send message: ${res.statusText}`);
  }

  async startAsyncRun(
    sessionId: string,
    messageText?: string
  ): Promise<{ runId: string; attachPath: string }> {
    const payload = messageText ? { parts: [{ type: "text", text: messageText }] } : {};
    const res = await fetch(`${this.baseUrl}/session/${sessionId}/prompt_async?return=run`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify(payload),
    });
    if (!res.ok) throw new Error(`Failed to start run: ${res.statusText}`);

    const data = await res.json();
    return {
      runId: data.id || data.runId || data.runID,
      attachPath: `${this.baseUrl}/event?sessionID=${sessionId}&runID=${data.id || data.runId || data.runID}&token=${this.token}`,
    };
  }

  getEventStreamUrl(sessionId: string, runId: string): string {
    return `${this.baseUrl}/event?sessionID=${sessionId}&runID=${runId}&token=${this.token}`;
  }

  async getSystemHealth(): Promise<any> {
    const res = await fetch(`${this.baseUrl}/global/health`, {
      headers: this.headers,
    });
    if (!res.ok) throw new Error(`Health check failed: ${res.statusText}`);
    return res.json();
  }
}

// Global singleton
export const api = new EngineAPI();
