import * as ProgressPrimitive from "@radix-ui/react-progress"
import { cn } from "@/lib/utils"

export function Progress({ className, value = 0, ...props }: React.ComponentProps<typeof ProgressPrimitive.Root>) {
  return <ProgressPrimitive.Root className={cn("relative mt-4 h-2.5 w-full overflow-hidden rounded-full bg-black/8", className)} {...props}><ProgressPrimitive.Indicator className="h-full bg-[var(--pink)] transition-transform" style={{ transform: `translateX(-${100 - Math.max(0, Math.min(100, value ?? 0))}%)` }} /></ProgressPrimitive.Root>
}
