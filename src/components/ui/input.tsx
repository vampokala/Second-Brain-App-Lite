import { cn } from '@/lib/utils'
import { type InputHTMLAttributes, forwardRef } from 'react'

type InputProps = InputHTMLAttributes<HTMLInputElement>

export const Input = forwardRef<HTMLInputElement, InputProps>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      'w-full h-9 px-3 text-sm rounded-[var(--radius-md)]',
      'bg-[var(--color-background)] text-[var(--color-foreground)]',
      'border border-[var(--color-border)]',
      'placeholder:text-[var(--color-muted-foreground)]',
      'focus:outline-2 focus:outline-[var(--color-accent)] focus:outline-offset-0',
      'disabled:opacity-50 disabled:cursor-not-allowed',
      className,
    )}
    {...props}
  />
))
Input.displayName = 'Input'
