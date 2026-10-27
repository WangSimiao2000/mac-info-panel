use std::io::{self, Write};

use extism_pdk::{FnResult, Json, plugin_fn};
use piccolo::{
    Callback, CallbackReturn, Closure, Context, Executor, Fuel, Lua, StashedExecutor, StashedTable,
    Table, Value,
    stdlib::{load_base, load_math, load_string, load_table},
};
use serde::Deserialize;

const FUEL_SLICE: i32 = 12;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_NODES: usize = 16_384;
const MAX_ARRAY_INDEX: usize = MAX_JSON_NODES;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunnerCall {
    source: String,
    request: serde_json::Value,
    limits: RunnerLimits,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunnerLimits {
    max_instructions: u64,
    max_lua_memory_bytes: usize,
    max_stack_depth: usize,
    max_output_bytes: usize,
    max_host_calls: u32,
}

#[plugin_fn]
pub fn metadata(Json(call): Json<RunnerCall>) -> FnResult<Vec<u8>> {
    execute("metadata", call).map_err(Into::into)
}

#[plugin_fn]
pub fn validate_config(Json(call): Json<RunnerCall>) -> FnResult<Vec<u8>> {
    execute("validate_config", call).map_err(Into::into)
}

#[plugin_fn]
pub fn refresh(Json(call): Json<RunnerCall>) -> FnResult<Vec<u8>> {
    execute("refresh", call).map_err(Into::into)
}

fn execute(function_name: &'static str, call: RunnerCall) -> Result<Vec<u8>, extism_pdk::Error> {
    validate_call(&call)?;

    let (mut lua, null_marker, array_marker) = restricted_lua()?;
    let source_executor = lua
        .try_enter(|ctx| {
            let closure = Closure::load(ctx, Some("plugin.lua"), call.source.as_bytes())?;
            Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
        })
        .map_err(|_| runner_error("Lua source is invalid"))?;

    let mut remaining_instructions = call.limits.max_instructions;
    drive_executor(
        &mut lua,
        &source_executor,
        &mut remaining_instructions,
        call.limits,
    )?;
    lua.enter(|ctx| {
        match ctx
            .fetch(&source_executor)
            .take_result::<Option<Value>>(ctx)
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(runner_error("Lua source failed")),
            Err(_) => Err(runner_error("Lua source did not complete")),
        }
    })?;

    let call_executor = lua.enter(|ctx| {
        let function = match ctx.get_global(function_name) {
            Value::Function(function) => function,
            _ => return Err(runner_error("Lua entry point is missing")),
        };
        let null = ctx.fetch(&null_marker);
        let array = ctx.fetch(&array_marker);
        let mut nodes = MAX_JSON_NODES;
        let request = json_to_lua(ctx, &call.request, null, array, 0, &mut nodes)?;
        Ok(ctx.stash(Executor::start(ctx, function, request)))
    })?;

    drive_executor(
        &mut lua,
        &call_executor,
        &mut remaining_instructions,
        call.limits,
    )?;

    lua.enter(|ctx| {
        let result = ctx
            .fetch(&call_executor)
            .take_result::<Value>(ctx)
            .map_err(|_| runner_error("Lua entry point did not complete"))?
            .map_err(|_| runner_error("Lua entry point failed"))?;
        encode_lua_json(
            ctx,
            result,
            ctx.fetch(&null_marker),
            ctx.fetch(&array_marker),
            call.limits.max_output_bytes,
        )
    })
}

fn validate_call(call: &RunnerCall) -> Result<(), extism_pdk::Error> {
    if call.source.len() > MAX_SOURCE_BYTES
        || call.limits.max_instructions == 0
        || call.limits.max_lua_memory_bytes == 0
        || call.limits.max_stack_depth == 0
        || call.limits.max_output_bytes == 0
    {
        return Err(runner_error("Lua runner limits are invalid"));
    }
    if call.limits.max_host_calls != 0 {
        return Err(runner_error("Lua host calls are unavailable"));
    }
    Ok(())
}

