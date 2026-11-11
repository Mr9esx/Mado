use super::{
    model::{path_valid, Collected, Definition, MAX_PROJECT},
    process::ManagedChild,
};
use rquickjs::{
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
    CatchResultExt, Context, Ctx, Error, Function, Module, Object, Promise, Runtime,
};
use serde::{Deserialize, Serialize};
use serde_json::{value::RawValue, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SDK: &str = "@mado/sdk";
const LEGACY_SDK: &str = "@tdisplay/sdk";
const RESPONSE_LIMIT: usize = 128 * 1024;
const REQUEST_LIMIT: usize = MAX_PROJECT * 8;
const CALL_LIMIT: usize = 32 * 1024;
const REPLY_LIMIT: usize = 6 * 1024 * 1024;
const MAX_CALLS: usize = 24;
pub const RUNNER_FLAG: &str = "--collector-runner";

#[derive(Deserialize, Serialize)]
pub struct Request {
    pub entry: String,
    pub files: BTreeMap<String, String>,
    pub inputs: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_settings: Vec<String>,
    pub sampled_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub state: Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultData {
    pub result: Option<Collected>,
    pub logs: Vec<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub definition: Option<Definition>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Call {
    pub operation: String,
    pub alias: String,
    pub params: Value,
}

#[derive(Deserialize, Serialize)]
enum Message {
    Call(Call),
    InstanceCall { id: String, call: Call },
    Done(ResultData),
    BatchDone(BatchRun),
    Definition(InspectedDefinition),
}

#[derive(Deserialize, Serialize)]
struct Job {
    #[serde(flatten)]
    request: Request,
    #[serde(default)]
    inspect: bool,
    #[serde(default)]
    batch: bool,
}

enum Entry {
    Collect,
    Batch,
    Inspect,
}

type Reply = Result<Value, String>;
type InspectedDefinition = Result<Box<RawValue>, String>;
pub(super) type BatchResults = BTreeMap<String, Result<Collected, String>>;

#[derive(Deserialize, Serialize)]
pub(super) struct BatchRun {
    pub results: Result<BatchResults, String>,
    pub logs: Vec<String>,
}

struct ProjectResolver;

impl Resolver for ProjectResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let invalid = || {
            Error::new_resolving_message(base, name, "仅支持项目内的相对 JS 路径和 @mado/sdk")
        };
        if attributes.is_some() {
            return Err(invalid());
        }
        if matches!(name, SDK | LEGACY_SDK) {
            return Ok(SDK.into());
        }
        if !(name.starts_with("./") || name.starts_with("../")) {
            return Err(invalid());
        }
        let mut parts: Vec<_> = base
            .rsplit_once('/')
            .map(|(parent, _)| parent.split('/').collect())
            .unwrap_or_default();
        for part in name.split('/') {
            match part {
                "." => {}
                ".." => {
                    parts.pop().ok_or_else(invalid)?;
                }
                _ => parts.push(part),
            }
        }
        let path = parts.join("/");
        if !path_valid(&path) || !(path.ends_with(".mjs") || path.ends_with(".js")) {
            return Err(invalid());
        }
        Ok(path)
    }
}

struct ProjectLoader(BTreeMap<String, String>);

impl Loader for ProjectLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        let source = if name == SDK {
            "export function defineModule(definition) { return definition }"
        } else {
            self.0
                .get(name)
                .ok_or_else(|| Error::new_loading_message(name, "找不到项目文件"))?
        };
        Module::declare(ctx.clone(), name, source)
    }
}

