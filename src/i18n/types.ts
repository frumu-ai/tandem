import "react-i18next";
import type enCommon from "./locales/en/common.json";
import type enChat from "./locales/en/chat.json";
import type enSettings from "./locales/en/settings.json";
import type enSkills from "./locales/en/skills.json";
import type enErrors from "./locales/en/errors.json";

declare module "react-i18next" {
  interface CustomTypeOptions {
    defaultNS: "common";
    resources: {
      common: typeof enCommon;
      chat: typeof enChat;
      settings: typeof enSettings;
      skills: typeof enSkills;
      errors: typeof enErrors;
    };
  }
}
