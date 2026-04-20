import assert from "node:assert/strict";
import test from "node:test";

import { deriveProviderState } from "../src/app/providerStatus.ts";

test("deriveProviderState accepts snake_case default_model entries", () => {
  const state = deriveProviderState(
    {
      default: "openrouter",
      providers: {
        openrouter: {
          default_model: "openai/gpt-5.4",
        },
      },
    },
    { connected: ["openrouter"] },
    { openrouter: { has_key: true } }
  );

  assert.equal(state.defaultProvider, "openrouter");
  assert.equal(state.defaultModel, "openai/gpt-5.4");
  assert.equal(state.ready, true);
  assert.equal(state.needsOnboarding, false);
});

test("deriveProviderState still supports camelCase defaultModel entries", () => {
  const state = deriveProviderState(
    {
      default: "openai",
      providers: {
        openai: {
          defaultModel: "gpt-5.2",
        },
      },
    },
    { connected: ["openai"] },
    { providers: { openai: { hasKey: true } } }
  );

  assert.equal(state.defaultProvider, "openai");
  assert.equal(state.defaultModel, "gpt-5.2");
  assert.equal(state.ready, true);
});

test("deriveProviderState treats connected oauth providers as ready", () => {
  const state = deriveProviderState(
    {
      default: "openai-codex",
      providers: {
        "openai-codex": {
          default_model: "gpt-5.4",
        },
      },
    },
    { connected: ["openai-codex"] },
    {
      providers: {
        "openai-codex": {
          auth_kind: "oauth",
          status: "connected",
          connected: true,
          has_key: true,
        },
      },
    }
  );

  assert.equal(state.defaultProvider, "openai-codex");
  assert.equal(state.defaultModel, "gpt-5.4");
  assert.equal(state.ready, true);
  assert.equal(state.needsOnboarding, false);
  assert.equal(state.defaultProviderAuthKind, "oauth");
  assert.equal(state.defaultProviderManagedBy, "");
});

test("deriveProviderState surfaces hosted Codex auth source details", () => {
  const state = deriveProviderState(
    {
      default: "openai-codex",
      providers: {
        "openai-codex": {
          default_model: "gpt-5.4",
        },
      },
    },
    { connected: ["openai-codex"] },
    {
      providers: {
        "openai-codex": {
          auth_kind: "oauth",
          status: "connected",
          connected: true,
          managed_by: "codex-upload",
          source: "oauth",
        },
      },
    }
  );

  assert.equal(state.ready, true);
  assert.equal(state.defaultProviderAuthKind, "oauth");
  assert.equal(state.defaultProviderSource, "oauth");
  assert.equal(state.defaultProviderManagedBy, "codex-upload");
});
