import { cn } from '@/lib/utils'
import { X } from 'lucide-react'
import { type HTMLAttributes } from 'react'

type ToastProps = HTMLAttributes<HTMLDivElement> & {
  kind: 'success' | 'error'
  message: string
  onDismiss: () => void
}

export function Toast({ kind, message, onDismiss, className, ...props }: ToastProps) {
  return (
    <div
      role="status"
      className={cn(
        'flex items-start gap-3 rounded-[var(--radius-md)] px-4 py-3 text-sm border',
        kind === 'success'
          ? 'bg-[color-mix(in_srgb,var(--color-success)_12%,transparent)] text-[var(--color-success)] border-[color-mix(in_srgb,var(--color-success)_25%,transparent)]'
          : 'bg-[color-mix(in_srgb,var(--color-destructive)_12%,transparent)] text-[var(--color-destructive)] border-[color-mix(in_srgb,var(--color-destructive)_25%,transparent)]',
        className,
      )}
      {...props}
    >
      <span className="flex-1 min-w-0 break-words">{message}</span>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss"
        className="shrink-0 opacity-70 hover:opacity-100 transition-opacity mt-0.5"
      >
        <X size={14} />
      </button>
    </div>
  )
}
