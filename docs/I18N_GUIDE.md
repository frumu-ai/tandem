# Internationalization (i18n) Guide

Tandem uses [react-i18next](https://react.i18next.com/) for internationalization support.

## Supported Languages

- English (en) - Default
- Simplified Chinese (zh-CN)

## Changing Language

Users can change the language in **Settings → Language**.

## For Developers

### Using Translations in Components

```tsx
import { useTranslation } from 'react-i18next';

function MyComponent() {
  const { t } = useTranslation('common');
  
  return <div>{t('welcome')}</div>;
}
```

### Available Namespaces

- `common` - Common UI elements (buttons, labels, etc.)
- `chat` - Chat interface
- `settings` - Settings page
- `skills` - Skills and workflows
- `errors` - Error messages

### Translation Files

Translation files are located in `src/i18n/locales/`:

```
src/i18n/locales/
├── en/
│   ├── common.json
│   ├── chat.json
│   ├── settings.json
│   ├── skills.json
│   └── errors.json
└── zh-CN/
    ├── common.json
    ├── chat.json
    ├── settings.json
    ├── skills.json
    └── errors.json
```

### Adding New Translations

1. Add the key to the appropriate namespace file in `en/`
2. Add the corresponding translation in `zh-CN/`
3. Use the translation in your component with `t('key')`

Example:

```json
// src/i18n/locales/en/common.json
{
  "myNewKey": "Hello World"
}

// src/i18n/locales/zh-CN/common.json
{
  "myNewKey": "你好世界"
}
```

```tsx
// In your component
const { t } = useTranslation('common');
<div>{t('myNewKey')}</div>
```

### Adding a New Language

1. Create a new directory in `src/i18n/locales/` (e.g., `fr/`)
2. Copy all JSON files from `en/` and translate them
3. Add the language to `src/i18n/index.ts`:

```typescript
const resources = {
  en: { /* ... */ },
  'zh-CN': { /* ... */ },
  fr: {
    common: frCommon,
    chat: frChat,
    // ...
  }
};
```

4. Add the language option to `src/components/settings/LanguageSettings.tsx`

### Backend Storage

Language preference is stored in Tauri's persistent store (`store.json`) and synced between frontend and backend using:

- `get_language_setting()` - Get current language
- `set_language_setting(language)` - Save language preference

## Testing

To test translations:

1. Run the app: `npm run tauri dev`
2. Go to Settings → Language
3. Switch between English and Simplified Chinese
4. Verify that UI elements update correctly

## Best Practices

- Keep translation keys descriptive and organized by feature
- Use namespaces to group related translations
- Avoid hardcoded strings in components
- Test all languages before committing changes
- Keep translation files in sync across all languages
