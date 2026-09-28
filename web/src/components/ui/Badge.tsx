import React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const badgeVariants = cva(
  'status-label inline-flex items-center gap-1.5 px-2 py-0.5 text-[11px] font-semibold border transition-colors',
  {
    variants: {
      variant: {
        default: 'bg-[var(--bg-sunken)] text-[var(--text-secondary)] border-[var(--color-border)]',
        copper: 'bg-[#D08A4C]/15 text-[#9E5D24] dark:text-[#D08A4C] border-[#D08A4C]/40',
        clay: 'bg-[#8B5E3F]/15 text-[#73482B] dark:text-[#D8A682] border-[#8B5E3F]/40',
        success:
          'bg-emerald-50 dark:bg-emerald-950/50 text-emerald-800 dark:text-emerald-300 border-emerald-300 dark:border-emerald-800/40',
        warning:
          'bg-amber-50 dark:bg-amber-950/50 text-amber-800 dark:text-amber-300 border-amber-300 dark:border-amber-800/40',
        danger:
          'bg-rose-50 dark:bg-rose-950/50 text-rose-800 dark:text-rose-300 border-rose-300 dark:border-rose-800/40',
        outline: 'border-[var(--color-border)] text-[var(--text-secondary)] bg-transparent',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {
  dot?: boolean;
  pip?: boolean;
}

export const Badge: React.FC<BadgeProps> = ({
  className,
  variant,
  dot,
  pip,
  children,
  ...props
}) => {
  return (
    <span className={cn(badgeVariants({ variant }), className)} {...props}>
      {pip && <span className="w-1.5 h-1.5 rounded-[1px] bg-[#D08A4C]" />}
      {dot && (
        <span
          className={cn(
            'w-1.5 h-1.5 rounded-full',
            variant === 'success' && 'bg-emerald-500 animate-pulse',
            variant === 'copper' && 'bg-[#D08A4C] animate-pulse',
            variant === 'warning' && 'bg-amber-500',
            variant === 'danger' && 'bg-rose-500',
            (!variant || variant === 'default') && 'bg-[#8B5E3F]',
          )}
        />
      )}
      {children}
    </span>
  );
};
