import type { DisplayPage, DisplayTheme } from './layout.ts'
import { samePages } from './pages.ts'

type LayoutEdit = { pages: DisplayPage[]; rotationSeconds: number; theme: DisplayTheme }
type LayoutDraft = LayoutEdit & { baseRevision: number }

export type LayoutHistory = {
  draft: LayoutDraft
  previous: LayoutHistory | null
  pageBeforeEdit: string
}

export function sameLayoutEdit(left: LayoutEdit, right: LayoutEdit) {
  return left.rotationSeconds === right.rotationSeconds
    && (left.pages === right.pages || samePages(left.pages, right.pages))
    && (left.theme === right.theme || JSON.stringify(left.theme) === JSON.stringify(right.theme))
}

export function recordLayoutEdit(
  history: LayoutHistory | null, saved: LayoutDraft, next: LayoutEdit, pageBeforeEdit: string,
) {
  const current = history?.draft ?? saved
  if (sameLayoutEdit(current, next)) return history
  return { draft: { ...next, baseRevision: current.baseRevision }, previous: history, pageBeforeEdit }
}
