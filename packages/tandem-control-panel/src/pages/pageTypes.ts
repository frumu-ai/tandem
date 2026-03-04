import type { LegacyPageProps } from "./LegacyPage";

export type PageCommonProps = Omit<LegacyPageProps, "renderer" | "routeId">;

export type RoutablePageProps = PageCommonProps & {
  path?: string;
  default?: boolean;
};
