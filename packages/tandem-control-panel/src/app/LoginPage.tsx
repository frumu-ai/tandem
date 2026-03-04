import { useState } from "react";
import type { UseMutationResult } from "@tanstack/react-query";

type LoginMutation = UseMutationResult<unknown, Error, { token: string; remember: boolean }>;

export function LoginPage({
  loginMutation,
  savedToken,
  onCheckEngine,
  controlPanelName,
}: {
  loginMutation: LoginMutation;
  savedToken: string;
  onCheckEngine: () => Promise<string>;
  controlPanelName: string;
}) {
  const [token, setToken] = useState(savedToken);
  const [remember, setRemember] = useState(true);
  const [message, setMessage] = useState("");
  const [ok, setOk] = useState(false);

  return (
    <main className="mx-auto grid min-h-screen w-full max-w-3xl place-items-center px-5 py-8">
      <section className="tcp-panel tcp-shell-glass w-full max-w-xl">
        <h1 className="mb-1 text-4xl font-semibold tracking-tight tcp-display">
          {controlPanelName}
        </h1>
        <p className="tcp-subtle mb-6">
          Use your engine API token to unlock the full web control center.
        </p>
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
          <label className="text-sm tcp-subtle">Engine Token</label>
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
          <button
            disabled={loginMutation.isPending}
            type="submit"
            className="tcp-btn-primary w-full"
          >
            Sign In
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
            Check Engine Connectivity
          </button>
          <div className={`min-h-[1.2rem] text-sm ${ok ? "text-lime-300" : "text-rose-300"}`}>
            {loginMutation.error?.message || message}
          </div>
        </form>
      </section>
    </main>
  );
}
