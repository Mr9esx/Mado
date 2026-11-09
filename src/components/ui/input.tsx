// ReUI v1 — https://v1.reui.io/r/input.json
import * as React from 'react';
import { cn } from '@/lib/utils';
import { cva, type VariantProps } from 'class-variance-authority';

// Define input size variants
const inputVariants = cva(
  `
    flex w-full bg-background border border-input shadow-xs shadow-black/5 transition-[color,box-shadow] text-foreground placeholder:text-muted-foreground/80 
    focus-visible:ring-ring/30  focus-visible:border-ring focus-visible:outline-none focus-visible:ring-[3px]     
    disabled:cursor-not-allowed disabled:opacity-60 
    [&[readonly]]:bg-muted/80 [&[readonly]]:cursor-not-allowed
    file:h-full [&[type=file]]:py-0 file:border-solid file:border-input file:bg-transparent 
    file:font-medium file:not-italic file:text-foreground file:p-0 file:border-0 file:border-e
    aria-invalid:border-destructive/60 aria-invalid:ring-destructive/10 dark:aria-invalid:border-destructive dark:aria-invalid:ring-destructive/20
  `,
  {
    variants: {
      variant: {
        lg: 'h-10 px-4 text-sm rounded-md file:pe-4 file:me-4',
        md: 'h-9 px-3 text-sm rounded-md file:pe-3 file:me-3',
        sm: 'h-8 px-2.5 text-xs rounded-md file:pe-2.5 file:me-2.5',
      },
    },
    defaultVariants: {
      variant: 'md',
    },
  },
);

function Input({
  className,
  type,
  variant,
  ...props
}: React.ComponentProps<'input'> & VariantProps<typeof inputVariants>) {
  return <input data-slot="input" type={type} className={cn(inputVariants({ variant }), className)} {...props} />;
}

export { Input };
