import { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";

interface MatrixLoaderProps {
  isLoading: boolean;
  onLoadingComplete?: () => void;
}

// Matrix-style characters
const MATRIX_CHARS =
  "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

interface Drop {
  x: number;
  y: number;
  speed: number;
  chars: string[];
  opacity: number;
}

function parseRgb(color: string): { r: number; g: number; b: number } | null {
  const c = color.trim();
  if (!c) return null;

  const rgbMatch = c.match(
    /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})(?:\s*,\s*[\d.]+\s*)?\)$/
  );
  if (rgbMatch) {
    return { r: Number(rgbMatch[1]), g: Number(rgbMatch[2]), b: Number(rgbMatch[3]) };
  }

  const hex = c.startsWith("#") ? c.slice(1) : c;
  if (hex.length === 3) {
    const r = Number.parseInt(hex[0] + hex[0], 16);
    const g = Number.parseInt(hex[1] + hex[1], 16);
    const b = Number.parseInt(hex[2] + hex[2], 16);
    return { r, g, b };
  }
  if (hex.length === 6) {
    const r = Number.parseInt(hex.slice(0, 2), 16);
    const g = Number.parseInt(hex.slice(2, 4), 16);
    const b = Number.parseInt(hex.slice(4, 6), 16);
    return { r, g, b };
  }

  return null;
}

function MatrixRain({ width, height }: { width: number; height: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dropsRef = useRef<Drop[]>([]);
  const animationRef = useRef<number | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const styles = getComputedStyle(document.documentElement);
    const accent = styles.getPropertyValue("--color-primary").trim() || "#00ff88";
    const bg = styles.getPropertyValue("--color-background").trim() || "#000000";
    const text = styles.getPropertyValue("--color-text").trim() || "#ffffff";

    const accentRgb = parseRgb(accent) ?? { r: 0, g: 255, b: 136 };
    const bgRgb = parseRgb(bg) ?? { r: 0, g: 0, b: 0 };
    const textRgb = parseRgb(text) ?? { r: 255, g: 255, b: 255 };

    const fontSize = 14;
    const columns = Math.floor(width / fontSize);

    // Initialize drops
    dropsRef.current = Array.from({ length: columns }, (_, i) => ({
      x: i * fontSize,
      y: Math.random() * height * -1,
      speed: 2 + Math.random() * 4,
      chars: Array.from(
        { length: 20 },
        () => MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)]
      ),
      opacity: 0.5 + Math.random() * 0.5,
    }));

    const draw = () => {
      // Semi-transparent background to create trail effect
      ctx.fillStyle = `rgba(${bgRgb.r}, ${bgRgb.g}, ${bgRgb.b}, 0.08)`;
      ctx.fillRect(0, 0, width, height);

      ctx.font = `${fontSize}px monospace`;

      dropsRef.current.forEach((drop) => {
        // Draw the trail of characters
        drop.chars.forEach((char, i) => {
          const y = drop.y - i * fontSize;
          if (y > 0 && y < height) {
            // Head of the drop is brighter
            if (i === 0) {
              ctx.fillStyle = `rgb(${textRgb.r}, ${textRgb.g}, ${textRgb.b})`;
            } else {
              const alpha = Math.max(0, (1 - i / drop.chars.length) * drop.opacity);
              ctx.fillStyle = `rgba(${accentRgb.r}, ${accentRgb.g}, ${accentRgb.b}, ${alpha})`;
            }
            ctx.fillText(char, drop.x, y);
          }
        });

        // Move drop down
        drop.y += drop.speed;

        // Reset drop when it goes off screen
        if (drop.y - drop.chars.length * fontSize > height) {
          drop.y = Math.random() * -100;
          drop.speed = 2 + Math.random() * 4;
          drop.chars = Array.from(
            { length: 20 },
            () => MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)]
          );
        }

        // Randomly change characters
        if (Math.random() > 0.95) {
          const idx = Math.floor(Math.random() * drop.chars.length);
          drop.chars[idx] = MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)];
        }
      });

      animationRef.current = requestAnimationFrame(draw);
    };

    draw();

    return () => {
      if (animationRef.current) {
        cancelAnimationFrame(animationRef.current);
      }
    };
  }, [width, height]);

  return <canvas ref={canvasRef} width={width} height={height} className="absolute inset-0" />;
}

