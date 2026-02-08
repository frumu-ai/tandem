import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { RefreshCw } from "lucide-react";
import { SkillsPanel } from "@/components/skills";
import { listSkills, startSidecar, stopSidecar, type SkillInfo } from "@/lib/tauri";

interface SkillsTabProps {
  workspacePath: string | null;
}

export function SkillsTab({ workspacePath }: SkillsTabProps) {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [restartingSidecar, setRestartingSidecar] = useState(false);

  const refresh = async () => {
    const skillsList = await listSkills();
    setSkills(skillsList);
  };

  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        setLoading(true);
        await refresh();
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
    // listSkills reads active workspace from backend state; re-load on workspace change.
  }, [workspacePath]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      </div>
    );
  }

  return (
    <>
      <SkillsPanel
        skills={skills}
        projectPath={workspacePath ?? undefined}
        onRefresh={refresh}
        onRestartSidecar={async () => {
          setRestartingSidecar(true);
          try {
            await stopSidecar();
            await new Promise((resolve) => setTimeout(resolve, 500));
            await startSidecar();
            await new Promise((resolve) => setTimeout(resolve, 1000));
          } finally {
            setRestartingSidecar(false);
          }
        }}
      />

      {/* Engine Restart Overlay */}
      <AnimatePresence>
        {restartingSidecar && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-50 flex items-center justify-center bg-background/95 backdrop-blur-sm"
          >
            <motion.div
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              className="flex flex-col items-center gap-6"
            >
              <div className="relative h-16 w-16">
                <motion.div
                  className="absolute inset-0 rounded-2xl bg-primary/10"
                  animate={{ scale: [1, 1.05, 1] }}
                  transition={{ duration: 1, repeat: Infinity }}
                />
                <div className="absolute inset-0 flex items-center justify-center">
                  <motion.div
                    animate={{ rotate: 360 }}
                    transition={{ duration: 2, repeat: Infinity, ease: "linear" }}
                  >
                    <RefreshCw className="h-8 w-8 text-primary" />
                  </motion.div>
                </div>
              </div>
              <div className="text-center">
                <h3 className="text-lg font-semibold text-text mb-1">Restarting AI Engine</h3>
                <p className="text-sm text-text-muted">Loading new skill...</p>
              </div>
              {/* Animated progress bars */}
              <div className="flex gap-1">
                {[0, 1, 2, 3, 4].map((i) => (
                  <motion.div
                    key={i}
                    className="h-1.5 w-6 rounded-full bg-primary/30"
                    animate={{
                      opacity: [0.3, 1, 0.3],
                      scaleX: [1, 1.2, 1],
                    }}
                    transition={{
                      duration: 1.5,
                      repeat: Infinity,
                      delay: i * 0.2,
                    }}
                  />
                ))}
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}
