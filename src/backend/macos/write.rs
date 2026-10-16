//! Create, update and delete mirror the Windows backend: bind by schema, confirm, apply per target.
use super::{
    context::Context,
    error,
    provider::{Change, Entry},
    query,
};
use crate::{
    backend::{Confirmation, ResolvedRequest, cancellable_targets, write::bind_properties},
    model::{ManagementError, ManagementResult, OperationResult, Record, Verb, WideText},
};

fn confirm(
    request: &ResolvedRequest,
    confirmation: &mut Option<&mut dyn Confirmation>,
    prompt: &WideText,
) -> Result<bool, ManagementError> {
    match confirmation.as_deref_mut() {
        Some(c) => c.confirm(prompt),
        None => Err(error::confirmation_unavailable(&request.class_name)),
    }
}

pub(super) fn execute(
    request: &ResolvedRequest,
    entry: &Entry,
    ctx: &Context,
    mut confirmation: Option<&mut dyn Confirmation>,
) -> Result<ManagementResult, ManagementError> {
    let node = &request.connection.node;
    let bound = if request.verb == Verb::Delete {
        vec![]
    } else {
        bind_properties(&entry.info.schema, &request.arguments)
            .map_err(|e| error::input_error(&request.class_name, e))?
    };
    let interactive = crate::backend::write::interactive(request);
    if request.verb == Verb::Create {
        if interactive {
            let mut prompt: WideText = "Create instance of '".into();
            prompt.0.extend(&request.class_name.0);
            prompt.0.extend("' class (Y/N)?".encode_utf16());
            if !confirm(request, &mut confirmation, &prompt)? {
                return Ok(ManagementResult::Operation(OperationResult {
                    action: request.verb,
                    paths: vec![],
                    instances: vec![Record::default()],
                }));
            }
        }
        if error::interrupted() {
            return Err(error::cancelled(node, None));
        }
        entry.provider.write(
            ctx,
            &Change {
                verb: request.verb,
                path: None,
                instance: None,
                properties: &bound,
            },
        )?;
        return Ok(ManagementResult::Operation(OperationResult {
            action: request.verb,
            paths: vec![],
            instances: vec![],
        }));
    }
    if request.class_mode {
        return Err(error::not_supported(node, "write"));
    }
    let selected = query::select(request, entry, ctx)?;
    let instances: Vec<Record> = selected
        .iter()
        .map(|o| Record {
            class: o.class.clone(),
            path: o.path.clone(),
            ..Default::default()
        })
        .collect();
    let operation = |paths: Vec<WideText>| {
        ManagementResult::Operation(OperationResult {
            action: request.verb,
            paths,
            instances: vec![],
        })
    };
    let mut paths: Vec<WideText> = vec![];
    for record in cancellable_targets(selected, error::interrupted) {
        let record = record.map_err(|_| error::cancelled(node, Some(operation(paths.clone()))))?;
        let Some(path) = record.path.clone() else {
            return Err(error::error(node, 0x80041002, "Not found", "write"));
        };
        if interactive {
            let mut prompt: WideText = if request.verb == Verb::Delete {
                "Delete '"
            } else {
                "Update property(s) of '"
            }
            .into();
            prompt.0.extend(&path.0);
            prompt.0.extend("' (Y/N/?)? ".encode_utf16());
            if !confirm(request, &mut confirmation, &prompt)? {
                continue;
            }
        }
        if error::interrupted() {
            return Err(error::cancelled(node, Some(operation(paths))));
        }
        let applied = entry.provider.write(
            ctx,
            &Change {
                verb: request.verb,
                path: Some(&path),
                instance: Some(&record),
                properties: &bound,
            },
        );
        if let Err(mut e) = applied {
            e.target = path;
            e.operation = if request.verb == Verb::Delete {
                "delete"
            } else {
                "update"
            }
            .into();
            if !paths.is_empty() {
                e.partial = Some(Box::new(operation(paths)));
            }
            return Err(e);
        }
        paths.push(path);
    }
    Ok(ManagementResult::Operation(OperationResult {
        action: request.verb,
        paths,
        instances,
    }))
}