fn run<T: serde::de::DeserializeOwned>(
    request: Request,
    entry_kind: Entry,
    logs: Rc<RefCell<Vec<String>>>,
    hidden: Rc<Vec<String>>,
    broker: impl FnMut(Option<String>, Call) -> Reply + 'static,
) -> Result<T, String> {
    let runtime = Runtime::new().map_err(|error| error.to_string())?;
    let start = Instant::now();
    let max_time = if request.action.is_some() { 185 } else { 12 };
    let max_calls = if request.action.is_some() {
        200
    } else {
        MAX_CALLS
    };
    let host_time = Rc::new(Cell::new(Duration::ZERO));
    let waited = Rc::clone(&host_time);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        let elapsed = start.elapsed();
        elapsed >= Duration::from_secs(max_time)
            || elapsed.saturating_sub(waited.get()) >= Duration::from_secs(2)
    })));
    runtime.set_memory_limit(32 * 1024 * 1024);
    runtime.set_max_stack_size(256 * 1024);
    let entry = request
        .files
        .get(&request.entry)
        .ok_or("找不到入口文件。")?
        .clone();
    runtime.set_loader(ProjectResolver, ProjectLoader(request.files));
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    let inputs = serde_json::to_string(&request.inputs).map_err(|error| error.to_string())?;
    let encoded = context.with(|ctx| {
        let run = || -> rquickjs::Result<String> {
            if matches!(entry_kind, Entry::Inspect) {
                let extract: Function = ctx.eval(
                    r#"(stringify => source => {
                      if (!source || typeof source.collect !== 'function') {
                        throw new Error('入口须默认导出 defineModule({ …, collect })')
                      }
                      if (source.instances?.batch && typeof source.collectBatch !== 'function') {
                        throw new Error('启用批量采集时须实现 collectBatch')
                      }
                      return stringify({
                        name: source.name, schedule: source.schedule, instances: source.instances, settings: source.settings,
                        inputs: source.inputs, permissions: source.permissions, resources: source.resources,
                        outputs: source.outputs, widgets: source.widgets, actions: source.actions,
                        suggestedBindings: source.suggestedBindings,
                      })
                    })(JSON.stringify)"#
                )?;
                let (module, load) = Module::declare(ctx.clone(), request.entry, entry)?.eval()?;
                load.finish::<()>()?;
                let definition: rquickjs::Value = module.get("default")?;
                return extract.call((definition,));
            }
            let invoke: Function = ctx.eval(
                "(function (stringify) { return async (collect, inputs, context) => stringify(await collect(inputs, context)) })(JSON.stringify)"
            )?;
            let args = ctx.json_parse(inputs)?;
            let host_factory: Function = ctx.eval(include_str!("../../../shared/collectors/host.mjs"))?;
            let broker = RefCell::new(broker);
            let calls = std::cell::Cell::new(0);
            let bridge = Function::new(ctx.clone(), move |encoded: String, instance_id: Option<String>| {
                let reply = (|| {
                    if calls.get() >= max_calls || encoded.len() > CALL_LIMIT {
                        return Err("宿主调用超过次数或大小限制。".into());
                    }
                    calls.set(calls.get() + 1);
                    let call = serde_json::from_str(&encoded).map_err(|_| "宿主调用参数无效。")?;
                    let start = Instant::now();
                    let result = broker.borrow_mut()(instance_id, call);
                    host_time.set(host_time.get() + start.elapsed());
                    result
                })();
                let mut bytes = serde_json::to_string(&reply).unwrap_or_default();
                if bytes.len() > REPLY_LIMIT {
                    bytes = r#"{"Err":"宿主返回数据过大。"}"#.into();
                }
                bytes
            })?;
            let host: Object = if matches!(entry_kind, Entry::Batch) {
                let host = Object::new(ctx.clone())?;
                let scoped_factory: Function = ctx.eval("(factory, bridge) => id => factory(bridge, id)")?;
                let for_instance: Function = scoped_factory.call((host_factory, bridge))?;
                host.set("forInstance", for_instance)?;
                host
            } else {
                host_factory.call((bridge,))?
            };
            host.set("sampledAtUnixMs", request.sampled_at as f64)?;
            host.set("state", ctx.json_parse(serde_json::to_string(&request.state).unwrap())?)?;
            host.set("log", Function::new(ctx.clone(), move |message: String| {
                let mut logs = logs.borrow_mut();
                if logs.len() < 40 {
                    logs.push(safe_message(message, &hidden, 400));
                }
            })?)?;
            let (module, load) = Module::declare(ctx.clone(), request.entry, entry)?.eval()?;
            load.finish::<()>()?;
            let definition: Object = module.get("default")?;
            let collect: Function = if let Some(action) = &request.action {
                let handlers: Object = definition.get("handlers")?;
                handlers.get(action.as_str())?
            } else if matches!(entry_kind, Entry::Batch) {
                definition.get("collectBatch")?
            } else {
                definition.get("collect")?
            };
            let result: Promise = invoke.call((collect, args, host))?;
            result.finish()
        };
        run().catch(&ctx).map_err(|error| error.to_string())
    })?;
    if encoded.len() > RESPONSE_LIMIT / 2 {
        return Err("采集输出过大。".into());
    }
    serde_json::from_str(&encoded).map_err(|error| format!("返回值不符合 SDK 契约：{error}"))
}

