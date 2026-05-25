import { useTranslation } from "react-i18next";
import { Globe, Check } from "lucide-react";
import { motion } from "framer-motion";
import { normalizeLanguage, persistLanguagePreference } from "@/i18n/languageSync";

const LANGUAGES = [
  { code: "en", name: "English", nativeName: "English" },
  { code: "zh-CN", name: "Chinese (Simplified)", nativeName: "简体中文" },
];

export function LanguageSettings() {
  const { t, i18n } = useTranslation("settings");
  const currentLanguage = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language);

  const handleLanguageChange = async (languageCode: string) => {
    await persistLanguagePreference(languageCode);
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-text terminal-text mb-2">{t("language.title")}</h2>
        <p className="text-text-muted">{t("language.description")}</p>
      </div>

      <div className="glass border-glass p-6 space-y-4">
        <div className="flex items-center gap-3 mb-4">
          <Globe className="h-5 w-5 text-primary" />
          <span className="text-sm font-medium text-text">
            {t("language.current")}: {LANGUAGES.find((l) => l.code === currentLanguage)?.nativeName}
          </span>
        </div>

        <div className="space-y-2">
          {LANGUAGES.map((language) => (
            <motion.button
              key={language.code}
              onClick={() => handleLanguageChange(language.code)}
              className={`w-full flex items-center justify-between p-4 rounded-lg border transition-all ${
                currentLanguage === language.code
                  ? "border-primary bg-primary/10"
                  : "border-glass hover:border-primary/50"
              }`}
              whileHover={{ scale: 1.01 }}
              whileTap={{ scale: 0.99 }}
            >
              <div className="flex flex-col items-start">
                <span className="font-medium text-text">{language.nativeName}</span>
                <span className="text-sm text-text-muted">{language.name}</span>
              </div>
              {currentLanguage === language.code && <Check className="h-5 w-5 text-primary" />}
            </motion.button>
          ))}
        </div>
      </div>
    </div>
  );
}
