import { cn } from '@/lib/utils'
import { type TextareaHTMLAttributes, forwardRef } from 'react'

type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement>

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(({ className, ...props }, ref) => (
  <textarea
    ref={ref}
    className={cn(
      'w-full px-3 py-2 text-sm rounded-[var(--radius-md)] resize-vertical min-h-24',
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
Textarea.displayName = 'Textarea'
