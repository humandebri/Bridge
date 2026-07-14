import * as React from "react"
import { cn } from "@/lib/utils"

export function Input({ className, type, ...props }: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input type={type} className={cn("h-14 w-full rounded-2xl border border-[var(--line)] bg-white px-4 text-base text-black outline-none transition duration-300 placeholder:text-[#aaa] focus:border-[var(--pink)] focus:ring-3 focus:ring-[color:var(--pink)]/10 disabled:opacity-55", className)} {...props} />
}
