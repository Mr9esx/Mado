// Capture the native bridge before loading any project code.
((call, instanceId = null) => {
  const stringify = JSON.stringify
  const parse = JSON.parse
  const invoke = async (operation, alias, params) => {
    const reply = parse(call(stringify({ operation, alias, params }), instanceId))
    if (reply.Err !== undefined) throw new Error(reply.Err)
    return reply.Ok
  }
  const request = (alias, options = {}) => invoke('http', alias, options)
  return {
    http: {
      request,
      async getJson(alias, path = '') {
        const { status, text } = await request(alias, { path })
        if (status < 200 || status >= 300) throw new Error('HTTP ' + status)
        return parse(text)
      },
    },
    files: {
      list: (alias, path = '') => invoke('list', alias, { path }),
      readText: (alias, path = '') => invoke('readText', alias, { path }),
      find: (alias, options) => invoke('findFiles', alias, options),
      readWindow: (alias, path, options) => invoke('readWindow', alias, { ...options, path }),
    },
    process: {
      run: (alias, args = []) => invoke('run', alias, { args }),
      request: (alias, method, params) => invoke('rpc', alias, { method, params }),
      notify: (alias, method, params) => invoke('notify', alias, { method, params }),
      receive: (alias, method, timeoutMs = 1000) => invoke('receive', alias, { method, timeoutMs }),
    },
    interaction: {
      openUrl: (alias, url) => invoke('openUrl', alias, { url }),
    },
    system: {
      read: alias => invoke('system', alias, {}),
    },
  }
})
