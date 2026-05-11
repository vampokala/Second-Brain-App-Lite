import { cn } from '@/lib/utils'
import { Check, Copy } from 'lucide-react'
import { useState } from 'react'

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const ta = document.createElement('textarea')
      ta.value = text
      ta.style.position = 'fixed'
      ta.style.left = '-9999px'
      document.body.appendChild(ta)
      ta.select()
      document.execCommand('copy')
      document.body.removeChild(ta)
      return true
    } catch {
      return false
    }
  }
}

export function CopyButton({ text, label, className }: { text: string; label: string; className?: string }) {
  const [copied, setCopied] = useState(false)
  const disabled = !text.trim().length

  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={async () => {
        if (disabled) return
        const ok = await copyToClipboard(text)
        if (ok) {
          setCopied(true)
          window.setTimeout(() => setCopied(false), 1600)
        }
      }}
      className={cn(
        'inline-flex items-center justify-center h-7 w-7 rounded-[var(--radius-sm)]',
        'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)]',
        'hover:bg-[var(--color-muted)] transition-colors',
        'disabled:opacity-40 disabled:cursor-not-allowed',
        className,
      )}
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  )
}
