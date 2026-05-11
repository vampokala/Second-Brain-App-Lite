import { cn } from '@/lib/utils'
import { type LabelHTMLAttributes, forwardRef } from 'react'

type LabelProps = LabelHTMLAttributes<HTMLLabelElement>

export const Label = forwardRef<HTMLLabelElement, LabelProps>(({ className, ...props }, ref) => (
  <label
    ref={ref}
    className={cn('text-sm font-medium text-[var(--color-foreground)] leading-none', className)}
    {...props}
  />
))
Label.displayName = 'Label'
