use anyhow::{anyhow, bail, Context, Result};
use std::{env, fs};
use wasm_encoder::{ExportKind, ExportSection, Module, RawSection};
use wasmparser::{KnownCustom, Name, Parser, Payload};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);

    let input = args
        .next()
        .context("usage: add-wasm-export-by-name <in.wasm> <out.wasm> <export-name> <func-name>")?;
    let output = args
        .next()
        .context("usage: add-wasm-export-by-name <in.wasm> <out.wasm> <export-name> <func-name>")?;
    let export_name = args
        .next()
        .context("usage: add-wasm-export-by-name <in.wasm> <out.wasm> <export-name> <func-name>")?;
    let func_name = args
        .next()
        .context("usage: add-wasm-export-by-name <in.wasm> <out.wasm> <export-name> <func-name>")?;

    let wasm = fs::read(&input).with_context(|| format!("failed to read {input}"))?;

    let func_index = find_function_index_by_name(&wasm, &func_name)
        .with_context(|| format!("could not find function name {func_name:?} in name section"))?;

    let new_wasm = add_func_export(&wasm, &export_name, func_index)?;
    wasmparser::validate(&new_wasm).context("rewritten wasm is invalid")?;

    fs::write(&output, new_wasm).with_context(|| format!("failed to write {output}"))?;
    println!(
        "added export {:?} -> function {:?} (index {})",
        export_name, func_name, func_index
    );

    Ok(())
}

fn find_function_index_by_name(wasm: &[u8], wanted: &str) -> Result<u32> {
    for payload in Parser::new(0).parse_all(wasm) {
        match payload? {
            Payload::CustomSection(section) => {
                match section.as_known() {
                    KnownCustom::Name(names) => {
                        for subsection in names {
                            match subsection? {
                                Name::Function(map) => {
                                    for naming in map {
                                        let naming = naming?;
                                        if naming.name == wanted {
                                            return Ok(naming.index);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    bail!("function name not found in name section")
}

fn add_func_export(wasm: &[u8], export_name: &str, func_index: u32) -> Result<Vec<u8>> {
    let mut module = Module::new();
    let mut found_export_section = false;
    let mut code_section_start: Option<usize> = None;
    let mut code_section_end: Option<usize> = None;

    for payload in Parser::new(0).parse_all(wasm) {
        match payload? {
            Payload::Version { .. } => {}

            Payload::TypeSection(s) => {
                module.section(&RawSection {
                    id: wasm_encoder::SectionId::Type.into(),
                    data: s.range().slice(wasm),
                });
            }

            Payload::ImportSection(s) => {
                module.section(&RawSection {
                    id: wasm_encoder::SectionId::Import.into(),
                    data: s.range().slice(wasm),
                });
            }

            Payload::FunctionSection(s) => {
                module.section(&RawSection {
                    id: wasm_encoder::SectionId::Function.into(),
                    data: s.range().slice(wasm),
                });
            }

            Payload::ExportSection(s) => {
                found_export_section = true;

                let mut new_exports = ExportSection::new();
                let mut already_present = false;

                for export in s {
                    let export = export?;
                    let kind = match export.kind {
                        wasmparser::ExternalKind::Func => ExportKind::Func,
                        wasmparser::ExternalKind::Table => ExportKind::Table,
                        wasmparser::ExternalKind::Memory => ExportKind::Memory,
                        wasmparser::ExternalKind::Global => ExportKind::Global,
                        wasmparser::ExternalKind::Tag => ExportKind::Tag,
                        wasmparser::ExternalKind::FuncExact => ExportKind::Func,
                    };

                    if export.name == export_name {
                        already_present = true;
                    }

                    new_exports.export(export.name, kind, export.index);
                }

                if !already_present {
                    new_exports.export(export_name, ExportKind::Func, func_index);
                }

                module.section(&new_exports);
            }

            Payload::CodeSectionStart { range, .. } => {
                code_section_start = Some(range.start);
            }

            Payload::CodeSectionEntry(body) => {
                code_section_end = Some(body.range().end);
            }

            Payload::End(_) => {}

            _ => {}
        }
    }

    if !found_export_section {
        let mut exports = ExportSection::new();
        exports.export(export_name, ExportKind::Func, func_index);
        module.section(&exports);
    }

    if let (Some(start), Some(end)) = (code_section_start, code_section_end) {
        module.section(&RawSection {
            id: wasm_encoder::SectionId::Code.into(),
            data: &wasm[start..end],
        });
    }

    Ok(module.finish())
}

trait RangeSlice {
    fn slice<'a>(&self, bytes: &'a [u8]) -> &'a [u8];
}

impl RangeSlice for std::ops::Range<usize> {
    fn slice<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.start..self.end]
    }
}
