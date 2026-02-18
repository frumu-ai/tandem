# Internationalization (i18n) Guide

Tandem uses [react-i18next](https://react.i18next.com/) for frontend localization.

## Supported Languages

- English (`en`) - default
- Simplified Chinese (`zh-CN`)

## Runtime Language Flow

Tandem now uses **frontend + backend sync** for language preference:

1. Frontend i18n initializes from `src/i18n/index.ts`
2. `bootstrapLanguagePreference()` runs before React render in `src/main.tsx`
3. Language is resolved from:
   - backend store (`get_language_setting`), then
   - localStorage (`tandem.language`), then
   - i18next detected language
4. Resolved language is written back to both:
   - i18next/localStorage
   - Tauri store (`set_language_setting`)

This ensures language survives restarts and keeps desktop/native state aligned.

## Where Language Is Changed

Users can change language in **Settings -> Language**.

The language selector calls `persistLanguagePreference(...)` from `src/i18n/languageSync.ts`.

## Namespaces

- `common`: app shell/navigation/shared actions
- `chat`: chat UI and empty states
- `settings`: settings screens
- `skills`: skills and packs workflows
- `errors`: generic error strings

## Translation Files

Located in:

- `src/i18n/locales/en/*.json`
- `src/i18n/locales/zh-CN/*.json`

## Required Quality Check

Run parity validation before committing locale changes:

```bash
pnpm i18n:parity
```

This check validates:

- same locale files in `en` and `zh-CN`
- same key paths in each file
- no empty translation values
- no locale shape/type mismatches

CI runs this automatically in `.github/workflows/ci.yml`.

## Developer Checklist For UI PRs

1. Avoid hardcoded user-facing strings in components.
2. Add keys to `en` locale first.
3. Add matching keys in `zh-CN` locale.
4. Use `useTranslation(...)` in component code.
5. Run `pnpm i18n:parity`.
6. Verify language switching in-app:
   - switch to Chinese
   - restart app
   - confirm language persists

## Adding a New Language

1. Create `src/i18n/locales/<lang>/` with all namespace files.
2. Add resources in `src/i18n/index.ts`.
3. Add language option in `LanguageSettings.tsx`.
4. Extend parity tooling if needed for multi-target checks.
5. Verify startup sync and persistence behavior.
