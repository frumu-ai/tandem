import { useMemo } from "react";
import ReactDiffViewer from "react-diff-viewer-continued";

interface DiffViewerProps {
  oldValue: string;
  newValue: string;
  oldTitle?: string;
  newTitle?: string;
  splitView?: boolean;
}

export function DiffViewer({
  oldValue,
  newValue,
  oldTitle = "Before",
  newTitle = "After",
  splitView = true,
}: DiffViewerProps) {
  // Create custom styles to match Tandem's theme
  const styles = useMemo(
    () => ({
      variables: {
        dark: {
          diffViewerBackground: "hsl(var(--surface))",
          diffViewerColor: "hsl(var(--text))",
          addedBackground: "hsl(142 76% 20% / 0.2)",
          addedColor: "hsl(142 76% 60%)",
          removedBackground: "hsl(0 84% 30% / 0.2)",
          removedColor: "hsl(0 84% 60%)",
          wordAddedBackground: "hsl(142 76% 25% / 0.4)",
          wordRemovedBackground: "hsl(0 84% 35% / 0.4)",
          addedGutterBackground: "hsl(142 76% 15% / 0.3)",
          removedGutterBackground: "hsl(0 84% 20% / 0.3)",
          gutterBackground: "hsl(var(--surface-elevated))",
          gutterBackgroundDark: "hsl(var(--surface))",
          highlightBackground: "hsl(var(--primary) / 0.1)",
          highlightGutterBackground: "hsl(var(--primary) / 0.2)",
          codeFoldGutterBackground: "hsl(var(--surface-elevated))",
          codeFoldBackground: "hsl(var(--surface))",
          emptyLineBackground: "transparent",
          gutterColor: "hsl(var(--text-subtle))",
          addedGutterColor: "hsl(142 76% 50%)",
          removedGutterColor: "hsl(0 84% 50%)",
          codeFoldContentColor: "hsl(var(--text-muted))",
          diffViewerTitleBackground: "hsl(var(--surface-elevated))",
          diffViewerTitleColor: "hsl(var(--text))",
          diffViewerTitleBorderColor: "hsl(var(--border))",
        },
      },
      line: {
        padding: "8px 2px",
        fontSize: "13px",
        fontFamily: "ui-monospace, monospace",
      },
      gutter: {
        padding: "8px",
        minWidth: "50px",
        fontSize: "12px",
      },
      marker: {
        padding: "8px",
      },
      diffContainer: {
        overflowX: "auto",
      },
      contentText: {
        lineHeight: "1.5",
      },
      titleBlock: {
        padding: "12px 16px",
        fontWeight: 600,
        fontSize: "14px",
      },
    }),
    []
  );

  return (
    <div className="rounded-lg overflow-hidden border border-border">
      <ReactDiffViewer
        oldValue={oldValue}
        newValue={newValue}
        leftTitle={oldTitle}
        rightTitle={newTitle}
        splitView={splitView}
        useDarkTheme={true}
        styles={styles}
        showDiffOnly={false}
        compareMethod={"diffWords" as any}
      />
    </div>
  );
}
