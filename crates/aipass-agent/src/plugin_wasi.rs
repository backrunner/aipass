//! Isolated binary entry point. No vault, network sockets, directory handles,
//! environment variables or native libraries are exposed to the guest.
use anyhow::{ensure, Result};
use std::{fs::File, io::Read, path::Path};
use wasmi::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};
use wasmi_wasi::{WasiCtx, WasiCtxBuilder};

struct Host {
    wasi: WasiCtx,
    limits: StoreLimits,
}

fn shape(bytes: &[u8]) -> Result<u32> {
    ensure!(
        bytes.len() <= 16 * 1024 * 1024 && bytes.starts_with(b"\0asm\x01\0\0\0"),
        "Invalid WASI binary"
    );
    // Go emits several thousand active data segments for ordinary programs.
    // Bound decoding before compilation as well as guest memory during execution.
    let mut start = None;
    let mut memory = false;
    for section in wasmparser::Parser::new(0).parse_all(bytes) {
        use wasmparser::Payload::*;
        let valid = match section? {
            TypeSection(r) => r.count() <= 8192,
            FunctionSection(r) => r.count() <= 16384,
            ImportSection(r) => {
                ensure!(r.count() <= 256, "Too many WASI imports");
                for import in r {
                    let i = import?;
                    ensure!(
                        i.module == "wasi_snapshot_preview1"
                            && matches!(i.ty, wasmparser::TypeRef::Func(_)),
                        "Unsupported WASI import"
                    );
                }
                true
            }
            ExportSection(r) => {
                ensure!(r.count() <= 1024, "Too many WASI exports");
                for export in r {
                    let e = export?;
                    match (e.name, e.kind) {
                        ("_start", wasmparser::ExternalKind::Func) => start = Some(e.index),
                        ("memory", wasmparser::ExternalKind::Memory) => memory = true,
                        _ => {}
                    }
                }
                true
            }
            GlobalSection(r) => r.count() <= 1024,
            DataSection(r) => r.count() <= 65536,
            ElementSection(r) => r.count() <= 1024,
            TableSection(r) => r.count() <= 1,
            MemorySection(r) => {
                ensure!(r.count() == 1, "Expected one WASI memory");
                let m = r
                    .into_iter()
                    .next()
                    .transpose()?
                    .ok_or_else(|| anyhow::anyhow!("Missing WASI memory"))?;
                !m.memory64 && !m.shared && m.page_size_log2.is_none() && m.initial <= 1024
            }
            _ => true,
        };
        ensure!(valid, "WASI module exceeds structural limits");
    }
    ensure!(memory, "Missing WASI memory export");
    start.ok_or_else(|| anyhow::anyhow!("Missing WASI command entry point"))
}

/// Passive inspection. Validate function bodies and the command signature without instantiation.
pub(crate) fn validate(bytes: &[u8]) -> Result<()> {
    let start = shape(bytes)?;
    let types = wasmparser::Validator::new().validate_all(bytes)?;
    let types = types.as_ref();
    let ty = types[types.core_function_at(start)].unwrap_func();
    ensure!(
        ty.params().is_empty() && ty.results().is_empty(),
        "Invalid WASI command signature"
    );
    Ok(())
}

pub fn run(path: &Path) -> Result<()> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    shape(&bytes)?;
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes)?;
    let mut linker = Linker::new(&engine);
    wasmi_wasi::add_to_linker(&mut linker, |host: &mut Host| &mut host.wasi)?;
    let host = Host {
        wasi: WasiCtxBuilder::new()
            .inherit_stdin()
            .inherit_stdout()
            .build(),
        limits: StoreLimitsBuilder::new()
            .memory_size(64 * 1024 * 1024)
            .table_elements(32768)
            .instances(1)
            .memories(1)
            .tables(1)
            .build(),
    };
    let mut store = Store::new(&engine, host);
    store.limiter(|host| &mut host.limits);
    store.set_fuel(1_000_000_000)?;
    let instance = linker.instantiate_and_start(&mut store, &module)?;
    let start = instance.get_typed_func::<(), ()>(&store, "_start")?;
    match start.call(&mut store, ()) {
        Ok(()) => Ok(()),
        Err(e) if e.i32_exit_status() == Some(0) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
