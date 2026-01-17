import { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";

interface MatrixLoaderProps {
  isLoading: boolean;
  onLoadingComplete?: () => void;
}

// Matrix-style characters
const MATRIX_CHARS = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

interface Drop {
  x: number;
  y: number;
  speed: number;
  chars: string[];
  opacity: number;
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

    const fontSize = 14;
    const columns = Math.floor(width / fontSize);

    // Initialize drops
    dropsRef.current = Array.from({ length: columns }, (_, i) => ({
      x: i * fontSize,
      y: Math.random() * height * -1,
      speed: 2 + Math.random() * 4,
      chars: Array.from({ length: 20 }, () => 
        MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)]
      ),
      opacity: 0.5 + Math.random() * 0.5,
    }));

    const draw = () => {
      // Semi-transparent black to create trail effect
      ctx.fillStyle = "rgba(0, 0, 0, 0.05)";
      ctx.fillRect(0, 0, width, height);

      ctx.font = `${fontSize}px monospace`;

      dropsRef.current.forEach((drop) => {
        // Draw the trail of characters
        drop.chars.forEach((char, i) => {
          const y = drop.y - i * fontSize;
          if (y > 0 && y < height) {
            // Head of the drop is brighter
            if (i === 0) {
              ctx.fillStyle = "#fff";
            } else {
              const alpha = Math.max(0, (1 - i / drop.chars.length) * drop.opacity);
              ctx.fillStyle = `rgba(0, 255, 136, ${alpha})`;
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
          drop.chars = Array.from({ length: 20 }, () =>
            MATRIX_CHARS[Math.floor(Math.random() * MATRIX_CHARS.length)]
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

  return (
    <canvas
      ref={canvasRef}
      width={width}
      height={height}
      className="absolute inset-0"
    />
  );
}

export function MatrixLoader({ isLoading, onLoadingComplete }: MatrixLoaderProps) {
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 });
  const [loadingText, setLoadingText] = useState("Initializing secure vault");
  const [dots, setDots] = useState("");

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
          className="fixed inset-0 z-50 flex items-center justify-center bg-black"
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
                    color: "#00ff88",
                    textShadow: "0 0 20px rgba(0, 255, 136, 0.5), 0 0 40px rgba(0, 255, 136, 0.3)",
                  }}
                >
                  TANDEM
                </motion.h1>
                <motion.div
                  className="absolute -inset-4 rounded-lg"
                  style={{
                    background: "linear-gradient(90deg, transparent, rgba(0, 255, 136, 0.1), transparent)",
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
              <p className="text-sm tracking-[0.3em] text-emerald-400/60 uppercase">
                AI Workspace
              </p>
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
                <motion.div
                  className="absolute inset-0 rounded-full border-2 border-emerald-500/20"
                />
                <motion.div
                  className="absolute inset-0 rounded-full border-2 border-transparent border-t-emerald-400"
                  animate={{ rotate: 360 }}
                  transition={{ duration: 1, repeat: Infinity, ease: "linear" }}
                />
                <motion.div
                  className="absolute inset-2 rounded-full border-2 border-transparent border-b-emerald-300"
                  animate={{ rotate: -360 }}
                  transition={{ duration: 1.5, repeat: Infinity, ease: "linear" }}
                />
              </div>

              {/* Loading text */}
              <div className="flex items-center gap-1 font-mono text-sm text-emerald-400">
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
                  className="h-1 w-8 rounded-full bg-emerald-500/30"
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
          <div className="absolute left-4 top-4 font-mono text-xs text-emerald-500/40">
            <div>SYS.INIT</div>
            <div>v0.1.0</div>
          </div>
          <div className="absolute right-4 top-4 font-mono text-xs text-emerald-500/40 text-right">
            <div>SECURE</div>
            <div>MODE</div>
          </div>
          <div className="absolute bottom-4 left-4 font-mono text-xs text-emerald-500/40">
            <motion.div
              animate={{ opacity: [0.4, 1, 0.4] }}
              transition={{ duration: 2, repeat: Infinity }}
            >
              ● ZERO-TRUST ACTIVE
            </motion.div>
          </div>
          <div className="absolute bottom-4 right-4 font-mono text-xs text-emerald-500/40">
            LOCAL-FIRST
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
