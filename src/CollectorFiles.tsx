import { hotkeysCoreFeature, syncDataLoaderFeature } from '@headless-tree/core'
import { useTree } from '@headless-tree/react'
import { ChevronDown, ChevronRight, FileCode2, FileJson2, Folder, Star } from 'lucide-react'
import { useEffect, useMemo } from 'react'
import type { Draft } from './collectorApi'

export type ProjectItem = { id: string; name: string; folder: boolean; children: string[] }
export const samplePath = 'fixtures/sample.json'

export function projectItems(draft: Draft) {
  const items = new Map<string, ProjectItem>([
    ['root', { id: 'root', name: '项目', folder: true, children: [] }],
  ])
  function add(id: string, folder: boolean, name = id.split('/').at(-1) ?? id) {
    if (items.has(id)) return
    const parent = id.includes('/') ? id.slice(0, id.lastIndexOf('/')) : 'root'
    if (!items.has(parent)) add(parent, true)
    items.set(id, { id, name, folder, children: [] })
    items.get(parent)?.children.push(id)
  }
  for (const path of [...draft.directories].sort()) add(path, true)
  for (const path of Object.keys(draft.files).sort()) add(path, false)
  return items
}

export function CollectorFiles({ draft, active, onOpen, onSelect }: {
  draft: Draft
  active: string
  onOpen: (item: string) => void
  onSelect: (item: string) => void
}) {
  const data = useMemo(() => projectItems(draft), [draft.files, draft.directories])
  const tree = useTree<ProjectItem>({
    rootItemId: 'root',
    initialState: { expandedItems: ['src', ...draft.directories] },
    getItemName: item => item.getItemData().name,
    isItemFolder: item => item.getItemData().folder,
    onPrimaryAction: item => {
      const id = item.getId()
      onSelect(id)
      if (!item.isFolder()) onOpen(id)
    },
    dataLoader: {
      getItem: id => data.get(id) ?? { id, name: id, folder: false, children: [] },
      getChildren: id => data.get(id)?.children ?? [],
    },
    features: [syncDataLoaderFeature, hotkeysCoreFeature],
  })
  useEffect(() => tree.rebuildTree(), [data, tree])

  return <div className="collector-tree" {...tree.getContainerProps('项目文件')}>
    {tree.getItems().map(item => {
      const id = item.getId()
      const data = item.getItemData()
      const Icon = data.folder ? Folder : /\.m?js$/.test(id) ? FileCode2 : FileJson2
      const Chevron = item.isExpanded() ? ChevronDown : ChevronRight
      return <button type="button" key={id} {...item.getProps()} className="collector-file" data-active={active === id}
        style={{ paddingLeft: 8 + item.getItemMeta().level * 12 }}>
        {data.folder ? <Chevron size={12} aria-hidden="true" /> : <span className="collector-file-indent" />}
        <Icon size={14} aria-hidden="true" />
        <span>{data.name}</span>
        {draft.entry === id && <Star size={12} aria-label="入口文件" />}
      </button>
    })}
  </div>
}
