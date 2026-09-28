import React from 'react';
import { cn } from '../../lib/utils';

export interface CardProps extends React.HTMLAttributes<HTMLDivElement> {
  hoverLift?: boolean;
}

export const Card = React.forwardRef<HTMLDivElement, CardProps>(
  ({ className, hoverLift = false, children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={cn(
          'bg-[var(--bg-surface)] border border-[var(--color-border)] rounded-[8px] p-5 transition-all duration-200 text-[var(--text-primary)]',
          'shadow-[0_4px_24px_rgba(28,25,23,0.06)] dark:shadow-[0_4px_24px_rgba(0,0,0,0.3)]',
          hoverLift &&
            'hover:border-[var(--color-accent)] hover:-translate-y-0.5 hover:shadow-[0_8px_28px_rgba(28,25,23,0.1)] dark:hover:shadow-[0_8px_28px_rgba(0,0,0,0.45)]',
          className,
        )}
        {...props}
      >
        {children}
      </div>
    );
  },
);
Card.displayName = 'Card';
