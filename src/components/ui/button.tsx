import { cn } from '@/lib/utils'
import { type ButtonHTMLAttributes, forwardRef } from 'react'

type Variant = 'primary' | 'secondary' | 'ghost' | 'destructive'
type Size = 'sm' | 'md' | 'icon'

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant
  size?: Size
}

const variantClasses: Record<Variant, string> = {
  primary: 'bg-[var(--color-accent)] text-[var(--color-accent-foreground)] hover:opacity-90',
  secondary: 'bg-[var(--color-surface)] text-[var(--color-foreground)] border border-[var(--color-border)] hover:bg-[var(--color-muted)]',
  ghost: 'bg-transparent text-[var(--color-foreground)] hover:bg-[var(--color-muted)]',
  destructive: 'bg-[var(--color-destructive)] text-[var(--color-destructive-foreground)] hover:opacity-90',
}

const sizeClasses: Record<Size, string> = {
  sm: 'h-8 px-3 text-sm',
  md: 'h-9 px-4 text-sm',
  icon: 'h-8 w-8 p-0 flex items-center justify-center',
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'primary', size = 'md', ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        'inline-flex items-center justify-center rounded-[var(--radius-md)] font-medium transition-colors',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        'focus-visible:outline-2 focus-visible:outline-[var(--color-accent)] focus-visible:outline-offset-2',
        variantClasses[variant],
        sizeClasses[size],
        className,
      )}
      {...props}
    />
  ),
)
Button.displayName = 'Button'