export function MatrixLoader({ isLoading, onLoadingComplete }: MatrixLoaderProps) {
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 });
  const [loadingText, setLoadingText] = useState("Initializing secure vault");
  const [dots, setDots] = useState("");
  const [titleGlow, setTitleGlow] = useState<string | undefined>(undefined);

  useEffect(() => {
    const updateDimensions = () => {
      setDimensions({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    };

    updateDimensions();
    window.addEventListener("resize", updateDimensions);
    return () => window.removeEventListener("resize", updateDimensions);
  }, []);

  useEffect(() => {
    const styles = getComputedStyle(document.documentElement);
    const accent = styles.getPropertyValue("--color-primary").trim() || "#00ff88";
    const accentRgb = parseRgb(accent) ?? { r: 0, g: 255, b: 136 };
    // Avoid synchronous setState in effect body (eslint: react-hooks/set-state-in-effect)
    Promise.resolve().then(() =>
      setTitleGlow(
        `0 0 20px rgba(${accentRgb.r}, ${accentRgb.g}, ${accentRgb.b}, 0.5), 0 0 40px rgba(${accentRgb.r}, ${accentRgb.g}, ${accentRgb.b}, 0.3)`
      )
    );
  }, []);

  // Animate loading dots
  useEffect(() => {
    if (!isLoading) return;

    const interval = setInterval(() => {
      setDots((prev) => (prev.length >= 3 ? "" : prev + "."));
    }, 400);

    return () => clearInterval(interval);
  }, [isLoading]);

  // Cycle through loading messages
  useEffect(() => {
    if (!isLoading) return;

    const messages = [
      "Initializing secure vault",
      "Encrypting neural pathways",
      "Loading AI subsystems",
      "Establishing secure channels",
      "Preparing workspace",
      "Almost there",
    ];

    let index = 0;
    const interval = setInterval(() => {
      index = (index + 1) % messages.length;
      setLoadingText(messages[index]);
    }, 2500);

    return () => clearInterval(interval);
  }, [isLoading]);

  useEffect(() => {
    if (!isLoading && onLoadingComplete) {
      const timer = setTimeout(onLoadingComplete, 500);
      return () => clearTimeout(timer);
    }
  }, [isLoading, onLoadingComplete]);

  return (
    <AnimatePresence>
      {isLoading && (
        <motion.div
          initial={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-background"
        >
          {dimensions.width > 0 && (
            <MatrixRain width={dimensions.width} height={dimensions.height} />
          )}

          {/* Center content */}
          <div className="relative z-10 flex flex-col items-center gap-8">
            {/* Logo/Title */}
            <motion.div
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.2 }}
              className="flex flex-col items-center gap-4"
            >
              <div className="relative">
                <motion.h1
                  className="text-6xl font-bold tracking-wider"
                  style={{
                    color: "var(--color-primary)",
                    textShadow: titleGlow,
                  }}
                >
                  TANDEM
                </motion.h1>
                <motion.div
                  className="absolute -inset-4 rounded-lg"
                  style={{
                    background:
                      "linear-gradient(90deg, transparent, color-mix(in srgb, var(--color-primary) 18%, transparent), transparent)",
                  }}
                  animate={{
                    x: [-200, 200],
                  }}
                  transition={{
                    duration: 2,
                    repeat: Infinity,
                    ease: "linear",
                  }}
                />
              </div>
              <p className="text-sm tracking-[0.3em] text-emerald-400/60 uppercase">AI Workspace</p>
            </motion.div>

            {/* Loading indicator */}
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 0.5 }}
              className="flex flex-col items-center gap-4"
            >
              {/* Spinner */}
              <div className="relative h-16 w-16">
                <motion.div className="absolute inset-0 rounded-full border-2 border-primary/20" />
                <motion.div
                  className="absolute inset-0 rounded-full border-2 border-transparent border-t-primary"
                  animate={{ rotate: 360 }}
                  transition={{ duration: 1, repeat: Infinity, ease: "linear" }}
                />
                <motion.div
                  className="absolute inset-2 rounded-full border-2 border-transparent border-b-secondary"
                  animate={{ rotate: -360 }}
                  transition={{ duration: 1.5, repeat: Infinity, ease: "linear" }}
                />
              </div>

              {/* Loading text */}
              <div className="flex items-center gap-1 font-mono text-sm text-primary">
                <span>{loadingText}</span>
                <span className="w-6">{dots}</span>
              </div>
            </motion.div>

            {/* Decorative elements */}
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 0.8 }}
              className="mt-8 flex gap-2"
            >
              {[...Array(5)].map((_, i) => (
                <motion.div
                  key={i}
                  className="h-1 w-8 rounded-full bg-primary/30"
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
            </motion.div>
          </div>

          {/* Corner decorations */}
          <div className="absolute left-4 top-4 font-mono text-xs text-text-subtle">
            <div>SYS.INIT</div>
            <div>v0.1.0</div>
          </div>
          <div className="absolute right-4 top-4 font-mono text-xs text-text-subtle text-right">
            <div>SECURE</div>
            <div>MODE</div>
          </div>
          <div className="absolute bottom-4 left-4 font-mono text-xs text-text-subtle">
            <motion.div
              animate={{ opacity: [0.4, 1, 0.4] }}
              transition={{ duration: 2, repeat: Infinity }}
            >
              ● ZERO-TRUST ACTIVE
            </motion.div>
          </div>
          <div className="absolute bottom-4 right-4 font-mono text-xs text-text-subtle">
            LOCAL-FIRST
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
