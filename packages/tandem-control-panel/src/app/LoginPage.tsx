import { useState } from "react";
import { PanelCard, StatusPulse } from "../ui/index.tsx";
import { Icon } from "../ui/Icon";

export function LoginPage({
  loginMutation,
  savedToken,
  onCheckEngine,
  controlPanelName,
  controlPanelMode,
  controlPanelModeReason,
  hostedManaged = false,
  hostedAuthAvailable = false,
  hostedLoginUrl = "",
}: {
  loginMutation: any;
  savedToken: string;
  onCheckEngine: () => Promise<string>;
  controlPanelName: string;
  controlPanelMode?: string;
  controlPanelModeReason?: string;
  hostedManaged?: boolean;
  hostedAuthAvailable?: boolean;
  hostedLoginUrl?: string;
}) {
  const [token, setToken] = useState(savedToken);
  const [remember, setRemember] = useState(true);
  const [message, setMessage] = useState("");
  const [ok, setOk] = useState(false);
  const requireHostedLogin = hostedManaged && hostedAuthAvailable;

  return (
    <main className="relative min-h-screen overflow-hidden px-5 py-8">
      <div className="relative z-10 mx-auto grid min-h-[calc(100vh-4rem)] w-full max-w-6xl items-center gap-6 lg:grid-cols-[1.05fr_0.95fr]">
        <section className="grid gap-4">
          <div className="tcp-page-eyebrow">Tandem Control</div>
          <h1 className="tcp-page-title max-w-3xl">Sign in to continue.</h1>
          <p className="tcp-subtle max-w-2xl text-base">
            {requireHostedLogin
              ? "Use your Tandem hosted account to access this managed server."
              : "Enter your engine token to access chat, task board, automations, runs, memory, and settings."}
          </p>
        </section>

        <PanelCard
          title={controlPanelName}
          subtitle={
            requireHostedLogin
              ? "Managed hosted server detected. Sign in through Tandem to continue."
              : hostedManaged
                ? "Managed local test server detected. Authenticate against your Tandem engine to continue."
                : controlPanelMode === "aca"
                  ? "ACA install detected. Authenticate against your Tandem engine to continue."
                  : "Standalone install detected. Authenticate against your Tandem engine to continue."
          }
        >
          {requireHostedLogin ? (
            <div className="grid gap-3">
              <p className="tcp-subtle text-sm">
                Managed hosted panels do not accept the engine token in the browser. Tandem will
                verify your hosted organization membership and return you here.
              </p>
              <button
                type="button"
                className="tcp-btn-primary w-full"
                disabled={!hostedLoginUrl}
                onClick={() => {
                  if (hostedLoginUrl) window.location.assign(hostedLoginUrl);
                }}
              >
                <Icon name="log-in" />
                Sign in with Tandem
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
                <Icon name="activity" />
                Check engine
              </button>
              <div className={`min-h-[1.2rem] text-sm ${ok ? "text-lime-300" : "text-rose-300"}`}>
                {message}
              </div>
            </div>
          ) : (
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
                  <Icon name="key-round" />
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
                  <Icon name="activity" />
                  Check engine
                </button>
              </div>

              <div className={`min-h-[1.2rem] text-sm ${ok ? "text-lime-300" : "text-rose-300"}`}>
                {loginMutation.error?.message || message}
              </div>

              {hostedManaged ? (
                <div className="rounded-xl border border-amber-500/25 bg-amber-950/20 p-3 text-xs text-amber-100">
                  Hosted Tandem login is not connected for this deployment, so this panel is using
                  the engine-token flow for local testing.
                </div>
              ) : null}

              <div className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
                <div className="mb-2 flex items-center justify-between gap-3">
                  <div className="font-medium">Readiness</div>
                  <StatusPulse
                    tone={ok ? "ok" : "warn"}
                    text={ok ? "Engine reachable" : "Waiting"}
                  />
                </div>
                <p className="tcp-subtle text-xs">
                  Connectivity checks are non-destructive and help verify the local panel can reach
                  the engine before authentication.
                </p>
                {controlPanelModeReason ? (
                  <p className="tcp-subtle mt-2 text-xs">{controlPanelModeReason}</p>
                ) : null}
              </div>
            </form>
          )}
        </PanelCard>
      </div>
    </main>
  );
}
