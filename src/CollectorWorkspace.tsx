import { ArrowRight, Braces, Check, ChevronRight, CircleAlert, CirclePlay, Cpu, Download, FileCode2, FilePlus2, FolderPlus, GitBranch, Pencil, Plus, RotateCcw, Settings2, Terminal, Trash2, Upload, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { CollectorCode } from './CollectorCode'
import { CollectorFiles, projectItems, samplePath } from './CollectorFiles'
import { CollectorAuthorization, CollectorCodexSettings } from './CollectorAuthorization'
import { createDraft, exportModule, focusLayout, getCatalog, getDraft, getGrantRequests, importModule, inspectImport, publishDraft, readDefinition, saveDraft, testDraft, type Approval, type Catalog, type Draft, type GrantRequest, type ImportReview, type TestResult } from './collectorApi'
import { WidgetReading } from './WidgetReading'
import type { Reading, Widget } from './layout'
import './collector.css'

type FileOperation = 'file' | 'folder' | 'rename' | 'delete'
type DeletedFiles = { before: Draft; after: Draft; file: string; selected: string; tabs: string[] }
const templates = [
  { id: 'sample', label: '快速开始', detail: '用一个固定读数试跑完整流程', icon: Braces },
  { id: 'http', label: 'HTTP 接口', detail: '从返回 JSON 的接口采集', icon: GitBranch },
  { id: 'file', label: '本地文件', detail: '从 JSON 文件读取数值', icon: FileCode2 },
]
const labelForFile = (path: string) => path.split('/').at(-1) ?? path
const pathParent = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf('/')))