#[cfg(test)]
pub fn evaluate(request: Request) -> ResultData {
    evaluate_with(request, |_| Err("未授予此宿主能力。".into()))
}

pub fn evaluate_with(request: Request, broker: impl FnMut(Call) -> Reply + 'static) -> ResultData {
    let hidden = private_values(&request, std::iter::once(&request.inputs));
    let logs = Rc::new(RefCell::new(Vec::new()));
    let mut broker = broker;
    let result = run::<Collected>(
        request,
        Entry::Collect,
        Rc::clone(&logs),
        Rc::clone(&hidden),
        move |_, call| broker(call),
    );
    let logs = logs.take();
    match result {
        Ok(mut result) => {
            redact_status(&mut result, &hidden);
            ResultData {
                result: Some(result),
                logs,
                error: None,
                definition: None,
            }
        }
        Err(error) => ResultData {
            result: None,
            logs,
            error: Some(safe_message(error, &hidden, 3000)),
            definition: None,
        },
    }
}

fn private_values<'a>(
    request: &Request,
    inputs: impl IntoIterator<Item = &'a Value>,
) -> Rc<Vec<String>> {
    Rc::new(
        inputs
            .into_iter()
            .flat_map(|input| {
                request
                    .private_settings
                    .iter()
                    .filter_map(move |key| input.get(key)?.as_str())
                    .filter(|value| !value.is_empty())
                    .flat_map(|value| {
                        let quoted = Value::String(value.into()).to_string();
                        [value.to_owned(), quoted[1..quoted.len() - 1].to_owned()]
                    })
            })
            .collect(),
    )
}

fn redact_status(result: &mut Collected, hidden: &[String]) {
    if !hidden.is_empty() && echoes_private_setting(&result.status.to_string(), hidden) {
        result.status = Value::Null;
    }
}

#[derive(Deserialize)]
struct BatchReply {
    results: Vec<Value>,
}

fn evaluate_batch(
    request: Request,
    broker: impl FnMut(String, Call) -> Reply + 'static,
) -> BatchRun {
    let logs = Rc::new(RefCell::new(Vec::new()));
    let results = run_batch(request, Rc::clone(&logs), broker);
    BatchRun {
        results,
        logs: logs.take(),
    }
}

fn run_batch(
    request: Request,
    logs: Rc<RefCell<Vec<String>>>,
    mut broker: impl FnMut(String, Call) -> Reply + 'static,
) -> Result<BatchResults, String> {
    let instances = request.inputs.as_array().ok_or("批量采集需要实例数组。")?;
    let hidden = private_values(
        &request,
        instances.iter().map(|instance| &instance["settings"]),
    );
    let mut results = BTreeMap::new();
    for instance in instances {
        let id = instance["id"].as_str().ok_or("批量采集实例缺少 ID。")?;
        results.insert(id.to_owned(), None);
    }
    let reply = run::<BatchReply>(
        request,
        Entry::Batch,
        logs,
        Rc::clone(&hidden),
        move |id, call| broker(id.ok_or("请指定已授权的采集实例。")?, call),
    )
    .map_err(|error| safe_message(error, &hidden, 3000))?;
    for row in reply.results {
        let Some(slot) = row["instanceId"]
            .as_str()
            .and_then(|id| results.get_mut(id))
        else {
            continue;
        };
        if slot.is_some() {
            *slot = Some(Err("批量采集返回了重复的实例结果。".into()));
            continue;
        }
        let result = if let Some(error) = row["error"].as_str() {
            Err(safe_message(error.to_owned(), &hidden, 3000))
        } else {
            serde_json::from_value::<Collected>(row)
                .map(|mut result| {
                    redact_status(&mut result, &hidden);
                    result
                })
                .map_err(|_| "实例结果不符合 SDK 契约。".into())
        };
        *slot = Some(result);
    }
    Ok(results
        .into_iter()
        .map(|(id, result)| {
            (
                id,
                result.unwrap_or_else(|| Err("批量采集未返回此实例的结果。".into())),
            )
        })
        .collect())
}

