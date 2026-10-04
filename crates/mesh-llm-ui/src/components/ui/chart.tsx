import * as React from 'react'
import type { ComponentProps, ReactNode } from 'react'
import * as RechartsPrimitive from 'recharts'
import { cn } from '@/lib/cn'

export type ChartConfig = {
  readonly [key: string]: {
    readonly label?: ReactNode
    readonly icon?: React.ComponentType<{ className?: string }>
    readonly color?: string
  }
}

export type ChartTooltipPayloadItem = {
  readonly dataKey?: string | number
  readonly name?: string
  readonly value?: unknown
  readonly color?: string
  readonly payload?: Record<string, unknown>
}

type ChartContextValue = { readonly config: ChartConfig }

const ChartContext = React.createContext<ChartContextValue | null>(null)

function useChart() {
  const context = React.useContext(ChartContext)
  if (!context) throw new Error('useChart must be used within a <ChartContainer />')
  return context
}

// Layer-3 fix from chart-bug.md: bypass recharts' <ResponsiveContainer>.
// The internal SizeDetector → redux store dispatch chain is the primary
// driver of the synchronous render loop (recharts v3.10.1 + react-redux v9
// `defaultNoopBatch` → "Maximum update depth exceeded" on the /logs page).
// We measure the wrapper once per layout change with a plain ResizeObserver
// and pass explicit numeric width/height to the chart child, which skips
// recharts' internal measurement and store notification entirely.
type ChartContainerSize = { readonly width: number; readonly height: number }

function useChartContainerSize(ref: React.RefObject<HTMLDivElement | null>): ChartContainerSize | undefined {
  const [size, setSize] = React.useState<ChartContainerSize | undefined>(undefined)
  const observerRef = React.useRef<ResizeObserver | null>(null)

  React.useEffect(() => {
    const element = ref.current
    if (!element || typeof ResizeObserver === 'undefined') {
      setSize((current) => current ?? { width: 0, height: 0 })
      return
    }
    const observer = new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1]
      if (!entry) return
      const { width, height } = entry.contentRect
      // Guard against zero-size containers while the panel is hidden: keep
      // the last known size instead of forcing a 0x0 chart.
      setSize((current) => {
        const nextWidth = Math.floor(width)
        const nextHeight = Math.floor(height)
        if (current && current.width === nextWidth && current.height === nextHeight) return current
        if (nextWidth === 0 && nextHeight === 0) return current
        if (nextWidth === 0 || nextHeight === 0) return current
        return { width: nextWidth, height: nextHeight }
      })
    })
    observer.observe(element)
    observerRef.current = observer
    return () => {
      observer.disconnect()
      observerRef.current = null
    }
  }, [ref])

  return size
}

export const ChartContainer = React.forwardRef<
  HTMLDivElement,
  Omit<ComponentProps<'div'>, 'children'> & {
    readonly config: ChartConfig
    readonly children: ComponentProps<typeof RechartsPrimitive.ResponsiveContainer>['children']
  }
>((props, forwardedRef) => {
  const { id, className, config, children, ...rest } = props
  const uniqueId = React.useId()
  const chartId = `chart-${id ?? uniqueId.replace(/:/g, '')}`
  const innerRef = React.useRef<HTMLDivElement | null>(null)
  const setInnerRef = React.useCallback(
    (node: HTMLDivElement | null) => {
      innerRef.current = node
      if (typeof forwardedRef === 'function') {
        forwardedRef(node)
      } else if (forwardedRef) {
        forwardedRef.current = node
      }
    },
    [forwardedRef]
  )
  const size = useChartContainerSize(innerRef)
  const chart = React.isValidElement(children)
    ? React.cloneElement(children, {
        ...(size === undefined ? {} : { width: size.width, height: size.height })
      })
    : children

  return (
    <ChartContext.Provider value={{ config }}>
      <div
        data-chart={chartId}
        ref={setInnerRef}
        className={cn(
          'flex justify-center text-[length:var(--density-type-caption)]',
          '[&_.recharts-cartesian-axis-tick_text]:fill-fg-faint',
          '[&_.recharts-cartesian-grid_line]:stroke-border-soft',
          '[&_.recharts-layer]:outline-none',
          '[&_.recharts-rectangle.recharts-tooltip-cursor]:fill-fg-faint',
          '[&_.recharts-surface]:outline-none',
          className
        )}
        {...rest}
      >
        <ChartStyle id={chartId} config={config} />
        {size === undefined ? null : chart}
      </div>
    </ChartContext.Provider>
  )
})
ChartContainer.displayName = 'ChartContainer'

function ChartStyle({ id, config }: { readonly id: string; readonly config: ChartConfig }) {
  const colorConfig = Object.entries(config).filter(([, item]) => item.color)
  if (colorConfig.length === 0) return null
  const css = `[data-chart="${id}"] {\n${colorConfig
    .map(([key, item]) => `  --color-${key}: ${item.color};`)
    .join('\n')}\n}`
  return <style>{css}</style>
}