fn restricted_lua() -> Result<(Lua, StashedTable, StashedTable), extism_pdk::Error> {
    let mut lua = Lua::empty();
    let (null_marker, array_marker) = lua.enter(|ctx| {
        load_base(ctx);
        load_math(ctx);
        load_string(ctx);
        load_table(ctx);

        for name in [
            "collectgarbage",
            "coroutine",
            "debug",
            "dofile",
            "getmetatable",
            "host",
            "io",
            "load",
            "loadfile",
            "os",
            "package",
            "rawget",
            "rawset",
            "require",
            "setmetatable",
        ] {
            ctx.set_global(name, Value::Nil)
                .expect("nil is a valid global value");
        }
        if let Value::Table(table) = ctx.get_global("table") {
            table
                .set(ctx, "unpack", Value::Nil)
                .expect("table.unpack is a valid table key");
        }

        let null = Table::new(&ctx);
        let array_metatable = Table::new(&ctx);
        let array_constructor =
            Callback::from_fn_with(&ctx, array_metatable, |marker, ctx, _, mut stack| {
                let table = Table::new(&ctx);
                table.set_metatable(&ctx, Some(*marker));
                stack.replace(ctx, table);
                Ok(CallbackReturn::Return)
            });
        ctx.set_global("json_null", null)
            .expect("json_null is a valid global");
        ctx.set_global("json_array", array_constructor)
            .expect("json_array is a valid global");
        (ctx.stash(null), ctx.stash(array_metatable))
    });
    if lua.total_memory() == 0 {
        return Err(runner_error("Lua VM did not initialize"));
    }
    Ok((lua, null_marker, array_marker))
}

fn drive_executor(
    lua: &mut Lua,
    executor: &StashedExecutor,
    remaining_instructions: &mut u64,
    limits: RunnerLimits,
) -> Result<(), extism_pdk::Error> {
    loop {
        check_runtime_limits(lua, executor, limits)?;
        if *remaining_instructions == 0 {
            return Err(runner_error("Lua instruction limit exceeded"));
        }

        let slice = u64::from(FUEL_SLICE as u32).min(*remaining_instructions) as i32;
        let mut fuel = Fuel::with(slice);
        let complete = lua.enter(|ctx| ctx.fetch(executor).step(ctx, &mut fuel));
        let consumed = slice.saturating_sub(fuel.remaining());
        if !complete && consumed == 0 {
            return Err(runner_error("Lua instruction fuel exhausted"));
        }
        *remaining_instructions =
            remaining_instructions.saturating_sub(u64::try_from(consumed).unwrap());

        check_runtime_limits(lua, executor, limits)?;
        if complete {
            return Ok(());
        }
    }
}

fn check_runtime_limits(
    lua: &mut Lua,
    executor: &StashedExecutor,
    limits: RunnerLimits,
) -> Result<(), extism_pdk::Error> {
    // Extism's WASM page ceiling is the hard pre-allocation memory boundary.
    // Piccolo metrics are a secondary detector between bounded fuel steps.
    if lua.total_memory() > limits.max_lua_memory_bytes {
        return Err(runner_error("Lua memory threshold exceeded"));
    }
    let stack_depth = lua.enter(|ctx| ctx.fetch(executor).stack_depth());
    if stack_depth >= limits.max_stack_depth {
        return Err(runner_error("Lua stack limit exceeded"));
    }
    Ok(())
}

fn json_to_lua<'gc>(
    ctx: Context<'gc>,
    value: &serde_json::Value,
    null_marker: Table<'gc>,
    array_marker: Table<'gc>,
    depth: usize,
    nodes: &mut usize,
) -> Result<Value<'gc>, extism_pdk::Error> {
    consume_json_node(depth, nodes)?;
    match value {
        serde_json::Value::Null => Ok(null_marker.into()),
        serde_json::Value::Bool(value) => Ok((*value).into()),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.into())
            } else if let Some(value) = value.as_u64() {
                i64::try_from(value)
                    .map(Value::Integer)
                    .map_err(|_| runner_error("JSON integer is outside Lua range"))
            } else {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .map(Value::Number)
                    .ok_or_else(|| runner_error("JSON number is invalid"))
            }
        }
        serde_json::Value::String(value) => Ok(ctx.intern(value.as_bytes()).into()),
        serde_json::Value::Array(values) => {
            let table = Table::new(&ctx);
            table.set_metatable(&ctx, Some(array_marker));
            for (index, value) in values.iter().enumerate() {
                let index = i64::try_from(index + 1)
                    .map_err(|_| runner_error("JSON array is too large"))?;
                table
                    .set(
                        ctx,
                        index,
                        json_to_lua(ctx, value, null_marker, array_marker, depth + 1, nodes)?,
                    )
                    .map_err(|_| runner_error("JSON array key is invalid"))?;
            }
            Ok(table.into())
        }
        serde_json::Value::Object(values) => {
            let table = Table::new(&ctx);
            for (key, value) in values {
                table
                    .set(
                        ctx,
                        key.clone(),
                        json_to_lua(ctx, value, null_marker, array_marker, depth + 1, nodes)?,
                    )
                    .map_err(|_| runner_error("JSON object key is invalid"))?;
            }
            Ok(table.into())
        }
    }
}

