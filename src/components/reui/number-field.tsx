// Adapted from ReUI's Number Field and its stacked stepper example.
// https://reui.io/docs/components/radix/number-field
// https://reui.io/components/number-field/c-number-field-5
import { NumberField as NumberFieldPrimitive } from '@base-ui/react/number-field'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { cn } from '@/lib/utils'

export const NumberField = NumberFieldPrimitive.Root

export function NumberFieldGroup({ className, ...props }: NumberFieldPrimitive.Group.Props) {
  return <NumberFieldPrimitive.Group data-slot="number-field-group"
    className={cn('flex h-7 items-center rounded-md border border-input bg-background focus-within:border-ring', className)}
    {...props} />
}

export function NumberFieldInput({ className, ...props }: NumberFieldPrimitive.Input.Props) {
  return <NumberFieldPrimitive.Input data-slot="number-field-input"
    className={cn('h-full min-w-0 bg-transparent tabular-nums outline-none', className)} {...props} />
}

const stepButtonClass = 'flex min-h-0 w-full items-center justify-center leading-none enabled:hover:bg-accent focus-visible:bg-accent disabled:[&_svg]:opacity-40'

export function NumberFieldIncrement(props: NumberFieldPrimitive.Increment.Props) {
  return <NumberFieldPrimitive.Increment data-slot="number-field-increment"
    className={stepButtonClass} {...props}>
    <ChevronUp className="size-3" aria-hidden="true" />
  </NumberFieldPrimitive.Increment>
}

export function NumberFieldDecrement(props: NumberFieldPrimitive.Decrement.Props) {
  return <NumberFieldPrimitive.Decrement data-slot="number-field-decrement" className={stepButtonClass} {...props}>
    <ChevronDown className="size-3" aria-hidden="true" />
  </NumberFieldPrimitive.Decrement>
}
