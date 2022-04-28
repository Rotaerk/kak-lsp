use crate::context::*;
use crate::diagnostics::gather_line_flags;
use crate::position::{kakoune_range_to_lsp, parse_kakoune_range, ranges_lines_overlap};
use crate::types::*;
use crate::util::editor_quote;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;

pub fn text_document_code_lens(meta: EditorMeta, ctx: &mut Context) {
    if !ctx
        .capabilities
        .as_ref()
        .map(|caps| match &caps.code_lens_provider {
            Some(_clp) => true, // TODO clp.resolve_provider
            None => false,
        })
        .unwrap_or(false)
    {
        error!("NOCAP");
        return;
    }
    // .unwrap().resolve_provider
    let req_params = CodeLensParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    ctx.call::<CodeLensRequest, _>(meta, req_params, |ctx: &mut Context, meta, result| {
        editor_code_lens(meta, result, ctx)
    });
}

fn editor_code_lens(meta: EditorMeta, result: Option<Vec<CodeLens>>, ctx: &mut Context) {
    let mut lenses = result.unwrap_or(vec![]);
    lenses.sort_by_key(|lens| lens.range.start.line);

    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return, // TODO clear lsp_code_lenses?
    };

    ctx.code_lenses.insert(meta.buffile.clone(), lenses);
    let buffile = &meta.buffile;
    let line_flags = gather_line_flags(ctx, buffile).0;
    let version = document.version;

    let command = format!(
         "evaluate-commands \"set-option buffer lsp_error_lines {} {} '0|%opt[lsp_diagnostic_line_error_sign]'\"",
         version, line_flags,
    );
    let command = format!(
        "evaluate-commands -buffer {} %§{}§",
        editor_quote(buffile),
        command.replace('§', "§§")
    );
    let meta = ctx.meta_for_buffer_version(None, buffile, version);
    ctx.exec(meta, command);
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensOptions {
    pub selection_desc: String,
}

pub fn apply_code_lens(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = CodeLensOptions::deserialize(params)
        .expect("Params should follow CodeLensParams structure");
    let (range, _cursor) = parse_kakoune_range(&params.selection_desc);
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };
    let range = kakoune_range_to_lsp(&range, &document.text, ctx.offset_encoding);

    let lenses = match ctx.code_lenses.get(&meta.buffile) {
        Some(lenses) => lenses,
        None => return,
    };

    let lenses = lenses
        .iter()
        .filter(|lens| ranges_lines_overlap(lens.range, range))
        .collect::<Vec<_>>();

    let command = match lenses.len() {
        0 => "lsp-show-error 'no code lens in selection'".to_string(),
        _ => format!(
            "lsp-perform-code-lens {}",
            lenses
                .iter()
                .map(|lens| {
                    let command = lens.command.as_ref().unwrap(); // TODO
                    let cmd = &command.command;
                    // Double JSON serialization is performed to prevent parsing args as a TOML
                    // structure when they are passed back via lsp-execute-command.
                    let args = &serde_json::to_string(&command.arguments).unwrap();
                    let args = editor_quote(&serde_json::to_string(&args).unwrap());
                    let editor_command = format!("lsp-execute-command {} {}", cmd, args);
                    format!(
                        "{} {}",
                        editor_quote(&command.title),
                        editor_quote(&editor_command)
                    )
                })
                .join(" "),
        ),
    };

    ctx.exec(meta, command);
}
