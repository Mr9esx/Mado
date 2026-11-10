import { display, sameLayout, type DisplayPage, type LayoutItem } from './layout.ts'

export function pagesFromSnapshot(snapshot?: { pages?: DisplayPage[]; layout: LayoutItem[] }): DisplayPage[] {
  return snapshot?.pages?.length ? snapshot.pages : [{ id: 'main', name: 'Codex 软件', layout: snapshot?.layout ?? display.initialLayout }]
}
export function nextPageId(pages: DisplayPage[], current: string, direction: number) {
  const index = pages.findIndex(page => page.id === current)
  return pages[index < 0 ? 0 : (index + direction + pages.length) % pages.length]?.id
}
export function samePages(left: DisplayPage[], right: DisplayPage[]) {
  return left.length === right.length && left.every((page, index) => page.id === right[index].id
    && page.name === right[index].name && sameLayout(page.layout, right[index].layout))
}
export function pageError(pages: DisplayPage[]) {
  if (!pages.length || pages.length > display.maxPages) return `请保留 1–${display.maxPages} 页。`
  if (new Set(pages.map(page => page.id)).size !== pages.length || pages.some(page => !page.id.trim())) return '页面 ID 为空或重复。'
  if (pages.some(page => !page.name.trim() || [...page.name].length > 20)) return '页面名称须为 1–20 个字符。'
  return ''
}
