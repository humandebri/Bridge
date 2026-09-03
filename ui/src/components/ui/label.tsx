import * as LabelPrimitive from "@radix-ui/react-label"
import { cn } from "@/lib/utils"

export function Label({ className, ...props }: React.ComponentProps<typeof LabelPrimitive.Root>) {
  return (
    <LabelPrimitive.Root className={cn("text-sm font-bold text-black", className)} {...props} />
  )
}
