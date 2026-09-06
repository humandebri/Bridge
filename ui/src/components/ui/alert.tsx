import { cn } from "@/lib/utils"

export function Alert({
  className,
  tone = "info",
  ...props
}: React.HTMLAttributes<HTMLDivElement> & { tone?: "info" | "danger" | "warning" }) {
  const tones = {
    info: "border-[#8fc3ff] bg-[#eaf4ff] text-[#086cd9]",
    danger: "border-[#ffbec2] bg-[#ffeff0] text-[#dc2b2b]",
    warning: "border-[#ffd19b] bg-[#fff3e4] text-[#d5691b]",
  }
  return (
    <div
      role="alert"
      className={cn("rounded-2xl border px-4 py-3 text-sm leading-6", tones[tone], className)}
      {...props}
    />
  )
}
