import icons from '../shared/device-icons.json' with { type: 'json' }
import { normalizedRatio } from './displayFormat'

export type DeviceIconName = keyof typeof icons

export function DeviceIcon({ name, size, label, charge }: { name: DeviceIconName; size: number; label: string; charge?: number }) {
  // 仓库内固定的 IconPark SVG 路径与固件图标共用，颜色继承所在卡片。
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" fill="none" role="img" aria-label={label}>
      <g dangerouslySetInnerHTML={{ __html: icons[name].svg }} />
      {name === 'battery-empty' && (
        <rect x="7" y="19" height="10" fill="currentColor" width={Math.round(normalizedRatio(charge ?? 0, 0, 100) * 28)} />
      )}
    </svg>
  )
}
