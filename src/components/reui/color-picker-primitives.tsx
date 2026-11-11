import type { ComponentProps } from 'react'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import * as PopoverPrimitive from '@radix-ui/react-popover'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'

// Use Studio's existing controls for the shadcn primitives expected by the picker.
export const Popover = PopoverPrimitive.Root
export const PopoverTrigger = PopoverPrimitive.Trigger

export function PopoverContent({ className, sideOffset = 6, ...props }: ComponentProps<typeof PopoverPrimitive.Content>) {
  return <PopoverPrimitive.Portal>
    <PopoverPrimitive.Content sideOffset={sideOffset} collisionPadding={10}
      className={cn('z-50 rounded-lg border border-border bg-popover p-3 text-popover-foreground shadow-lg outline-none', className)}
      {...props} />
  </PopoverPrimitive.Portal>
}

export const DropdownMenu = Dropdown.Root
export const DropdownMenuTrigger = Dropdown.Trigger
export const DropdownMenuRadioGroup = Dropdown.RadioGroup

export function DropdownMenuContent({ className, ...props }: ComponentProps<typeof Dropdown.Content>) {
  return <Dropdown.Portal><Dropdown.Content sideOffset={4}
    className={cn('z-[60] rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-md', className)}
    {...props} /></Dropdown.Portal>
}

export function DropdownMenuRadioItem({ className, ...props }: ComponentProps<typeof Dropdown.RadioItem>) {
  return <Dropdown.RadioItem className={cn('flex items-center gap-2 rounded-sm px-2 py-1 text-xs outline-none data-highlighted:bg-accent', className)}
    {...props} />
}

export function InputGroup({ className, ...props }: ComponentProps<'div'>) {
  return <div className={cn('flex h-7 min-w-0 items-center rounded-md border border-input bg-background focus-within:border-ring', className)}
    {...props} />
}

export function InputGroupAddon({ align, className, ...props }: ComponentProps<'div'> & { align?: 'inline-start' }) {
  return <div data-align={align} className={cn('flex shrink-0 items-center ps-1', className)} {...props} />
}

export function InputGroupButton({ size, className, ...props }: Omit<ComponentProps<typeof Button>, 'size'> & { size?: 'xs' | 'icon-xs' }) {
  return <Button size="xs" variant="ghost" mode={size === 'icon-xs' ? 'icon' : undefined}
    className={cn(size === 'icon-xs' && 'size-5 p-0', className)} {...props} />
}

export function InputGroupInput({ className, ...props }: ComponentProps<typeof Input>) {
  return <Input variant="sm" className={cn('h-full min-w-0 rounded-none border-0 bg-transparent px-1.5 shadow-none focus-visible:ring-0', className)}
    {...props} />
}

export function Separator({ orientation, ...props }: ComponentProps<'span'> & { orientation?: 'vertical' | 'horizontal' }) {
  return <span className={orientation === 'vertical' ? 'w-px self-stretch bg-border' : 'h-px w-full bg-border'} {...props} />
}
