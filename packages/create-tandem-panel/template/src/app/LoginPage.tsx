import { useState } from "react";
import { GlowLayer, PanelCard, StatusPulse } from "../ui/index.tsx";

export function LoginPage({
  loginMutation,
  savedToken,
  onCheckEngine,
  controlPanelName,
}: {
  loginMutation: any;
  savedToken: string;
  onCheckEngine: () => Promise<string>;
  controlPanelName: string;
}) {
  const [token, setToken] = useState(savedToken);
  const [remember, setRemember] = useState(true);
  const [message, setMessage] = useState("");
  const [ok, setOk] = useState(false);

  return (
    <main className="relative min-h-screen overflow-hidden px-5 py-8">
      <GlowLayer className="tcp-shell-background">
        <div className="tcp-shell-glow tcp-shell-glow-a"></div>
        <div className="tcp-shell-glow tcp-shell-glow-b"></div>
      </GlowLayer>

      <div className="relative z-10 mx-auto grid min-h-[calc(100vh-4rem)] w-full max-w-6xl items-center gap-6 lg:grid-cols-[1.05fr_0.95fr]">
        <section className="grid gap-4">
          <div className="tcp-page-eyebrow">Tandem Control</div>
          <h1 className="tcp-page-title max-w-3xl">Sign in to continue.</h1>
          <p className="tcp-subtle max-w-2xl text-base">
            Enter your engine token to access chat, orchestrator, automations, memory, live feed,
            and settings.
          </p>
        </section>

        <PanelCard
          title={controlPanelName}
          subtitle="Authenticate against your Tandem engine to continue."
        >
          <form
            className="grid gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (!token.trim()) {
                setOk(false);
                setMessage("Token is required.");
                return;
              }
              loginMutation.mutate({ token: token.trim(), remember });
            }}
          >
            <label className="text-sm tcp-subtle">Engine token</label>
            <input
              className="tcp-input"
              type="password"
              value={token}
              onInput={(e) => setToken((e.target as HTMLInputElement).value)}
              placeholder="tk_..."
              autoComplete="off"
            />

            <label className="inline-flex items-center gap-2 text-xs tcp-subtle">
              <input
                type="checkbox"
                className="h-4 w-4 accent-slate-400"
                checked={remember}
                onChange={(e) => setRemember((e.target as HTMLInputElement).checked)}
              />
              Remember token on this browser
            </label>

            <div className="grid gap-2 sm:grid-cols-2">
              <button
                disabled={loginMutation.isPending}
                type="submit"
                className="tcp-btn-primary w-full"
              >
                <i data-lucide="key-round"></i>
                Sign in
              </button>
              <button
                type="button"
                className="tcp-btn w-full"
                onClick={async () => {
                  try {
                    const result = await onCheckEngine();
                    setOk(true);
                    setMessage(result);
                  } catch (error) {
                    setOk(false);
                    setMessage(error instanceof Error ? error.message : String(error));
                  }
                }}
              >
                <i data-lucide="activity"></i>
                Check engine
              </button>
            </div>

            <div className={`min-h-[1.2rem] text-sm ${ok ? "text-lime-300" : "text-rose-300"}`}>
              {loginMutation.error?.message || message}
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
              <div className="mb-2 flex items-center justify-between gap-3">
                <div className="font-medium">Readiness</div>
                <StatusPulse tone={ok ? "ok" : "warn"} text={ok ? "Engine reachable" : "Waiting"} />
              </div>
              <p className="tcp-subtle text-xs">
                Connectivity checks are non-destructive and help verify the local panel can reach
                the engine before authentication.
              </p>
            </div>
          </form>
        </PanelCard>
      </div>
    </main>
  );
}
