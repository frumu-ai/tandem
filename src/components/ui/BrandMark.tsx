import { cn } from "@/lib/utils";

type BrandMarkProps = {
  className?: string;
  size?: "sm" | "md" | "lg";
  title?: string;
};

const sizeClasses: Record<NonNullable<BrandMarkProps["size"]>, string> = {
  sm: "h-8 w-8 rounded-lg text-xl",
  md: "h-10 w-10 rounded-xl text-2xl",
  lg: "h-20 w-20 rounded-2xl text-5xl",
};

export function BrandMark({ className, size = "md", title = "Tandem" }: BrandMarkProps) {
  return (
    <div
      className={cn(
        "inline-flex items-center justify-center ring-1 ring-white/10",
        "bg-gradient-to-br from-primary/25 to-secondary/20",
        "shadow-[0_0_0_1px_rgba(255,255,255,0.03)]",
        sizeClasses[size],
        className
      )}
      aria-hidden={title ? undefined : true}
      title={title}
    >
      <span
        style={{ fontFamily: '"Rubik", system-ui, -apple-system, sans-serif', fontWeight: 900 }}
        className="leading-none text-primary drop-shadow-sm"
      >
        T
      </span>
    </div>
  );
}
