import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"
import * as React from "react"
import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-2xl text-sm font-bold transition-[background,color,transform,border-color] duration-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-45 active:translate-y-0",
  {
    variants: {
      variant: {
        default: "bg-black text-white hover:-translate-y-[3px] hover:bg-[var(--pink)]",
        bridge: "bg-black text-white hover:-translate-y-[3px] hover:bg-[var(--pink)]",
        outline:
          "border border-[var(--line)] bg-white text-black hover:-translate-y-[3px] hover:border-[var(--pink)] hover:bg-[var(--pink)] hover:text-white",
        ghost: "text-black hover:bg-[var(--panel)] hover:text-[var(--pink)]",
        danger: "bg-black text-white hover:-translate-y-[3px] hover:bg-[var(--danger)]",
      },
      size: {
        default: "h-12 px-6",
        sm: "h-11 px-5 text-sm",
        lg: "h-16 px-9 text-base",
        icon: "size-11",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

export function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot : "button"
  return <Comp className={cn(buttonVariants({ variant, size, className }))} {...props} />
}
