import { motion } from "motion/react";
import { useEffect, useId, useState } from "react";
import { prefersReducedMotion } from "../app/themes.js";

function useReducedMotionPreference() {
  const [reduced, setReduced] = useState(() => prefersReducedMotion());

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => setReduced(media.matches);
    onChange();
    if (typeof media.addEventListener === "function") {
      media.addEventListener("change", onChange);
      return () => media.removeEventListener("change", onChange);
    }
    media.addListener(onChange);
    return () => media.removeListener(onChange);
  }, []);

  return reduced;
}

const QUADRANTS = [
  {
    key: "topLeft",
    d: "M 165.7787,0.020459 C 110.5808,0.078505 55.382896,0.1357013 0.18497613,0.1942872 c -0.13808,69.0470108 -0.28197,138.0943328 -0.0977,207.1414028 0.11052,70.36172 0.22237,140.72344 0.33206,211.08516 C 77.700496,418.71333 154.9818,419.01452 232.263,419.26154 c 9.5066,-0.0925 19.113,-1.11955 28.5387,0.59284 4.6532,1.08536 8.9565,3.23566 13.4536,4.79499 6.593,2.55589 12.7266,6.10955 18.9726,9.38867 13.4755,-16.05439 26.9519,-32.10807 40.4278,-48.16211 -12.1424,-6.81702 -24.1012,-14.20169 -37.2648,-18.93966 -13.5687,-4.91142 -27.9081,-7.38898 -42.2708,-8.40335 -11.0216,-0.79383 -22.0809,-0.70151 -33.1242,-0.72592 -10.7282,0.0403 -21.4565,0.0756 -32.1848,0.0358 -42.5681,6.3e-4 -85.1362,10e-4 -127.704264,0.002 0,-99.46224 0,-198.92448 0,-298.386721 91.275364,0 182.550764,0 273.826164,0 -0.014,56.512711 0.04,113.025721 -0.1026,169.538201 -0.4117,20.19995 -1.8163,40.48931 0.4545,60.63079 1.6786,15.01655 5.5662,29.97844 13.0059,43.21631 7.5387,13.60301 18.1131,25.18796 29.0127,36.15897 23.8256,24.12545 48.9172,46.9543 73.9775,69.78218 6.9487,5.87852 13.8953,11.75951 20.8454,17.63644 13.0693,-12.95418 26.1394,-25.90752 39.2109,-38.85937 -28.7582,-27.3941 -57.5601,-54.76749 -85.5909,-82.89143 -5.5883,-5.64711 -11.1906,-11.30599 -16.1884,-17.49334 -0.5249,-0.66821 -1.2376,-1.54691 -1.8149,-2.3185 -5.2184,-6.75403 -9.5006,-14.31908 -11.8959,-22.54362 -2.2613,-7.64006 -2.7179,-15.6557 -3.0509,-23.57147 -0.5245,-16.31096 0.6385,-32.74612 0.7322,-49.10694 0.046,-10.58259 -0.014,-21.16542 0.011,-31.74814 0,-62.47018 0.01,-124.940367 0.01,-187.4105497 -40.8839,-0.537121 -81.7722,-0.4363093 -122.6586,-0.4765625 -35.037,-0.00429 -70.074,0.00506 -105.111,0.019531 z",
    offsetX: -18,
    offsetY: -18,
    packetA: {
      cx: [154, 208, 272, 338, 392],
      cy: [154, 208, 272, 338, 392],
    },
    packetB: {
      cx: [270, 322, 364, 410, 446],
      cy: [270, 322, 364, 410, 446],
    },
  },
  {
    key: "topRight",
    d: "M 738.18805,0.00904887 C 652.65224,0.14687727 567.11644,0.28620927 481.58063,0.42311147 481.29355,77.736195 480.97641,155.04938 480.73994,232.36252 c 0.0951,9.50059 1.12257,19.10363 -0.61083,28.52103 -1.08724,4.6222 -3.22866,8.89525 -4.77671,13.3659 -2.11394,5.50021 -4.95558,10.65543 -7.73606,15.88318 -0.5514,1.03279 -1.10342,2.06526 -1.6529,3.09908 16.05439,13.47549 32.10807,26.95183 48.16211,40.42773 6.80088,-12.1143 14.16648,-24.04427 18.90635,-37.17241 4.70755,-12.97452 7.19327,-26.66148 8.28795,-40.39257 0.96234,-11.87369 0.85374,-23.79791 0.87506,-35.70103 -0.0448,-10.76015 -0.0708,-21.52028 -0.0362,-32.28047 -6.3e-4,-42.33412 -0.001,-84.66823 -0.002,-127.002348 99.46224,0 198.92448,0 298.38672,0 0,91.275388 0,182.550778 0,273.826178 -56.54657,-0.0148 -113.09344,0.0431 -169.63977,-0.10651 -20.19988,-0.41744 -40.49026,-1.8122 -60.63074,0.46855 -14.76186,1.67135 -29.47359,5.44353 -42.53126,12.68208 -13.67228,7.44177 -25.30466,17.9973 -36.29986,28.88593 -24.28534,23.96271 -47.26235,49.19759 -70.22483,74.41794 -5.87851,6.9487 -11.7595,13.89532 -17.63643,20.84537 12.95567,13.06842 25.9083,26.13986 38.86328,39.20898 27.66859,-29.06037 55.33459,-58.14591 83.75768,-86.45455 8.5617,-8.48715 17.33411,-17.0041 27.81511,-23.11301 6.25643,-3.65571 13.14158,-6.26131 20.29954,-7.44598 5.95822,-1.04868 11.924,-1.34198 17.974,-1.55197 16.53486,-0.49041 33.05458,0.76393 49.58797,0.75655 72.54855,0.0238 145.09719,0.0223 217.64578,0.0218 0.55496,-42.85079 0.44067,-85.70605 0.47573,-128.55937 0.007,-88.26801 -0.0861,-176.535983 -0.19253,-264.80391353 -53.87299,-0.1086191 -107.74602,-0.2217584 -161.61914,-0.1796876 z",
    offsetX: 18,
    offsetY: -18,
    packetA: {
      cx: [746, 692, 628, 562, 508],
      cy: [154, 208, 272, 338, 392],
    },
    packetB: {
      cx: [630, 578, 536, 490, 454],
      cy: [270, 322, 364, 410, 446],
    },
  },
  {
    key: "bottomLeft",
    d: "m 416.44677,389.82499 c -27.63909,29.04056 -55.28159,58.0743 -83.68816,86.33426 -8.1962,8.0499 -16.58633,16.12467 -26.56533,21.95472 -6.62254,3.91501 -13.97516,6.59579 -21.58594,7.75282 -6.08601,0.97701 -12.18199,1.21688 -18.32138,1.39901 -17.19693,0.40632 -34.37442,-0.94673 -51.57082,-0.79907 -66.02773,-0.0225 -132.055473,-0.003 -198.083206,-0.0124 -5.384635,-1.2e-4 -10.7692708,-3.5e-4 -16.15390616,-4e-4 -0.5424246,41.45067 -0.4419914,82.90578 -0.47756399963,124.35898 -0.008,89.66814 0.0887219996,179.33624 0.19436079963,269.0043 69.81380636,0.14113 139.62791536,0.27714 209.44178536,0.0951 69.59493,-0.10965 139.18985,-0.22064 208.78477,-0.32945 0.29326,-77.24849 0.59162,-154.49714 0.84172,-231.74566 -0.092,-9.38018 -1.07737,-18.846 0.4955,-28.1585 0.94174,-4.60968 3.10009,-8.83662 4.63349,-13.25795 0.843,-2.189 1.75327,-4.34941 2.73288,-6.40453 2.16593,-4.48341 4.58573,-8.83746 6.92337,-13.23219 -16.05732,-13.47848 -32.11466,-26.95695 -48.17188,-40.43555 -7.04213,12.52672 -14.6781,24.87418 -19.38736,38.52837 -5.66567,16.14988 -7.88257,33.29731 -8.44331,50.33996 -0.47979,17.62097 -0.0831,35.24985 -0.20252,52.87425 6.5e-4,43.60143 10e-4,87.20286 0.002,130.8043 -99.46224,0 -198.92448,0 -298.386714,0 0,-91.27539 0,-182.55079 0,-273.82618 56.512704,0.0137 113.025714,-0.0402 169.538204,0.10259 20.23434,0.41703 40.55908,1.817 60.73424,-0.4647 14.916,-1.68335 29.77681,-5.53584 42.94199,-12.90038 13.61582,-7.51192 25.2132,-18.07117 36.18649,-28.96459 24.17327,-23.87479 47.05648,-49.00616 69.92358,-74.12285 5.8785,-6.94807 11.7596,-13.89394 17.63643,-20.84342 -12.95561,-13.07175 -25.90997,-26.14473 -38.86328,-39.21876 -0.36979,0.38933 -0.73958,0.77865 -1.10938,1.16797 z",
    offsetX: -18,
    offsetY: 18,
    packetA: {
      cx: [154, 208, 272, 338, 392],
      cy: [746, 692, 628, 562, 508],
    },
    packetB: {
      cx: [270, 322, 364, 410, 446],
      cy: [630, 578, 536, 490, 454],
    },
  },
  {
    key: "bottomRight",
    d: "m 426.8571,444.59091 c -12.72985,12.61977 -25.46283,25.23638 -38.19336,37.85547 29.12691,27.73171 58.25373,55.43889 86.62972,83.92587 8.60302,8.72405 17.28324,17.63916 23.36358,28.37664 3.38707,5.99569 5.81075,12.551 6.99498,19.34006 0.10315,0.70198 0.28675,1.69505 0.39581,2.51939 0.71872,4.98285 0.98533,10.12075 1.1706,15.08218 0.49786,16.07496 -0.64911,32.13742 -0.72978,48.20992 -0.0746,12.71328 -0.005,25.42699 -0.0304,38.14047 -0.002,60.49453 -0.005,120.98906 -0.007,181.48359 42.91832,0.56029 85.84121,0.44262 128.76211,0.47547 88.20042,0.002 176.40082,-0.0852 264.60117,-0.19227 0.14305,-69.71328 0.28248,-139.42689 0.0946,-209.14023 -0.1095,-69.69544 -0.22033,-139.39088 -0.329,-209.08633 -77.28117,-0.29086 -154.56251,-0.59133 -231.84371,-0.83873 -9.52988,0.0858 -19.16335,1.13203 -28.60918,-0.6117 -4.23636,-0.97203 -8.15732,-2.91274 -12.24527,-4.33881 -5.77747,-2.14913 -11.22148,-5.06032 -16.63414,-7.98379 -1.15488,-0.6186 -2.3098,-1.23709 -3.46458,-1.85588 -13.47912,16.05798 -26.95826,32.11595 -40.4375,48.17383 12.19551,6.85207 24.21269,14.26933 37.44508,19.00784 12.8948,4.64093 26.48482,7.10173 40.12002,8.18796 11.765,0.95772 23.58036,0.84872 35.37474,0.87453 10.96718,-0.0435 21.93434,-0.0712 32.90157,-0.0372 42.23515,-6.2e-4 84.47031,-10e-4 126.70547,-0.002 -1e-5,99.46224 0,198.92448 0,298.38672 -91.2754,0 -182.55079,0 -273.82618,0 0.0161,-56.57923 -0.0465,-113.15884 0.11074,-169.73778 0.41464,-20.2322 1.81951,-40.55708 -0.49651,-60.7277 -1.71763,-15.19144 -5.73298,-30.3167 -13.38031,-43.64446 -7.65026,-13.55499 -18.30059,-25.08429 -29.21818,-36.05517 -23.64522,-23.89553 -48.51342,-46.53253 -73.36373,-69.15939 -6.9487,-5.87852 -13.89532,-11.7595 -20.84537,-17.63644 -0.33854,0.33594 -0.67708,0.67188 -1.01562,1.00782 z",
    offsetX: 18,
    offsetY: 18,
    packetA: {
      cx: [746, 692, 628, 562, 508],
      cy: [746, 692, 628, 562, 508],
    },
    packetB: {
      cx: [630, 578, 536, 490, 454],
      cy: [630, 578, 536, 490, 454],
    },
  },
] as const;

