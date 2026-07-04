import { useSettingsPageController } from "./SettingsPageController";
import { AnimatedPage, SplitView, StaggerGroup } from "../ui/index.tsx";
import { SettingsPageNavigationProvidersSections } from "./SettingsPageNavigationProvidersSections";
import { SettingsPageSearchIdentityThemeSections } from "./SettingsPageSearchIdentityThemeSections";
import { SettingsPageChannelsMcpSections } from "./SettingsPageChannelsMcpSections";
import { SettingsPageIncidentMonitorSections } from "./SettingsPageIncidentMonitorSections";
import { SettingsPageMaintenanceBrowserSections } from "./SettingsPageMaintenanceBrowserSections";
import { SettingsPageOverlays } from "./SettingsPageOverlays";
import type { AppPageProps } from "./pageTypes";
import { Icon } from "../ui/Icon";

export function SettingsPage(props: AppPageProps) {
  const controller = useSettingsPageController(props);
  const { rootRef, sectionTabs, activeSection, setActiveSection } = controller;
  const safeSectionTabs = Array.isArray(sectionTabs) ? sectionTabs : [];

  return (
    <AnimatedPage className="grid gap-4">
      <div ref={rootRef} className="grid gap-4">
        <div className="tcp-settings-tabs">
          {safeSectionTabs.map((section) => (
            <button
              key={section.id}
              type="button"
              className={`tcp-settings-tab tcp-settings-tab-underline ${
                activeSection === section.id ? "active" : ""
              }`}
              onClick={() => setActiveSection(section.id)}
            >
              <Icon name={section.icon} />
              {section.label}
            </button>
          ))}
        </div>

        <SplitView
          main={
            <StaggerGroup className="grid gap-4">
              <SettingsPageNavigationProvidersSections controller={controller} />
              <SettingsPageSearchIdentityThemeSections controller={controller} />
              <SettingsPageChannelsMcpSections controller={controller} />
              <SettingsPageIncidentMonitorSections controller={controller} />
              <SettingsPageMaintenanceBrowserSections controller={controller} />
            </StaggerGroup>
          }
        />

        <SettingsPageOverlays controller={controller} />
      </div>
    </AnimatedPage>
  );
}
