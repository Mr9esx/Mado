import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Screen } from '../../src/Screen'
import { PageManager } from '../../src/PageManager'
import { WidgetShell } from '../../src/WidgetShell'
import { nextPageId } from '../../src/pages'
import { WidgetReading } from '../../src/WidgetReading'
import { CardSettings } from '../../src/CardSettings'
import { useCardPlayback } from '../../src/useCardPlayback'
import { defaultDisplayTheme, type LayoutItem, type Snapshot, type Widget } from '../../src/layout'
import '../../src/styles.css'
import '../../src/collector.css'

const definitions: Widget[] = [
  { id: 'quota', name: '剩余额度', shortName: '额度', kind: 'progress', unit: '%', accentColor: '#48B34A',
    detailFields: [{ key: 'window', label: '窗口' }] },
  { id: 'speed', name: '输出速度', shortName: '输出速度', kind: 'metric', unit: 'tok/s', numberAnimation: 'digitRoll' as const },
  { id: 'task', name: '任务状态', shortName: '任务状态', kind: 'state', unit: '', supportsNotify: true,
    stateLabels: { running: '运行中', completed: '已完成', error: '运行异常' },
    stateStyles: { running: { color: '#397ADC' }, completed: { color: '#48B34A' }, error: { color: '#D63F41' } } },
  { id: 'resets', name: '重置卡', shortName: '重置卡', kind: 'metric', unit: '张' },
  { id: 'recovery', name: '额度恢复', shortName: '额度恢复', kind: 'countdown', unit: '' },
].map(widget => ({ ...widget, group: 'codex', sizes: [[1, 1], [2, 1], [2, 2], [4, 1]] } as Widget))
const system: Widget[] = [
  { id: 'cpu', name: 'CPU占用', shortName: 'CPU占用', kind: 'chart', unit: '%' },
  { id: 'trend', name: 'CPU 趋势', shortName: 'CPU 趋势', kind: 'trend', unit: '%' },
  { id: 'range', name: 'CPU 区间', shortName: 'CPU 区间', kind: 'range', unit: '%' },
  { id: 'number', name: '数值缩写', shortName: '数值缩写', kind: 'metric', unit: 'tok/s' },
  { id: 'zero', name: '零值', shortName: '零值', kind: 'progress', unit: '%' },
  { id: 'missing', name: '无读数', shortName: '无读数', kind: 'metric', unit: 'tok/s' },
  { id: 'longUnit', name: '长单位', shortName: '长单位', kind: 'metric', unit: 'abcdefghi' },
].map(widget => ({ ...widget, group: 'system', sizes: [[1, 1]] }))
const placements = (widgets: Widget[]) => widgets.map((widget, index) => ({ id: widget.id, x: index % 4, y: Math.floor(index / 4), w: 1, h: 1 }))
const base: Snapshot = {
  revision: 1, sequence: 1, lastSyncedAt: new Date(2026, 10, 10, 17, 29, 30).getTime() / 1000,
  layout: placements(definitions), widgets: definitions, power: { source: 'battery', percent: 100 },
  values: { quota: { kind: 'progress', value: 81, ratio: .81, details: { window: '5 小时' } }, speed: 248, resets: 0,
    recovery: Date.now() / 1000 + 5 * 86400 + 4 * 3600,
    task: { kind: 'state', state: 'running', detail: '整理设备屏显示和同步状态' } },
}
const cpu: Snapshot = { ...base, layout: placements(system), widgets: system, values: {
  cpu: { kind: 'chart', values: [18, 26, 22, 42, 35, 61, null, 73, 54, 88, 67, 79], min: 0, max: 100 },
  trend: { kind: 'trend', value: 68, delta: 4, deltaUnit: 'pp' },
  range: { kind: 'range', value: 68, bounds: { min: 0, max: 100 } },
  number: 1234, zero: { kind: 'progress', value: 0, ratio: 0 }, longUnit: 12,
} }
function Preview() {
  const [snapshot, setSnapshot] = useState(base)
  const [pages, setPages] = useState([{ id: 'codex', name: 'Codex 软件', layout: base.layout }, { id: 'system', name: '系统', layout: cpu.layout }])
  const [pageId, setPageId] = useState('codex')
  const [rotation, setRotation] = useState(0)
  const currentPage = pages.find(page => page.id === pageId) ?? pages[0]
  const step = (direction: number) => setPageId(nextPageId(pages, pageId, direction) ?? pages[0].id)
  const [selected, setSelected] = useState('')
  const [title, setTitle] = useState('整理设备屏显示和同步状态')
  const [card, setCard] = useState<LayoutItem>({ id: 'quota', x: 0, y: 0, w: 1, h: 1 })
  const cardPlayback = useCardPlayback(card.id)
  const longError = '请检查 USB 数据线和 Studio 连接设置。\n'.repeat(24)
  const update = (state: string, notify: boolean) => setSnapshot(current => ({ ...current, sequence: current.sequence + 1,
    values: { ...current.values, task: { kind: 'state', state, detail: title, notify } } }))
  return <main style={{ padding: 24, color: '#222', background: '#eeede9', minHeight: '100vh' }}>
    <h1 style={{ fontSize: 22 }}>设备画布验收 · 100% 设备像素</h1>
    <div style={{ display: 'flex', gap: 12, marginBlock: 12 }}>
      <input aria-label="任务标题" value={title} onChange={event => setTitle(event.target.value)} />
      <button onClick={() => update('running', false)}>更新标题</button>
      <button onClick={() => update('completed', true)}>完成提醒</button>
      <button onClick={() => update('error', true)}>异常提醒</button>
      <button onClick={() => setSnapshot(current => ({ ...current, sequence: current.sequence + 1, values: { ...current.values, speed: 80 } }))}>速度 80</button>
      <button onClick={() => setSnapshot(current => ({ ...current, sequence: current.sequence + 1, values: { ...current.values, speed: 90 } }))}>速度 90</button>
    </div>
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 320px)', gap: '20px 24px' }}>
      {(['dark', 'light'] as const).map(theme => <section key={theme}><p>{theme} · Codex</p><Screen theme={theme} layout={snapshot.layout} snapshot={snapshot} /></section>)}
      {(['dark', 'light'] as const).map(theme => <section key={theme}><p>{theme} · 系统和边界</p><Screen theme={theme} layout={cpu.layout} snapshot={cpu} /></section>)}
      <section><p>编辑叠层</p><Screen layout={snapshot.layout} snapshot={snapshot} selected={selected} onCardPointerDown={(_, item) => setSelected(item.id)} onRemove={() => {}} /></section>
      <section><p>采集器预览 · 75 × 64</p><div className="collector-preview">{definitions.slice(0, 4).map(widget => <span key={widget.id} className="screen-widget is-narrow"><WidgetReading widget={widget} width={1} value={snapshot.values[widget.id]} /></span>)}</div></section>
      <section><p>桌面控件 · 未连接硬件、尚未应用</p><Screen surface="desktop" layout={base.layout} snapshot={{ ...base, lastSyncedAt: null }} /></section>
      <section><p>桌面控件 · 已应用</p><Screen surface="desktop" layout={cpu.layout} snapshot={cpu} /></section>
      <section><p>多页管理与工具栏交互</p>
        <PageManager pages={pages} selected={currentPage.id} disabled={false} rotationSeconds={rotation} onRotationChange={setRotation}
          onSelect={setPageId} onChange={(next, selected) => { setPages(next); if (selected) setPageId(selected) }} />
        <WidgetShell onPageStep={step} tools={<><button aria-label="上一页" onClick={() => step(-1)}>‹</button><button aria-label="下一页" onClick={() => step(1)}>›</button></>}>
          <Screen surface="desktop" pages={pages} pageId={currentPage.id} layout={currentPage.layout} onPageChange={() => step(1)}
            snapshot={{ ...base, widgets: [...definitions, ...system], values: { ...base.values, ...cpu.values } }} />
        </WidgetShell>
      </section>
      <section><p>等待连接</p><Screen layout={[]} /></section>
      <section><p>等待同步</p><Screen layout={base.layout} snapshot={{ ...base, lastSyncedAt: null }} /></section>
      <section><p>跨格 · 2 × 2、1 × 2</p><Screen snapshot={snapshot} layout={[
        { id: 'quota', x: 0, y: 0, w: 2, h: 2 },
        { id: 'speed', x: 2, y: 0, w: 2, h: 1 },
        { id: 'task', x: 2, y: 1, w: 2, h: 1 },
      ]} /></section>
      <section><p>连接失败 · 长原因</p><Screen layout={[]} connection={{ phase: 'failed', reason: longError }} /></section>
      <section><p>断线保留 · 充电</p><Screen layout={base.layout} snapshot={snapshot}
        connection={{ phase: 'disconnected', hasLayout: true, firmwareVersion: '0.1.0', power: { source: 'battery', percent: null, charging: true } }} /></section>
    </div>
    <section style={{ marginTop: 20, maxWidth: 476 }}><h2>跨格与组件设置</h2>
      <Screen snapshot={snapshot} playback={cardPlayback.frame && { ...cardPlayback.frame, widgetId: card.id }} layout={[
        { id: 'task', x: 0, y: 0, w: 4, h: 1 }, { ...card, y: 1, h: 1 },
      ]} />
      <CardSettings item={card} widget={definitions[0]} theme={defaultDisplayTheme} onChange={setCard}
        currentValue={snapshot.values[card.id]} onPlay={cardPlayback.play} resolveSize={item => item.h === 1 && item.w <= 4 ? item : undefined} />
    </section>
  </main>
}
createRoot(document.getElementById('root')!).render(<Preview />)
