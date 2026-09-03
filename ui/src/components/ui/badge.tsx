import { cn } from "@/lib/utils"

export function Badge({
  className,
  tone = "neutral",
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & { tone?: "neutral" | "good" | "warn" | "info" }) {
  const tones = {
    neutral: "bg-black/5 text-[var(--muted)]",
    good: "bg-[#def2e6] text-[#11845b]",
    warn: "bg-[#fff3e4] text-[#92400e]",
    info: "bg-[#eaf4ff] text-[#086cd9]",
  }
  return (
    <span
      className={cn(
        "inline-flex items-center whitespace-nowrap rounded-full px-3 py-1 text-[11px] font-bold",
        tones[tone],
        className,
      )}
      {...props}
    />
  )
}
