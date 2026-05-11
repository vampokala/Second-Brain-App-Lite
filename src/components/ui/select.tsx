import { cn } from '@/lib/utils'
import { type SelectHTMLAttributes, forwardRef } from 'react'

type SelectProps = SelectHTMLAttributes<HTMLSelectElement>

export const Select = forwardRef<HTMLSelectElement, SelectProps>(({ className, ...props }, ref) => (
  <select
    ref={ref}
    className={cn(
      'w-full h-9 px-3 text-sm rounded-[var(--radius-md)]',
      'bg-[var(--color-background)] text-[var(--color-foreground)]',
      'border border-[var(--color-border)]',
      'focus:outline-2 focus:outline-[var(--color-accent)] focus:outline-offset-0',
      'disabled:opacity-50 disabled:cursor-not-allowed',
      className,
    )}
    {...props}
  />
))
Select.displayName = 'Select'
