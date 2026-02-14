import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

// Import translation files
import enCommon from "./locales/en/common.json";
import enChat from "./locales/en/chat.json";
import enSettings from "./locales/en/settings.json";
import enSkills from "./locales/en/skills.json";
import enErrors from "./locales/en/errors.json";

import zhCommon from "./locales/zh-CN/common.json";
import zhChat from "./locales/zh-CN/chat.json";
import zhSettings from "./locales/zh-CN/settings.json";
import zhSkills from "./locales/zh-CN/skills.json";
import zhErrors from "./locales/zh-CN/errors.json";

const resources = {
  en: {
    common: enCommon,
    chat: enChat,
    settings: enSettings,
    skills: enSkills,
    errors: enErrors,
  },
  "zh-CN": {
    common: zhCommon,
    chat: zhChat,
    settings: zhSettings,
    skills: zhSkills,
    errors: zhErrors,
  },
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: "en",
    defaultNS: "common",
    ns: ["common", "chat", "settings", "skills", "errors"],
    interpolation: {
      escapeValue: false, // React already escapes
    },
    detection: {
      order: ["localStorage", "navigator"],
      caches: ["localStorage"],
      lookupLocalStorage: "tandem.language",
    },
  });

export default i18n;
