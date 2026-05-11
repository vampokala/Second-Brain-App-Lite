import { useTheme } from './theme-provider'
import { cn } from '@/lib/utils'
import { Moon, Sun, SunMoon } from 'lucide-react'

export function ModeToggle() {
  const { theme, setTheme } = useTheme()

  const options = [
    { value: 'light' as const, icon: Sun, label: 'Light' },
    { value: 'system' as const, icon: SunMoon, label: 'System' },
    { value: 'dark' as const, icon: Moon, label: 'Dark' },
  ]

  return (
    <div className="flex items-center gap-0.5 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-muted)] p-0.5">
      {options.map(({ value, icon: Icon, label }) => (
        <button
          key={value}
          type="button"
          aria-label={label}
          title={label}
          onClick={() => setTheme(value)}
          className={cn(
            'flex items-center justify-center h-6 w-6 rounded-[var(--radius-sm)] transition-colors',
            theme === value
              ? 'bg-[var(--color-surface)] text-[var(--color-foreground)] shadow-sm'
              : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)]',
          )}
        >
          <Icon size={13} />
        </button>
      ))}
    </div>
  )
}