export function TandemLogoAnimation({
  className = "",
  mode = "panel",
  title,
}: {
  className?: string;
  mode?: "compact" | "panel";
  title?: string;
}) {
  const reducedMotion = useReducedMotionPreference();
  const id = useId().replace(/:/g, "");
  const compact = mode === "compact";
  const baseFill = compact
    ? "var(--tcp-logo-compact-base, color-mix(in srgb, white 86%, var(--color-surface-elevated) 14%))"
    : "color-mix(in srgb, white 78%, var(--color-surface-elevated) 22%)";
  const glowFilter = compact
    ? "var(--tcp-logo-compact-glow, drop-shadow(0 0 10px rgba(245, 158, 11, 0.22)))"
    : "drop-shadow(0 0 18px rgba(245, 158, 11, 0.34))";
  const diagonalA = ["topLeft", "bottomRight"];
  const diagonalB = ["topRight", "bottomLeft"];
  const compactLogoTransform = "translate(52 52) scale(0.885)";
  const viewBox = compact ? "-48 -48 996 996" : "0 0 900 900";

  return (
    <div
      className={`relative isolate ${compact ? "flex items-center justify-center overflow-visible" : "overflow-hidden border border-white/6 bg-black/30"} ${className}`.trim()}
      title={title}
      aria-hidden="true"
    >
      {!compact ? (
        <div className="pointer-events-none absolute inset-0 opacity-70">
          <div className="absolute left-[6%] top-[8%] h-[34%] w-[34%] bg-[radial-gradient(circle_at_center,rgba(245,158,11,0.24),transparent_68%)]" />
          <div className="absolute bottom-[8%] right-[6%] h-[34%] w-[34%] bg-[radial-gradient(circle_at_center,rgba(148,163,184,0.16),transparent_72%)]" />
          <div className="absolute inset-0 bg-[linear-gradient(135deg,rgba(255,255,255,0.04),transparent_36%,transparent_64%,rgba(245,158,11,0.08))]" />
        </div>
      ) : null}

      <motion.svg
        viewBox={viewBox}
        className="relative z-10 h-full w-full"
        animate={
          reducedMotion || !compact
            ? undefined
            : {
                rotate: [0, 0, 360, 360],
              }
        }
        transition={
          reducedMotion || !compact
            ? undefined
            : {
                duration: 2.8,
                ease: "easeInOut",
                repeat: Infinity,
                times: [0, 0.24, 0.7, 1],
              }
        }
        style={{
          transformOrigin: "50% 50%",
        }}
      >
        <defs>
          <radialGradient id={`${id}-core`} cx="50%" cy="50%" r="50%">
            <stop
              offset="0%"
              stopColor={
                compact
                  ? "var(--tcp-logo-compact-core, rgba(251,191,36,0.98))"
                  : "rgba(251,191,36,0.98)"
              }
            />
            <stop
              offset="55%"
              stopColor={
                compact
                  ? "var(--tcp-logo-compact-core-mid, rgba(245,158,11,0.62))"
                  : "rgba(245,158,11,0.62)"
              }
            />
            <stop
              offset="100%"
              stopColor={
                compact
                  ? "var(--tcp-logo-compact-core-clear, rgba(245,158,11,0))"
                  : "rgba(245,158,11,0)"
              }
            />
          </radialGradient>
        </defs>

        {!compact ? (
          <rect
            x="0"
            y="0"
            width="900"
            height="900"
            style={{
              fill: "color-mix(in srgb, var(--color-surface-elevated) 78%, #000 22%)",
            }}
          />
        ) : null}

        <g transform={compact ? compactLogoTransform : undefined}>
          {QUADRANTS.map((quadrant) => (
            <path
              key={`base-${quadrant.key}`}
              className="tcp-logo-base"
              d={quadrant.d}
              style={{
                fill: baseFill,
                opacity: compact ? 0.92 : 0.82,
              }}
            />
          ))}

          <motion.g
            style={{
              transformBox: "fill-box",
              transformOrigin: "50% 50%",
              filter: glowFilter,
            }}
            animate={
              reducedMotion
                ? { opacity: 0.16, rotate: 0 }
                : {
                    opacity: compact ? [0.14, 0.32, 0.14, 0.14] : [0.12, 0.34, 0.12],
                    rotate: compact ? [0, 0, 16, 0] : [-8, 8, -8],
                  }
            }
            transition={
              reducedMotion
                ? undefined
                : compact
                  ? { duration: 2.8, ease: "easeInOut", repeat: Infinity, times: [0, 0.24, 0.7, 1] }
                  : { duration: 2.4, ease: "easeInOut", repeat: Infinity }
            }
          >
            {QUADRANTS.filter((quadrant) => diagonalA.includes(quadrant.key)).map((quadrant) => (
              <path
                key={`spin-a-${quadrant.key}`}
                d={quadrant.d}
                fill={
                  compact
                    ? "var(--tcp-logo-compact-accent-a, rgba(245, 158, 11, 0.9))"
                    : "rgba(245, 158, 11, 0.9)"
                }
              />
            ))}
          </motion.g>

          <motion.g
            style={{
              transformBox: "fill-box",
              transformOrigin: "50% 50%",
              filter: compact
                ? "var(--tcp-logo-compact-highlight-glow, drop-shadow(0 0 9px rgba(255, 230, 170, 0.18)))"
                : "drop-shadow(0 0 14px rgba(255, 230, 170, 0.22))",
            }}
            animate={
              reducedMotion
                ? { opacity: 0.14, rotate: 0 }
                : {
                    opacity: compact ? [0.1, 0.24, 0.1, 0.1] : [0.08, 0.24, 0.08],
                    rotate: compact ? [0, 0, -16, 0] : [8, -8, 8],
                  }
            }
            transition={
              reducedMotion
                ? undefined
                : compact
                  ? {
                      duration: 2.8,
                      ease: "easeInOut",
                      repeat: Infinity,
                      times: [0, 0.24, 0.7, 1],
                      delay: 0.08,
                    }
                  : { duration: 2.4, ease: "easeInOut", repeat: Infinity, delay: 0.18 }
            }
          >
            {QUADRANTS.filter((quadrant) => diagonalB.includes(quadrant.key)).map((quadrant) => (
              <path
                key={`spin-b-${quadrant.key}`}
                d={quadrant.d}
                fill={
                  compact
                    ? "var(--tcp-logo-compact-accent-b, rgba(255, 243, 199, 0.82))"
                    : "rgba(255, 243, 199, 0.82)"
                }
              />
            ))}
          </motion.g>

          <motion.circle
            cx="450"
            cy="450"
            r={compact ? 42 : 72}
            fill={`url(#${id}-core)`}
            animate={
              reducedMotion
                ? { opacity: 0.24 }
                : compact
                  ? { opacity: [0.16, 0.16, 0.42, 0.16], scale: [0.98, 0.98, 1.06, 0.98] }
                  : { opacity: [0.32, 0.68, 0.32], scale: [0.92, 1.06, 0.92] }
            }
            transition={
              reducedMotion
                ? undefined
                : compact
                  ? { duration: 2.8, ease: "easeInOut", repeat: Infinity, times: [0, 0.24, 0.7, 1] }
                  : { duration: 2.4, ease: "easeInOut", repeat: Infinity }
            }
          />
          <motion.rect
            x={compact ? "420" : "410"}
            y={compact ? "420" : "410"}
            width={compact ? "60" : "80"}
            height={compact ? "60" : "80"}
            rx={compact ? "6" : "8"}
            style={{
              fill: compact
                ? "var(--tcp-logo-compact-center, rgba(255, 243, 199, 0.76))"
                : "rgba(255, 243, 199, 0.9)",
              filter: compact
                ? "var(--tcp-logo-compact-center-glow, drop-shadow(0 0 10px rgba(245, 158, 11, 0.22)))"
                : "drop-shadow(0 0 16px rgba(245, 158, 11, 0.42))",
              transformBox: "fill-box",
              transformOrigin: "50% 50%",
            }}
            animate={
              reducedMotion
                ? { rotate: 45 }
                : compact
                  ? { rotate: [45, 45, 405, 405], scale: [0.96, 0.96, 1.04, 0.96] }
                  : { rotate: [45, 135, 225, 315, 405], scale: [0.92, 1, 0.92] }
            }
            transition={
              reducedMotion
                ? undefined
                : compact
                  ? { duration: 2.8, ease: "easeInOut", repeat: Infinity, times: [0, 0.24, 0.7, 1] }
                  : { duration: 6.5, ease: "linear", repeat: Infinity }
            }
          />
        </g>
        {!compact ? (
          <>
            <motion.circle
              cx="450"
              cy="450"
              r="32"
              fill="none"
              stroke="rgba(255, 231, 179, 0.58)"
              strokeWidth="3"
              animate={
                reducedMotion ? { opacity: 0.28 } : { r: [26, 44, 64], opacity: [0.48, 0.24, 0] }
              }
              transition={
                reducedMotion ? undefined : { duration: 2.2, ease: "easeOut", repeat: Infinity }
              }
            />
            <motion.circle
              cx="450"
              cy="450"
              r="22"
              fill="none"
              stroke="rgba(148, 163, 184, 0.42)"
              strokeWidth="2"
              animate={
                reducedMotion ? { opacity: 0.24 } : { r: [18, 34, 50], opacity: [0.22, 0.12, 0] }
              }
              transition={
                reducedMotion
                  ? undefined
                  : { duration: 2.2, ease: "easeOut", repeat: Infinity, delay: 0.62 }
              }
            />
          </>
        ) : null}

        {!compact ? (
          <g>
            {[0, 1, 2, 3].map((index) => (
              <motion.rect
                key={`telemetry-${index}`}
                x={92 + index * 178}
                y="804"
                width="128"
                height="10"
                style={{
                  fill: "rgba(255, 240, 200, 0.78)",
                  filter: "drop-shadow(0 0 12px rgba(245, 158, 11, 0.24))",
                  transformBox: "fill-box",
                  transformOrigin: "left center",
                }}
                animate={
                  reducedMotion
                    ? { opacity: 0.32, scaleX: 0.62 }
                    : { opacity: [0.22, 0.92, 0.32], scaleX: [0.36, 1, 0.42] }
                }
                transition={{
                  duration: 1.6,
                  ease: "easeInOut",
                  repeat: Infinity,
                  delay: index * 0.16,
                }}
              />
            ))}
          </g>
        ) : null}
      </motion.svg>
    </div>
  );
}
