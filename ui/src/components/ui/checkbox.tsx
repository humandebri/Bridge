import * as CheckboxPrimitive from "@radix-ui/react-checkbox"
import { Check } from "lucide-react"
import { cn } from "@/lib/utils"

export function Checkbox({ className, ...props }: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return <CheckboxPrimitive.Root className={cn("peer size-5 shrink-0 rounded-md border border-[var(--line-strong)] bg-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] data-[state=checked]:border-[var(--pink)] data-[state=checked]:bg-[var(--pink)]", className)} {...props}><CheckboxPrimitive.Indicator className="grid place-items-center text-white"><Check className="size-3.5" /></CheckboxPrimitive.Indicator></CheckboxPrimitive.Root>
}