fn echoes_private_setting(message: &str, hidden: &[String]) -> bool {
    hidden.iter().any(|value| message.contains(value))
}

fn safe_message(message: String, hidden: &[String], limit: usize) -> String {
    if echoes_private_setting(&message, hidden) {
        const REDACTED: &str = "已隐藏包含变量值的采集消息。";
        if echoes_private_setting(REDACTED, hidden) {
            return String::new();
        }
        return REDACTED.into();
    }
    message.chars().take(limit).collect()
}

pub(super) fn inspect_code(request: Request) -> InspectedDefinition {
    run(
        request,
        Entry::Inspect,
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(Vec::new()),
        |_, _| Err("读取模块定义时禁止访问宿主资源。".into()),
    )
    .map_err(|error| error.chars().take(3000).collect())
}

pub fn inspect(request: Request) -> InspectedDefinition {
    match exchange(
        Job {
            request,
            inspect: true,
            batch: false,
        },
        |_, _| Err("读取模块定义时禁止访问宿主资源。".into()),
    )? {
        Message::Definition(result) => result,
        _ => Err("未收到模块定义。".into()),
    }
}

pub fn read_message<T: serde::de::DeserializeOwned>(
    reader: &mut impl BufRead,
    limit: usize,
) -> Result<T, String> {
    serde_json::from_slice(&read_line(reader, limit)?).map_err(|error| error.to_string())
}

pub(super) fn read_line(reader: &mut impl BufRead, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.last() != Some(&b'\n') || bytes.len() > limit {
        return Err("进程消息不完整或超出大小限制。".into());
    }
    Ok(bytes)
}

fn send_message(writer: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| error.to_string())?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|error| error.to_string())
}

pub fn run_child() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = std::io::stdin().lock();
    let Job {
        request,
        inspect,
        batch,
    } = read_message(&mut input, REQUEST_LIMIT)?;
    if inspect {
        return send_message(
            &mut std::io::stdout().lock(),
            &Message::Definition(inspect_code(request)),
        )
        .map_err(Into::into);
    }
    if batch {
        let mut output = std::io::stdout().lock();
        let result = evaluate_batch(request, move |id, call| {
            send_message(&mut output, &Message::InstanceCall { id, call })?;
            read_message::<Reply>(&mut input, REPLY_LIMIT)?
        });
        return send_message(&mut std::io::stdout().lock(), &Message::BatchDone(result))
            .map_err(Into::into);
    }
    let mut output = std::io::stdout().lock();
    let result = evaluate_with(request, move |call| {
        send_message(&mut output, &Message::Call(call))?;
        read_message::<Reply>(&mut input, REPLY_LIMIT)?
    });
    send_message(&mut std::io::stdout().lock(), &Message::Done(result))?;
    Ok(())
}

pub fn isolated(request: Request) -> Result<ResultData, String> {
    isolated_with(request, |_| Err("未授予此宿主能力。".into()))
}

pub fn isolated_with(
    request: Request,
    mut broker: impl FnMut(Call) -> Reply + Send + 'static,
) -> Result<ResultData, String> {
    match exchange(
        Job {
            request,
            inspect: false,
            batch: false,
        },
        move |id, call| {
            if id.is_none() {
                broker(call)
            } else {
                Err("此任务不支持实例调用。".into())
            }
        },
    )? {
        Message::Done(result) => Ok(result),
        _ => Err("未收到采集结果。".into()),
    }
}