// Defense-in-depth against recharts v3.10.x + react-redux v9's synchronous
// `defaultNoopBatch` notification loop: recharts' `ReportChartProps` runs
// `useEffect([dispatch, props])` with a fresh props object every render, so an
// unmemoized tooltip re-dispatches on every parent render. Memoizing the
// tooltip (shallow-comparing the `cursor` prop recharts shallow-compares, and
// reference-comparing the rest) breaks that re-dispatch cycle without changing
// tooltip behavior. See chart-bug.md at the repo root.
function chartTooltipPropsAreEqual(
  prev: ComponentProps<typeof RechartsPrimitive.Tooltip>,
  next: ComponentProps<typeof RechartsPrimitive.Tooltip>
): boolean {
  const prevKeys = Object.keys(prev) as (keyof ComponentProps<typeof RechartsPrimitive.Tooltip>)[]
  const prevLength = prevKeys.length
  if (prevLength !== Object.keys(next).length) return false
  for (const key of prevKeys) {
    if (key === 'cursor') {
      // recharts shallow-compares `cursor`; match that so the tooltip does not
      // opt out of the internal axis-props memoization allowlist.
      const prevCursor = prev[key]
      const nextCursor = next[key]
      if (prevCursor === nextCursor) continue
      if (
        typeof prevCursor === 'object' &&
        prevCursor !== null &&
        typeof nextCursor === 'object' &&
        nextCursor !== null &&
        Object.keys(prevCursor).length === Object.keys(nextCursor).length &&
        (Object.keys(prevCursor) as (keyof typeof prevCursor)[]).every(
          (cursorKey) => prevCursor[cursorKey] === nextCursor[cursorKey]
        )
      ) {
        continue
      }
      return false
    }
    if (prev[key] !== next[key]) return false
  }
  return true
}

export const ChartTooltip = React.memo((props: ComponentProps<typeof RechartsPrimitive.Tooltip>) => {
  return <RechartsPrimitive.Tooltip {...props} />
}, chartTooltipPropsAreEqual)
ChartTooltip.displayName = 'ChartTooltip'

type ChartTooltipContentProps = {
  readonly active?: boolean
  readonly payload?: readonly ChartTooltipPayloadItem[]
  readonly label?: unknown
  readonly className?: string
  readonly hideLabel?: boolean
  readonly hideIndicator?: boolean
  readonly hideName?: boolean
  readonly indicator?: 'dot' | 'line' | 'none'
  readonly labelKey?: string
  readonly nameKey?: string
  readonly labelFormatter?: (label: unknown, payload: readonly ChartTooltipPayloadItem[]) => ReactNode
  readonly formatter?: (value: unknown, name: string, item: ChartTooltipPayloadItem, index: number) => ReactNode
}

export function ChartTooltipContent({
  active,
  payload,
  label,
  className,
  hideLabel = false,
  hideIndicator = false,
  hideName = false,
  indicator = 'dot',
  labelKey,
  nameKey,
  labelFormatter,
  formatter
}: ChartTooltipContentProps) {
  const { config } = useChart()
  if (!active || !payload || payload.length === 0) return null

  const firstItem = payload[0]
  const dataKey = firstItem?.dataKey
  const configKey = labelKey ?? nameKey ?? (typeof dataKey === 'string' ? dataKey : undefined)
  const configItem = configKey ? config[configKey] : undefined
  const resolvedLabel = labelKey && firstItem?.payload ? firstItem.payload[labelKey] : label

  return (
    <div
      className={cn(
        'rounded-[var(--radius)] border border-border-soft bg-panel-strong px-3 py-2 shadow-surface-low',
        className
      )}
    >
      {!hideLabel && resolvedLabel != null ? (
        <div className="mb-1.5 text-[length:var(--density-type-label)] font-medium text-fg">
          {labelFormatter ? labelFormatter(resolvedLabel, payload) : String(resolvedLabel)}
        </div>
      ) : null}
      <div className="flex flex-col gap-1">
        {payload.map((item, index) => {
          const itemName = nameKey && item.payload ? String(item.payload[nameKey]) : item.name
          const itemConfig = itemName ? config[itemName] : undefined
          const color = item.color ?? itemConfig?.color ?? configItem?.color
          const displayName = hideName ? undefined : (itemConfig?.label ?? itemName)
          return (
            <div
              key={`chart-item-${String(itemName ?? '')}-${String(dataKey ?? '')}-${index}`}
              className="flex items-center gap-2"
            >
              {!hideIndicator && indicator !== 'none' ? (
                <span
                  aria-hidden="true"
                  className={cn('shrink-0 rounded-full', indicator === 'dot' ? 'size-2' : 'h-0.5 w-3.5')}
                  style={{ backgroundColor: color }}
                />
              ) : null}
              {displayName ? <span className="text-fg-dim">{displayName}</span> : null}
              <span className={cn('font-mono font-medium tabular-nums text-fg', displayName ? 'ml-auto' : 'ml-1')}>
                {formatter ? formatter(item.value, itemName ?? '', item, index) : String(item.value)}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
