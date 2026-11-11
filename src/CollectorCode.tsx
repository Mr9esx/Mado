import { acceptCompletion, autocompletion, closeBrackets, closeBracketsKeymap, snippetCompletion, type Completion, type CompletionContext } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { javascript } from '@codemirror/lang-javascript'
import { json } from '@codemirror/lang-json'
import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from '@codemirror/language'
import { EditorState, Prec } from '@codemirror/state'
import { EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from '@codemirror/view'
import { tags } from '@lezer/highlight'
import { useEffect, useLayoutEffect, useRef } from 'react'
import { readDefinition, type Draft } from './collectorApi'

const studioSyntax = HighlightStyle.define([
  { tag: [tags.keyword, tags.operatorKeyword, tags.definitionKeyword], class: 'studio-syntax-keyword' },
  { tag: [tags.string, tags.special(tags.string)], class: 'studio-syntax-string' },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], class: 'studio-syntax-literal' },
  { tag: [tags.propertyName, tags.function(tags.variableName)], class: 'studio-syntax-name' },
  { tag: tags.comment, class: 'studio-syntax-comment' },
  { tag: tags.invalid, class: 'studio-syntax-invalid' },
])

function completion(context: CompletionContext, file: string, draft: Draft) {
  const definition = readDefinition(draft.definition)
  const before = context.state.sliceDoc(0, context.pos)
  const token = context.matchBefore(/[\w@./-]*/)
  if (!token || (token.from === token.to && !context.explicit && !/["'.]$/.test(before))) return null
  const v2 = draft.sdkApiVersion === 2
  const properties = (labels: string[]) => labels.map(label => ({ label, type: 'property' }))
  const from = context.pos - (before.match(/[\w-]*$/)?.[0].length ?? 0)
  let options: Completion[]
  if (file === draft.entry && /["']?(kind|valueType|outputKey)["']?\s*:\s*["'][\w-]*$/.test(before)) {
    if (/["']?kind["']?\s*:\s*["'][\w-]*$/.test(before)) {
      options = [...(v2 ? ['http', 'file', 'directory', 'process', 'stdio', 'system'] : ['httpJson', 'fileJson']), 'metric', 'progress', 'state', 'chart', 'trend', 'range'].map(label => ({ label, type: 'enum' }))
    } else if (/["']?valueType["']?\s*:\s*["'][\w-]*$/.test(before)) {
      options = ['number', 'deadline', 'progress', 'state'].map(label => ({ label, type: 'enum' }))
    } else if (/["']?outputKey["']?\s*:\s*["'][\w-]*$/.test(before)) {
      options = Object.keys(definition.outputs ?? {}).map(label => ({ label, type: 'property' }))
    } else {
      return null
    }
  } else if (file === 'fixtures/sample.json') {
    options = properties(v2 ? ['settings', 'calls', 'operation', 'alias', 'params', 'result'] : Object.keys(definition.inputs ?? {}))
  } else if (/\b(?:from|import)\s*['"][^'"]*$/.test(before)) {
    const parent = file.split('/').slice(0, -1)
    options = ['@mado/sdk', ...Object.keys(draft.files).filter(path => path !== file && /\.m?js$/.test(path)).map(path => {
      const pieces = path.split('/')
      let common = 0
      while (common < parent.length && parent[common] === pieces[common]) common++
      return (common === parent.length ? './' : '../'.repeat(parent.length - common)) + pieces.slice(common).join('/')
    })].map(label => ({ label, type: 'namespace' }))
  } else if (v2 && /\bcontext\.(http|files|process|system)\.[\w]*$/.test(before)) {
    const namespace = before.match(/\bcontext\.(http|files|process|system)\.[\w]*$/)?.[1]
    const methods: Record<string, [string, string][]> = {
      http: [['request', "request('${alias}', { path: '/${path}' })"], ['getJson', "getJson('${alias}', '/${path}')"]],
      files: [['list', "list('${alias}', '${path}')"], ['readText', "readText('${alias}', '${path}')"]],
      process: [['run', "run('${alias}', [${args}])"], ['request', "request('${alias}', '${method}', {${}})"],
        ['notify', "notify('${alias}', '${method}')"]],
      system: [['read', "read('${alias}')"]],
    }
    return { from, options: methods[namespace ?? ''].map(([label, snippet]) => snippetCompletion(snippet, { label, type: 'method' })) }
  } else if (v2 && /\bcontext\.(http|files|process|system)\.\w+\(\s*['"][\w-]*$/.test(before)) {
    const namespace = before.match(/\bcontext\.(http|files|process|system)\.\w+\(\s*['"][\w-]*$/)?.[1]
    const kinds: Record<string, string[]> = { http: ['http'], files: ['file', 'directory'], process: ['process', 'stdio'], system: ['system'] }
    return { from, options: Object.entries(definition.permissions ?? {})
      .filter(([, permission]) => kinds[namespace ?? ''].includes(permission.kind))
      .map(([label, permission]) => ({ label, detail: permission.label, type: 'property' })) }
  } else if (/\bcontext\.[\w]*$/.test(before)) {
    return { from, options: [
      { label: 'sampledAtUnixMs', type: 'property', detail: '本次采样时间' },
      snippetCompletion('log(${message})', { label: 'log', type: 'method', detail: '输出试跑日志' }),
      ...properties(v2 ? ['http', 'files', 'process', 'system'] : []),
    ] }
  } else if (/\b(?:inputs|settings)\.[\w]*$/.test(before)) {
    const fields = v2 ? definition.settings : definition.inputs
    return { from, options: Object.entries(fields ?? {}).map(([label, field]) => ({ label, type: 'property', detail: field.label })) }
  } else {
    options = [
      snippetCompletion(v2
        ? "defineModule({\n  name: '${name}',\n  schedule: { refreshSeconds: 60, enabled: true },\n  outputs: { ${} },\n  widgets: {},\n  async collect(settings, context) {\n    return { outputs: { ${} } }\n  },\n})"
        : "defineModule({\n  name: '${name}',\n  outputs: { ${} },\n  widgets: {},\n  collect(inputs, context) {\n    return { outputs: { ${} } }\n  },\n})", {
        label: 'defineModule', type: 'function', detail: 'Mado SDK',
      }),
      ...(file === draft.entry ? properties(['name', 'schedule', 'refreshSeconds', 'enabled', 'resources',
        ...(v2 ? ['settings', 'permissions', 'default', 'methods', 'args'] : ['inputs']),
        'widgets', 'kind', 'label', 'valueType', 'unit', 'outputKey', 'supportedSpans', 'stateKeys',
        'stateLabels', 'chart', 'trend', 'range', 'format', 'accentColor', 'collect']) : []),
      ...Object.keys(definition.outputs ?? {}).map(label => ({ label, type: 'property', detail: '模块输出' })),
      ...Object.keys(definition.inputs ?? {}).map(label => ({ label, type: 'property', detail: '模块输入' })),
      ...['outputs', 'notifications', 'value', 'ratio', 'deadlineUnixMs', 'state', 'detail'].map(label => ({ label, type: 'property' })),
    ]
  }
  return { from: token.from, options }
}

export function CollectorCode({ draft, file, value, readOnly, jumpLine, onChange }: {
  draft: Draft
  file: string
  value: string
  readOnly: boolean
  jumpLine?: number
  onChange: (text: string) => void
}) {
  const target = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const states = useRef(new Map<string, EditorState>())
  const current = useRef({ draft, value, onChange })
  current.current = { draft, value, onChange }

  useLayoutEffect(() => {
    if (!target.current) return
    const key = draft.id + '/' + file + '/' + readOnly
    const saved = states.current.get(key)
    const state = saved?.doc.toString() === current.current.value ? saved : EditorState.create({
      doc: current.current.value,
      extensions: [
        file.endsWith('.json') ? json() : javascript(),
        lineNumbers(), history(), highlightActiveLine(), highlightActiveLineGutter(),
        bracketMatching(), closeBrackets(), indentOnInput(), syntaxHighlighting(studioSyntax), EditorView.lineWrapping,
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
        EditorView.contentAttributes.of({ 'aria-label': file === 'fixtures/sample.json' ? '样例 JSON 编辑器' : '采集器代码编辑器', spellcheck: 'false' }),
        autocompletion({ override: [context => completion(context, file, current.current.draft)] }),
        Prec.highest(keymap.of([{ key: 'Tab', run: acceptCompletion }])),
        keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap]),
        EditorView.updateListener.of(update => {
          if (update.docChanged) current.current.onChange(update.state.doc.toString())
        }),
      ],
    })
    const editor = new EditorView({ state, parent: target.current })
    view.current = editor
    return () => {
      states.current.set(key, editor.state)
      editor.destroy()
      view.current = null
    }
  }, [draft.id, file, readOnly])

  useEffect(() => {
    if (!jumpLine || !view.current) return
    const editor = view.current
    const line = editor.state.doc.line(Math.min(jumpLine, editor.state.doc.lines))
    editor.dispatch({ selection: { anchor: line.from }, scrollIntoView: true })
    editor.focus()
  }, [file, jumpLine])

  return <div className="collector-code" ref={target} />
}