export function CollectorWorkspace() {
  const [catalog, setCatalog] = useState<Catalog>()
  const [draft, setDraft] = useState<Draft>()
  const draftRef = useRef<Draft | undefined>(undefined)
  const savedRef = useRef<Draft | undefined>(undefined)
  const saveQueue = useRef<Promise<void>>(Promise.resolve())
  const navigating = useRef(false)
  const [tabs, setTabs] = useState<string[]>([])
  const [file, setFile] = useState('')
  const [selectedPath, setSelectedPath] = useState('')
  const [creating, setCreating] = useState(false)
  const [importing, setImporting] = useState<ImportReview>()
  const [name, setName] = useState('')
  const [template, setTemplate] = useState('sample')
  const [busy, setBusy] = useState('')
  const [notice, setNotice] = useState('')
  const [error, setError] = useState('')
  const [saveError, setSaveError] = useState('')
  const [fileOperation, setFileOperation] = useState<FileOperation>()
  const [fileName, setFileName] = useState('')
  const [deletedFiles, setDeletedFiles] = useState<DeletedFiles>()
  const [test, setTest] = useState<TestResult>()
  const [resultDraft, setResultDraft] = useState<Draft>()
  const [testedDraft, setTestedDraft] = useState<Draft>()
  const [jumpLine, setJumpLine] = useState<number>()
  const [resultView, setResultView] = useState<'preview' | 'output' | 'sample'>('preview')
  const [grantPrompt, setGrantPrompt] = useState<{ requests: GrantRequest[]; live: boolean; confirm: (approval?: Approval) => void }>()
  const testedApproval = useRef<{ draft: Draft; approval: Approval } | undefined>(undefined)
  const [showCodexSettings, setShowCodexSettings] = useState(false)
  const module = catalog?.modules.find(module => module.id === draft?.id)
  const readOnly = module?.builtIn === true
  const editorDraft = draft && resultDraft?.id === draft.id ? { ...draft, definition: resultDraft.definition } : draft
  const definition = readDefinition(editorDraft?.definition ?? '')
  const hasSampleFile = Boolean(draft && Object.hasOwn(draft.files, samplePath))
  const selected = draft && projectItems(draft).get(selectedPath)
  const canEditPath = selected && selected.id !== 'root'
  const passed = Boolean(draft && testedDraft === draft && test?.result)
  const activeText = draft?.files[file]

  useEffect(() => {
    let active = true
    function load() {
      void getCatalog().then(value => { if (active) setCatalog(value) }, reason => { if (active) setError(String(reason)) })
    }
    load()
    const interval = window.setInterval(load, 2500)
    return () => { active = false; window.clearInterval(interval) }
  }, [])

  function editable(next: Draft) {
    testedApproval.current = undefined
    draftRef.current = next
    setDraft(next)
    setNotice('')
  }

  function persist(next: Draft | undefined) {
    if (!next || next === savedRef.current || next.id === 'codex' || next.id === 'system') return saveQueue.current
    const pending = saveQueue.current.catch(() => {}).then(async () => {
      await saveDraft(next)
      savedRef.current = next
      setSaveError('')
    })
    saveQueue.current = pending
    return pending
  }

  useEffect(() => {
    if (!draft || draft === savedRef.current || readOnly) return
    const timer = window.setTimeout(() => {
      void persist(draft).catch(reason => setSaveError(String(reason)))
    }, 360)
    return () => window.clearTimeout(timer)
  }, [draft, readOnly])

  useEffect(() => {
    const window = getCurrentWebviewWindow()
    let alive = true
    let stop: (() => void) | undefined
    void window.onCloseRequested(async event => {
      const current = draftRef.current
      if (!current || current === savedRef.current || current.id === 'codex' || current.id === 'system') return
      navigating.current = true
      try {
        await persist(current)
      } catch (reason) {
        navigating.current = false
        event.preventDefault()
        setSaveError(String(reason))
      }
    }).then(unlisten => { if (alive) stop = unlisten; else unlisten() }, reason => setSaveError(String(reason)))
    return () => { alive = false; stop?.() }
  }, [])

  function openFile(path: string, line?: number) {
    setFile(path)
    setSelectedPath(path)
    setJumpLine(line)
    setTabs(current => current.includes(path) ? current : [...current, path])
  }

  function adopt(next: Draft) {
    draftRef.current = next
    savedRef.current = next
    setDraft(next)
    setCreating(false)
    setImporting(undefined)
    setFile(next.entry)
    setSelectedPath(next.entry)
    setTabs([next.entry])
    setResultView('preview')
    setTest(undefined)
    setResultDraft(undefined)
    setTestedDraft(undefined)
    testedApproval.current = undefined
    setJumpLine(undefined)
    setFileOperation(undefined)
    setDeletedFiles(undefined)
    setNotice('')
  }

  async function selectModule(id: string) {
    if (busy) return
    navigating.current = true
    setBusy('open')
    setError('')
    try {
      await persist(draftRef.current)
      adopt(await getDraft(id))
    } catch (reason) {
      setError(String(reason))
    } finally {
      navigating.current = false
      setBusy('')
    }
  }

  async function create() {
    setBusy('create')
    setError('')
    try {
      await persist(draftRef.current)
      const next = await createDraft(name, template)
      setCatalog(await getCatalog())
      adopt(next)
      setName('')
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy('')
    }
  }

  async function chooseImport() {
    setBusy('import')
    setError('')
    try {
      await persist(draftRef.current)
      const result = await inspectImport()
      if (result?.identical) {
        adopt(await getDraft(result.draft.id))
        setNotice('已打开相同的模块。')
      } else if (result) {
        setCreating(false)
        setImporting(result)
      }
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy('')
    }
  }

  async function acceptImport(mode: 'new' | 'update' | 'copy') {
    if (!importing) return
    setBusy('import')
    setError('')
    try {
      adopt(await importModule(importing.draft, mode))
      setCatalog(await getCatalog())
      setNotice('已导入草稿。检查定义和代码后即可试跑。')
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy('')
    }
  }

  async function saveExport() {
    if (!draft) return
    setBusy('export')
    setError('')
    try {
      const path = await exportModule(draft.id)
      if (path) setNotice('已导出发布版本到 ' + path)
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy('')
    }
  }

  async function requestApproval(draft: Draft, { restore = false, live = false } = {}) {
    const requests = await getGrantRequests(draft, restore, live)
    if (requests.length === 0) return { accepted: true } as const
    const previous = testedApproval.current
    if (!live && !restore && previous?.draft === draft
        && JSON.stringify(previous.approval.requests) === JSON.stringify(requests)) {
      return { accepted: true, approval: previous.approval } as const
    }
    const approval = await new Promise<Approval | undefined>(confirm => setGrantPrompt({ requests, live, confirm }))
    return approval ? { accepted: true, approval } as const : { accepted: false } as const
  }

  function finishGrantPrompt(approval?: Approval) {
    grantPrompt?.confirm(approval)
    setGrantPrompt(undefined)
  }

  async function runTest(live = false) {
    const target = draftRef.current
    if (!target || busy) return
    setBusy('test')
    setError('')
    setNotice('')
    try {
      await persist(target)
      const authorization = live ? await requestApproval(target, { live: true }) : { accepted: true } as const
      if (!authorization.accepted) return
      testedApproval.current = undefined
      setTestedDraft(undefined)
      setResultView('preview')
      const response = await testDraft(target, live, authorization.approval)
      setTest(response)
      setResultDraft(response.definition ? { ...target, definition: JSON.stringify(response.definition) } : target)
      if (response.result && draftRef.current === target) {
        setTestedDraft(target)
        if (authorization.approval) testedApproval.current = { draft: target, approval: authorization.approval }
      }
    } catch (reason) {
      testedApproval.current = undefined
      setTestedDraft(undefined)
      setTest({ result: null, error: String(reason), logs: [] })
    } finally {
      setBusy('')
    }
  }

  async function publish(restore = false) {
    if (!draft || busy || (!restore && !passed)) return
    setBusy(restore ? 'restore' : 'publish')
    setError('')
    try {
      await persist(draftRef.current)
      const authorization = await requestApproval(draft, { restore })
      if (!authorization.accepted) return
      setCatalog(await publishDraft(draft.id, restore, authorization.approval))
      testedApproval.current = undefined
      setTestedDraft(undefined)
      setNotice(restore ? '已恢复上一发布版本。' : '已发布，可在屏幕布局选择这个采集器的卡片。')
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy('')
    }
  }

  function editContent(text: string) {
    const current = draftRef.current
    if (!current || readOnly || busy || navigating.current) return
    editable({ ...current, files: { ...current.files, [file]: text } })
  }

  function fileAction(operation: FileOperation) {
    setFileOperation(operation)
    setFileName(operation === 'rename' ? selected?.name ?? '' : operation === 'file' ? 'helper.mjs' : '')
    setError('')
  }

  function updateFiles() {
    if (!draft || !fileOperation || navigating.current) return
    const original = selectedPath
    const relativeTo = selected?.folder ? original : pathParent(original)
    const directory = fileOperation === 'rename' ? pathParent(original) : relativeTo
    const parent = directory === 'root' ? '' : directory
    const base = fileName.trim()
    const nextPath = parent ? parent + '/' + base : base
    const within = (path: string) => path === original || path.startsWith(original + '/')
    const remap = (path: string) => within(path) ? nextPath + path.slice(original.length) : path
    setError('')
    if (fileOperation === 'delete') {
      if (within(draft.entry)) {
        setError('请先将其他文件设为入口，再删除该文件或文件夹。')
        return
      }
      const after = {
        ...draft,
        files: Object.fromEntries(Object.entries(draft.files).filter(([path]) => !within(path))),
        directories: draft.directories.filter(path => !within(path)),
      }
      editable(after)
      setDeletedFiles({ before: draft, after, file, selected: selectedPath, tabs })
      setTabs(current => current.filter(path => !within(path)))
      if (within(file)) openFile(draft.entry)
      setSelectedPath('')
    } else {
      if (!base || /[\\/<>:"|?*]/.test(base) || ['.', '..', 'root'].includes(base.toLowerCase())) {
        setError('请输入有效文件名。')
        return
      }
      if (fileOperation === 'file' && !/\.(?:mjs|js|json)$/.test(base)) {
        setError('请选择 .mjs、.js 或 .json 扩展名。')
        return
      }
      if (fileOperation === 'rename' && nextPath === original) {
        setFileOperation(undefined)
        return
      }
      if ([...projectItems(draft).keys()].some(path => path.toLowerCase() === nextPath.toLowerCase() && path !== original)) {
        setError('同一目录下已有同名文件或文件夹。')
        return
      }
      if (fileOperation === 'file') {
        editable({ ...draft, files: { ...draft.files, [nextPath]: base.endsWith('.json') ? '{}\n' : '' } })
        openFile(nextPath)
      } else if (fileOperation === 'folder') {
        editable({ ...draft, directories: [...draft.directories, nextPath] })
        setSelectedPath(nextPath)
      } else {
        if (within(draft.entry) && !/\.m?js$/.test(remap(draft.entry))) {
          setError('入口文件须保留 .js 或 .mjs 扩展名。')
          return
        }
        editable({
          ...draft,
          files: Object.fromEntries(Object.entries(draft.files).map(([path, value]) => [remap(path), value])),
          directories: draft.directories.map(remap),
          entry: remap(draft.entry),
        })
        setTabs(current => current.map(remap))
        setFile(remap(file))
        setSelectedPath(nextPath)
      }
    }
    setFileOperation(undefined)
  }

  const jsError = test?.error?.match(/([\p{L}\w./-]+\.m?js):(\d+)(?::\d+)?/u)
  const sampleError = test?.error?.match(/样例数据：.*?line (\d+)/)
  const testLocation = jsError && draft?.files[jsError[1]] !== undefined
    ? { path: jsError[1], line: Number(jsError[2]) }
    : sampleError && hasSampleFile ? { path: samplePath, line: Number(sampleError[1]) } : undefined
  const preview = resultDraft && test?.result && buildPreview(resultDraft, test.result.outputs)

  return <main className="studio collector-workspace" aria-label="数据采集器工作区">
    <div className="window-grip" data-tauri-drag-region aria-hidden="true" />
    <header className="collector-masthead">
      <h1>数据采集器</h1>
      <div className="collector-actions">
        <Button size="sm" variant="ghost" disabled={Boolean(busy)} onClick={() => void chooseImport()}><Download size={15} />导入模块</Button>
        <Button size="sm" variant="outline" disabled={Boolean(busy)} onClick={() => { setCreating(true); setImporting(undefined); setError('') }}>
          <Plus size={16} />新建采集器
        </Button>
      </div>
    </header>
    <div className="collector-body">
      <nav className="collector-nav" aria-label="采集模块">
        <h2 className="collector-nav-heading">模块</h2>
        {catalog?.modules.map(item => {
          const Icon = item.id === 'system' ? Cpu : item.builtIn ? Terminal : Braces
          return <Button autoHeight size="xs" variant="ghost" key={item.id} type="button" className="collector-module" data-active={!creating && item.id === draft?.id}
            onClick={() => void selectModule(item.id)} disabled={Boolean(busy)}>
            <Icon size={16} aria-hidden="true" />
            <span><strong>{item.name}</strong><small>{item.builtIn ? '内置模块' : item.revision == null ? '未发布' : item.changed ? '已发布 · 有草稿' : '已发布'}</small></span>
            <ChevronRight size={13} aria-hidden="true" />
          </Button>
        })}
      </nav>
      <section className="collector-main" aria-label="模块工作区">
        {(error || saveError) && <div className="collector-banner collector-error" role="alert"><CircleAlert size={16} />{error || saveError}</div>}
        {notice && <div className="collector-banner collector-success" role="status"><Check size={16} />{notice}</div>}
        {deletedFiles && draft === deletedFiles.after && <div className="collector-banner" role="status">
          <span>已从草稿删除 {labelForFile(deletedFiles.selected)}。</span>
          <Button size="xs" variant="ghost" onClick={() => {
            editable(deletedFiles.before)
            setTabs(deletedFiles.tabs)
            setFile(deletedFiles.file)
            setSelectedPath(deletedFiles.selected)
            setDeletedFiles(undefined)
          }}>撤销删除</Button>
        </div>}
        {importing ? <div className="collector-start">
          <div className="collector-section-label">导入模块</div>
          <h2>{readDefinition(importing.draft.definition).name ?? '采集模块'}</h2>
          <p>{importing.conflict ? '目录已有相同 ID 的模块。可以替换编辑中的草稿，或保留它并导入副本。' : 'Studio 会把项目打开为草稿。请确认脚本来自你自己或你信任的作者。'}</p>
          <div className="collector-form-actions">
            <Button variant="ghost" size="sm" onClick={() => setImporting(undefined)} disabled={Boolean(busy)}>取消</Button>
            {importing.conflict && <Button variant="outline" size="sm" disabled={Boolean(busy)} onClick={() => void acceptImport('update')}>作为更新导入</Button>}
            <Button size="sm" disabled={Boolean(busy)} onClick={() => void acceptImport(importing.conflict ? 'copy' : 'new')}>{importing.conflict ? '作为副本导入' : '导入草稿'}</Button>
          </div>
        </div> : creating ? <form className="collector-start" onSubmit={event => { event.preventDefault(); void create() }}>
          <h2>新建采集器</h2>
          <p>选择一个模板，Studio 会建立可直接试跑的项目。</p>
          <div className="collector-templates">
            {templates.map(({ id, label, detail, icon: Icon }) => <Button autoHeight size="xs" variant="ghost" key={id} type="button" aria-pressed={id === template}
              onClick={() => setTemplate(id)}><Icon size={16} aria-hidden="true" /><span><strong>{label}</strong><small>{detail}</small></span>
              {id === template && <Check size={14} aria-hidden="true" />}</Button>)}
          </div>
          <label>采集器名称<Input autoFocus required maxLength={80} value={name} placeholder="例如 主机温度" onChange={event => setName(event.target.value)} /></label>
          <div className="collector-form-actions">
            <Button variant="ghost" size="sm" onClick={() => setCreating(false)}>取消</Button>
            <Button type="submit" size="sm" disabled={!name.trim() || Boolean(busy)}>打开工作区<ArrowRight size={15} /></Button>
          </div>
        </form> : draft && module && catalog ? <>
          {catalog.status[module.id]?.error && <div className="collector-banner collector-error" role="status">{catalog.status[module.id].error}</div>}
          <div className="collector-workbar">
            <div><h2>{definition.name ?? module.name}</h2>
              <span>{module.builtIn ? '内置 · 可试跑' : module.revision == null ? '本地草稿'
                : '已发布 · 版本 ' + module.revision + (catalog.status[module.id]?.updatedAt && !catalog.status[module.id]?.error ? ' · 采集正常' : '')}</span></div>
            <div className="collector-actions">
              {!readOnly && module.revision != null && <Button size="sm" variant="ghost" disabled={Boolean(busy)} onClick={() => void saveExport()} title="导出发布版本"><Upload size={14} />导出</Button>}
              {module.canRestore && <Button size="sm" variant="ghost" disabled={Boolean(busy)} onClick={() => void publish(true)} title="恢复上一发布版本"><RotateCcw size={14} />恢复</Button>}
              {module.id === 'codex' && <Button size="sm" variant="ghost" onClick={() => setShowCodexSettings(true)}><Settings2 size={14} />连接设置</Button>}
              {!readOnly && module.revision != null && <Button size="sm" variant="ghost" title="返回屏幕布局" onClick={() => void focusLayout().catch(reason => setError(String(reason)))}><ArrowRight size={14} />布局</Button>}
              <Button size="sm" variant="outline" disabled={Boolean(busy)} onClick={() => void runTest()}>
                <CirclePlay size={15} />{busy === 'test' ? '运行中…' : '测试运行'}
              </Button>
              {!readOnly && <Button size="sm" disabled={!passed || Boolean(busy)} onClick={() => void publish()} title={passed ? undefined : '当前草稿试跑通过后可以发布'}>
                <Check size={15} />发布
              </Button>}
            </div>
          </div>
            <div className="collector-workbench">
              <aside className="collector-files">
                <div className="collector-files-head"><strong>项目文件</strong>{!readOnly && <div>
                  <Button size="xs" variant="ghost" type="button" title="新建文件" aria-label="新建文件" disabled={Boolean(busy)} onClick={() => fileAction('file')}><FilePlus2 size={15} /></Button>
                  <Button size="xs" variant="ghost" type="button" title="新建文件夹" aria-label="新建文件夹" disabled={Boolean(busy)} onClick={() => fileAction('folder')}><FolderPlus size={15} /></Button>
                </div>}</div>
                <CollectorFiles key={draft.id} draft={draft} active={selectedPath} onOpen={openFile} onSelect={setSelectedPath} />
                {!readOnly && canEditPath && <div className="collector-file-actions">
                  <Button size="xs" variant="ghost" type="button" title="重命名" aria-label="重命名" disabled={Boolean(busy)} onClick={() => fileAction('rename')}><Pencil size={14} /></Button>
                  <Button size="xs" variant="ghost" type="button" title="删除" aria-label="删除" disabled={Boolean(busy)} onClick={() => fileAction('delete')}><Trash2 size={14} /></Button>
                  {!selected.folder && draft.entry !== selected.id && /\.m?js$/.test(selected.id) &&
                    <Button size="xs" variant="ghost" type="button" disabled={Boolean(busy)} onClick={() => editable({ ...draft, entry: selected.id })}>设为入口</Button>}
                </div>}
              </aside>
              <div className="collector-editor">
                <div className="collector-editor-tabs" aria-label="已打开文件">
                  {tabs.map(path => <div key={path} data-active={path === file}>
                    <Button size="xs" variant="ghost" type="button" aria-pressed={path === file} onClick={() => openFile(path)} title={path}>{labelForFile(path)}</Button>
                    {tabs.length > 1 && <Button size="xs" variant="ghost" type="button" aria-label={'关闭 ' + labelForFile(path)} onClick={() => {
                      const remaining = tabs.filter(tab => tab !== path)
                      setTabs(remaining)
                      if (file === path) openFile(remaining.at(-1) ?? draft.entry)
                    }}><X size={12} /></Button>}
                  </div>)}
                </div>
                {fileOperation && <form className="collector-file-form" onSubmit={event => { event.preventDefault(); updateFiles() }}>
                  <label>{fileOperation === 'delete' ? '删除 ' + (selected?.name ?? '') : fileOperation === 'rename' ? '重命名' : fileOperation === 'folder' ? '新文件夹' : '新文件'}
                    {fileOperation !== 'delete' && <Input variant="sm" key={fileOperation} autoFocus required value={fileName} onChange={event => setFileName(event.target.value)} />}
                  </label>
                  <Button size="xs" variant="ghost" onClick={() => setFileOperation(undefined)}>取消</Button>
                  <Button size="xs" type="submit" disabled={Boolean(busy)}>{fileOperation === 'delete' ? '确认删除' : '确定'}</Button>
                </form>}
                <CollectorCode draft={editorDraft ?? draft} file={file} value={activeText ?? ''} readOnly={readOnly || Boolean(busy)} jumpLine={jumpLine} onChange={editContent} />
                <div className="collector-editor-status">
                  <span>{file}</span>
                  <span>{readOnly ? '内置 · 只读' : saveError ? '保存失败' : draft === savedRef.current ? '已保存' : '自动保存'}</span>
                </div>
              </div>
              <section className="collector-results" aria-label="试跑结果">
                <div className="collector-results-bar">
                  <strong><Terminal size={14} />试跑结果</strong>
                  <div>
                  <Button size="xs" variant="ghost" type="button" aria-pressed={resultView === 'sample'} onClick={() => setResultView(resultView === 'sample' ? 'preview' : 'sample')}>样例数据</Button>
                  {!readOnly && <Button size="xs" variant="ghost" type="button" disabled={Boolean(busy)} onClick={() => void runTest(true)}>用实际资源试跑</Button>}
                  {test?.result && <>
                    <Button size="xs" variant="ghost" type="button" aria-pressed={resultView === 'preview'} onClick={() => setResultView('preview')}>卡片预览</Button>
                    <Button size="xs" variant="ghost" type="button" aria-pressed={resultView === 'output'} onClick={() => setResultView('output')}>JSON 与日志</Button>
                  </>}
                  </div>
                </div>
                {resultView === 'sample' ? <div className="collector-sample">
                  {hasSampleFile ? <p>离线试跑使用项目文件 <Button size="xs" variant="ghost" type="button" onClick={() => openFile(samplePath)}>{samplePath}</Button>。</p>
                    : <>
                      <p>填写模拟响应供离线试跑，实际采集使用已授权的资源。</p>
                      <CollectorCode draft={editorDraft ?? draft} file={samplePath} value={draft.fixture}
                        readOnly={readOnly || Boolean(busy)} onChange={text => {
                          const current = draftRef.current
                          if (current && !readOnly && !busy && !navigating.current) editable({ ...current, fixture: text })
                        }} />
                    </>}
                </div> : test?.error ? <div className="collector-result-error" role="alert">
                  <pre>{test.error}</pre>
                  {testLocation && <Button size="xs" variant="ghost" type="button" onClick={() => openFile(testLocation.path, testLocation.line)}>跳到错误位置 →</Button>}
                  {sampleError && !hasSampleFile && <Button size="xs" variant="ghost" type="button" onClick={() => setResultView('sample')}>打开样例数据 →</Button>}
                  {test.logs.length > 0 && <pre>{test.logs.join('\n')}</pre>}
                </div> : test?.result ? <div className="collector-result-content">
                  <div className={passed ? 'collector-success' : ''}>{passed ? '试跑通过' : '这是上一份草稿的试跑结果'}</div>
                  {resultView === 'preview' ? <div className="collector-preview">{preview}</div>
                    : <pre>{JSON.stringify(test.result.outputs, null, 2)}{test.logs.length ? '\n\n' + test.logs.join('\n') : ''}</pre>}
                </div> : <div className="collector-result-empty">{busy === 'test' ? '正在隔离运行器中执行…' : '点击「测试运行」，使用样例数据检查读数和卡片。'}</div>}
              </section>
            </div>
        </> : <div className="collector-start">
          <h2>选择一个模块</h2>
          <p>在左侧查看和试跑内置模块，或从模板新建采集器。</p>
        </div>}
      </section>
    </div>
    {grantPrompt && <CollectorAuthorization requests={grantPrompt.requests} live={grantPrompt.live} onFinish={finishGrantPrompt} />}
    {showCodexSettings && <CollectorCodexSettings onDismiss={() => setShowCodexSettings(false)} />}
  </main>
}

function buildPreview(draft: Draft, outputs: NonNullable<TestResult['result']>['outputs']) {
  const definition = readDefinition(draft.definition)
  return Object.entries(definition.widgets ?? {}).map(([key, spec]) => {
    const output = outputs[spec.outputKey]
    const widget: Widget = {
      id: key, group: draft.id, name: spec.label, shortName: spec.shortLabel ?? spec.label,
      unit: definition.outputs?.[spec.outputKey]?.unit ?? '', sizes: [[1, 1]], kind: spec.kind,
      stateLabels: spec.stateLabels, accentColor: spec.accentColor,
    }
    let reading: Reading | undefined = output ? { ...output, kind: spec.kind } : undefined
    if (reading && spec.kind === 'chart' && spec.chart) reading = {
      ...reading,
      values: [...Array<null>(spec.chart.sampleCount - 1).fill(null), reading.value ?? null],
      min: spec.chart.min,
      max: spec.chart.max,
    }
    if (reading && spec.range?.mode === 'fixed' && spec.range.min !== undefined && spec.range.max !== undefined) {
      reading = { ...reading, bounds: { min: spec.range.min, max: spec.range.max } }
    }
    return <span className="screen-widget is-narrow" key={key}><WidgetReading widget={widget} width={1} value={reading} /></span>
  })
}
