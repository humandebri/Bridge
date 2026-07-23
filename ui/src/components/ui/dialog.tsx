import * as DialogPrimitive from "@radix-ui/react-dialog"
import { X } from "lucide-react"
import type { ComponentProps } from "react"
import { cn } from "@/lib/utils"

export const Dialog = DialogPrimitive.Root
export const DialogTrigger = DialogPrimitive.Trigger
export const DialogClose = DialogPrimitive.Close

export function DialogContent({ className, children, ...props }: ComponentProps<typeof DialogPrimitive.Content>) {
  return <DialogPrimitive.Portal><DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/45 backdrop-blur-sm" /><DialogPrimitive.Content className={cn("fixed left-1/2 top-1/2 z-50 w-[calc(100%-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 rounded-[20px] bg-white p-6 shadow-2xl", className)} {...props}>{children}<DialogPrimitive.Close aria-label="Close confirmation" className="absolute right-5 top-5 rounded-xl p-2 text-[var(--muted)] hover:bg-[var(--panel)] hover:text-[var(--pink)]"><X className="size-4" /></DialogPrimitive.Close></DialogPrimitive.Content></DialogPrimitive.Portal>
}

export function DialogHeader({ className, ...props }: ComponentProps<"div">) { return <div className={cn("space-y-2 pr-7", className)} {...props} /> }
export function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>) { return <DialogPrimitive.Title className={cn("font-display text-3xl text-[var(--ink)]", className)} {...props} /> }
export function DialogDescription({ className, ...props }: ComponentProps<typeof DialogPrimitive.Description>) { return <DialogPrimitive.Description className={cn("text-sm leading-6 text-[var(--muted)]", className)} {...props} /> }
export function DialogFooter({ className, ...props }: ComponentProps<"div">) { return <div className={cn("mt-6 flex flex-col-reverse gap-3 sm:flex-row sm:justify-end", className)} {...props} /> }