fn encode_lua_json<'gc>(
    ctx: Context<'gc>,
    value: Value<'gc>,
    null_marker: Table<'gc>,
    array_marker: Table<'gc>,
    limit: usize,
) -> Result<Vec<u8>, extism_pdk::Error> {
    let mut counter = CountingWriter::new(limit);
    let mut nodes = MAX_JSON_NODES;
    write_lua_json(
        &mut counter,
        ctx,
        value,
        null_marker,
        array_marker,
        0,
        false,
        &mut nodes,
        &mut Vec::new(),
    )?;

    let exact_len = counter.written();
    let mut output = LimitedWriter::new(exact_len);
    let mut nodes = MAX_JSON_NODES;
    write_lua_json(
        &mut output,
        ctx,
        value,
        null_marker,
        array_marker,
        0,
        false,
        &mut nodes,
        &mut Vec::new(),
    )?;
    if output.bytes.len() != exact_len {
        return Err(runner_error("Lua output size changed during encoding"));
    }
    Ok(output.bytes)
}

#[allow(clippy::too_many_arguments)]
fn write_lua_json<'gc, W: Write>(
    writer: &mut W,
    ctx: Context<'gc>,
    value: Value<'gc>,
    null_marker: Table<'gc>,
    array_marker: Table<'gc>,
    depth: usize,
    node_reserved: bool,
    nodes: &mut usize,
    ancestry: &mut Vec<Table<'gc>>,
) -> Result<(), extism_pdk::Error> {
    if !node_reserved {
        consume_json_node(depth, nodes)?;
    } else if depth > MAX_JSON_DEPTH {
        return Err(runner_error("Lua JSON structure limit exceeded"));
    }

    match value {
        Value::Nil => Err(runner_error("Lua result contains nil")),
        Value::Boolean(value) => write_json(writer, &value),
        Value::Integer(value) => write_json(writer, &value),
        Value::Number(value) if value.is_finite() => write_json(writer, &value),
        Value::Number(_) => Err(runner_error("Lua result contains a non-finite number")),
        Value::String(value) => {
            let value = std::str::from_utf8(value.as_bytes())
                .map_err(|_| runner_error("Lua result contains invalid UTF-8"))?;
            write_json(writer, &value)
        }
        Value::Table(table) if table == null_marker => write_raw(writer, b"null"),
        Value::Table(table) => write_lua_table(
            writer,
            ctx,
            table,
            null_marker,
            array_marker,
            depth,
            nodes,
            ancestry,
        ),
        _ => Err(runner_error("Lua result contains an unsupported value")),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_lua_table<'gc, W: Write>(
    writer: &mut W,
    ctx: Context<'gc>,
    table: Table<'gc>,
    null_marker: Table<'gc>,
    array_marker: Table<'gc>,
    depth: usize,
    nodes: &mut usize,
    ancestry: &mut Vec<Table<'gc>>,
) -> Result<(), extism_pdk::Error> {
    if ancestry.contains(&table) {
        return Err(runner_error("Lua result contains a table cycle"));
    }
    ancestry.push(table);
    let kind = inspect_table(table, array_marker, depth, nodes)?;

    let result = match kind {
        TableKind::Array(length) => {
            write_raw(writer, b"[")?;
            for index in 1..=length {
                if index > 1 {
                    write_raw(writer, b",")?;
                }
                write_lua_json(
                    writer,
                    ctx,
                    table.get(ctx, index as i64),
                    null_marker,
                    array_marker,
                    depth + 1,
                    true,
                    nodes,
                    ancestry,
                )?;
            }
            write_raw(writer, b"]")
        }
        TableKind::Object => {
            write_raw(writer, b"{")?;
            let mut first = true;
            for (key, value) in table.iter() {
                let Value::String(key) = key else {
                    return Err(runner_error("Lua object contains a non-string key"));
                };
                if !first {
                    write_raw(writer, b",")?;
                }
                first = false;
                let key = std::str::from_utf8(key.as_bytes())
                    .map_err(|_| runner_error("Lua object key is invalid UTF-8"))?;
                write_json(writer, &key)?;
                write_raw(writer, b":")?;
                write_lua_json(
                    writer,
                    ctx,
                    value,
                    null_marker,
                    array_marker,
                    depth + 1,
                    true,
                    nodes,
                    ancestry,
                )?;
            }
            write_raw(writer, b"}")
        }
    };
    ancestry.pop();
    result
}

#[derive(Clone, Copy)]
enum TableKind {
    Array(usize),
    Object,
}

fn inspect_table<'gc>(
    table: Table<'gc>,
    array_marker: Table<'gc>,
    depth: usize,
    nodes: &mut usize,
) -> Result<TableKind, extism_pdk::Error> {
    let marked_array = table.metatable() == Some(array_marker);
    let mut entry_count = 0usize;
    let mut max_index = 0usize;
    let mut integer_keys = true;
    let mut string_keys = true;

    for (key, _) in table.iter() {
        consume_json_node(depth + 1, nodes)?;
        entry_count = entry_count
            .checked_add(1)
            .ok_or_else(|| runner_error("Lua table is too large"))?;
        if entry_count > MAX_JSON_NODES {
            return Err(runner_error("Lua table is too large"));
        }
        match key {
            Value::Integer(index) if index > 0 => {
                let index = usize::try_from(index)
                    .map_err(|_| runner_error("Lua array index is invalid"))?;
                if index > MAX_ARRAY_INDEX {
                    return Err(runner_error("Lua array index is too large"));
                }
                max_index = max_index.max(index);
                string_keys = false;
            }
            Value::String(_) => integer_keys = false,
            _ => {
                integer_keys = false;
                string_keys = false;
            }
        }
    }

    if marked_array || (entry_count > 0 && integer_keys) {
        if !integer_keys || max_index != entry_count {
            return Err(runner_error(
                "Lua array must use contiguous positive indices",
            ));
        }
        Ok(TableKind::Array(entry_count))
    } else if string_keys {
        Ok(TableKind::Object)
    } else {
        Err(runner_error("Lua table mixes incompatible key types"))
    }
}