pub(super) fn isolated_batch(
    request: Request,
    mut broker: impl FnMut(String, Call) -> Reply + Send + 'static,
) -> Result<BatchRun, String> {
    match exchange(
        Job {
            request,
            inspect: false,
            batch: true,
        },
        move |id, call| broker(id.ok_or("请指定已授权的采集实例。")?, call),
    )? {
        Message::BatchDone(result) => Ok(result),
        _ => Err("未收到批量采集结果。".into()),
    }
}

fn exchange(
    job: Job,
    mut broker: impl FnMut(Option<String>, Call) -> Reply + Send + 'static,
) -> Result<Message, String> {
    let interactive = job.request.action.is_some();
    let request = serde_json::to_vec(&job).map_err(|error| error.to_string())?;
    if request.len() >= REQUEST_LIMIT {
        return Err("项目或输入过大。".into());
    }
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(program);
    command
        .arg(RUNNER_FLAG)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = ManagedChild::start(&mut command)
        .map_err(|error| format!("无法启动采集运行器：{error}"))?;
    let mut input = child.stdin();
    let mut output = BufReader::new(child.stdout());
    let (send, receive) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut exchange = || {
            input
                .write_all(&request)
                .and_then(|()| input.write_all(b"\n"))
                .map_err(|error| error.to_string())?;
            for _ in 0..=if interactive { 200 } else { MAX_CALLS } {
                let (id, call) = match read_message(&mut output, RESPONSE_LIMIT)? {
                    Message::Call(call) => (None, call),
                    Message::InstanceCall { id, call } => (Some(id), call),
                    message => return Ok(message),
                };
                let reply = broker(id, call);
                let size = serde_json::to_vec(&reply)
                    .map_err(|error| error.to_string())?
                    .len();
                if size < REPLY_LIMIT {
                    send_message(&mut input, &reply)?;
                } else {
                    send_message(&mut input, &Reply::Err("宿主返回数据过大。".into()))?;
                }
            }
            Err("宿主调用超过次数限制。".into())
        };
        let _ = send.send(exchange());
    });
    let result = receive
        .recv_timeout(Duration::from_secs(if interactive { 186 } else { 13 }))
        .map_err(|_| "采集运行超时或异常退出。".to_string())
        .and_then(|result| result);
    // The result completes the protocol. Reap the runner even if it fails to exit.
    drop(child);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn execute(source: &str) -> ResultData {
        evaluate(Request {
            entry: "src/index.mjs".into(),
            files: BTreeMap::from([
                ("src/index.mjs".into(), source.into()),
                ("src/helper.mjs".into(), "export const value = 42".into()),
            ]),
            inputs: json!({}),
            private_settings: Vec::new(),
            sampled_at: 1,
            action: None,
            state: Value::Null,
        })
    }

    #[test]
    fn loads_project_dependencies_and_awaits_the_collector() {
        let result = execute("import { defineModule } from '@mado/sdk'; import { value } from './helper.mjs';
            export default defineModule({ async collect(_, context) { context.log('ok'); return { outputs: { value: { value } } } } });");
        assert_eq!(result.result.unwrap().outputs["value"]["value"], 42);
        assert_eq!(result.logs, ["ok"]);
    }

    #[test]
    fn batch_routes_host_calls_and_keeps_failures_with_their_instance() {
        let source = r#"
            export default { async collectBatch(instances, context) {
              const results = [];
              for (const { id, settings, state } of instances) {
                try {
                  const reading = await context.forInstance(id).system.read('metrics');
                  results.push({ instanceId: id, outputs: { value: { value: reading.value } },
                    state: { previous: state.previous, name: settings.NAME } });
                } catch (error) {
                  results.push({ instanceId: id, error: error.message + ' ' + settings.NAME });
                }
              }
              return { results };
            } };
        "#;
        let run = evaluate_batch(
            Request {
                entry: "index.mjs".into(),
                files: BTreeMap::from([("index.mjs".into(), source.into())]),
                inputs: json!([
                    {"id":"one","settings":{"NAME":"private-alpha"},"state":{"previous":1}},
                    {"id":"two","settings":{"NAME":"private-beta"},"state":{"previous":2}},
                    {"id":"three","settings":{"NAME":"private-gamma"},"state":{"previous":3}}
                ]),
                private_settings: vec!["NAME".into()],
                sampled_at: 100,
                action: None,
                state: Value::Null,
            },
            |id, call| {
                assert_eq!(call.alias, "metrics");
                match id.as_str() {
                    "one" => Ok(json!({"value":10})),
                    "three" => Ok(json!({"value":30})),
                    _ => Err("资源异常".into()),
                }
            },
        );
        let results = run.results.unwrap();
        let first = results["one"].as_ref().unwrap();
        assert_eq!(first.outputs["value"]["value"], 10);
        assert_eq!(first.state["previous"], 1);
        assert!(!results["two"]
            .as_ref()
            .unwrap_err()
            .contains("private-beta"));
        assert_eq!(
            results["three"].as_ref().unwrap().outputs["value"]["value"],
            30
        );
    }

    #[test]
    fn reads_definition_from_the_entry_and_its_imports_without_collecting() {
        let encoded = inspect_code(Request {
            entry: "src/index.mjs".into(),
            files: BTreeMap::from([
                (
                    "src/index.mjs".into(),
                    r#"
                    import { defineModule } from '@tdisplay/sdk'
                    import { schedule } from './schedule.mjs'
                    export default defineModule({
                        name: "Test", schedule, settings: { SECOND: { label: '二', required: false }, FIRST: { label: '一', required: false } },
                        permissions: {}, resources: {},
                        outputs: {}, widgets: {}, collect() { throw new Error("must not run") }
                    })
                "#
                    .into(),
                ),
                (
                    "src/schedule.mjs".into(),
                    "export const schedule = { refreshSeconds: 19, enabled: true }".into(),
                ),
            ]),
            inputs: Value::Null,
            private_settings: Vec::new(),
            sampled_at: 0,
            action: None,
            state: Value::Null,
        })
        .unwrap();
        let definition: Definition = serde_json::from_str(encoded.get()).unwrap();
        assert_eq!(
            definition
                .settings
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["SECOND", "FIRST"]
        );
        let result: Value = serde_json::from_str(encoded.get()).unwrap();
        assert_eq!(result["name"], "Test");
        assert_eq!(result["schedule"]["refreshSeconds"], 19);
        assert!(result.get("collect").is_none());
    }

    #[test]
    fn refuses_host_modules_and_paths_outside_the_project() {
        for specifier in [
            "os",
            "node:fs",
            "../../outside.mjs",
            "https://example.org/script.mjs",
        ] {
            let result = execute(&format!("import '{specifier}'; export default {{}};"));
            assert!(result.error.is_some(), "{specifier}");
        }
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn script_chooses_which_host_calls_to_make() {
        let source = r#"
          export default { async collect(_, context) {
            const status = await context.http.getJson('metrics', '/status')
            const result = await context.http.getJson('metrics', status.path)
            return { outputs: { reading: { value: result.value } } }
          } }
        "#;
        let paths = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&paths);
        let result = evaluate_with(
            Request {
                entry: "index.mjs".into(),
                files: BTreeMap::from([("index.mjs".into(), source.into())]),
                inputs: json!({}),
                private_settings: Vec::new(),
                sampled_at: 0,
                action: None,
                state: Value::Null,
            },
            move |call| {
                let mut paths = observed.borrow_mut();
                paths.push(call.params["path"].as_str().unwrap().to_owned());
                Ok(
                    json!({ "status": 200, "text": if paths.len() == 1 { r#"{"path":"/detail"}"# } else { r#"{"value":43}"# } }),
                )
            },
        );
        assert_eq!(*paths.borrow(), ["/status", "/detail"]);
        assert_eq!(result.result.unwrap().outputs["reading"]["value"], 43);
    }
}