fn write_json<W: Write, T: serde::Serialize + ?Sized>(
    writer: &mut W,
    value: &T,
) -> Result<(), extism_pdk::Error> {
    serde_json::to_writer(writer, value).map_err(|_| runner_error("Lua output limit exceeded"))
}

fn write_raw<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), extism_pdk::Error> {
    writer
        .write_all(bytes)
        .map_err(|_| runner_error("Lua output limit exceeded"))
}

fn consume_json_node(depth: usize, nodes: &mut usize) -> Result<(), extism_pdk::Error> {
    if depth > MAX_JSON_DEPTH || *nodes == 0 {
        return Err(runner_error("Lua JSON structure limit exceeded"));
    }
    *nodes -= 1;
    Ok(())
}

struct CountingWriter {
    written: usize,
    limit: usize,
}

impl CountingWriter {
    fn new(limit: usize) -> Self {
        Self { written: 0, limit }
    }

    fn written(&self) -> usize {
        self.written
    }
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.written) {
            return Err(io::Error::other("Lua output limit exceeded"));
        }
        self.written += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static FINAL_OUTPUT_BUFFER_CAPACITY: std::cell::Cell<usize> = const {
        std::cell::Cell::new(usize::MAX)
    };
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        #[cfg(test)]
        FINAL_OUTPUT_BUFFER_CAPACITY.with(|capacity| capacity.set(limit));
        Self {
            bytes: Vec::with_capacity(limit),
            limit,
        }
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("Lua output limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn runner_error(message: &'static str) -> extism_pdk::Error {
    extism_pdk::Error::msg(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_output_writer_never_allocates_past_its_budget() {
        let mut writer = LimitedWriter::new(8);
        assert!(writer.write_all(&[b'x'; 1024]).is_err());
        assert!(writer.bytes.is_empty());
        assert!(writer.bytes.capacity() <= 8);
    }

    #[test]
    fn counting_writer_charges_actual_json_escaping() {
        let value = "\"\\\n";
        let expected = serde_json::to_vec(value).unwrap();
        let mut exact = CountingWriter::new(expected.len());
        write_json(&mut exact, value).unwrap();
        assert_eq!(exact.written(), expected.len());

        let mut short = CountingWriter::new(expected.len() - 1);
        assert!(write_json(&mut short, value).is_err());
        assert!(short.written() < expected.len());
    }

    #[test]
    fn oversized_lua_value_fails_before_final_output_buffer_allocation() {
        FINAL_OUTPUT_BUFFER_CAPACITY.with(|capacity| capacity.set(usize::MAX));
        let mut lua = Lua::empty();
        let result = lua.enter(|ctx| {
            let null = Table::new(&ctx);
            let array = Table::new(&ctx);
            let value = Table::new(&ctx);
            value
                .set(ctx, "payload", ctx.intern(&[b'x'; 4096]))
                .unwrap();
            encode_lua_json(ctx, value.into(), null, array, 16)
        });
        assert!(result.is_err());
        FINAL_OUTPUT_BUFFER_CAPACITY.with(|capacity| assert_eq!(capacity.get(), usize::MAX));
    }
}
