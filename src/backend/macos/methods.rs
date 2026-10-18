//! Method execution mirrors the Windows backend: bind by schema, confirm, run per target.
use super::{
    context::Context,
    error,
    provider::{Entry, MethodCall},
    query,
};
use crate::{
    backend::{Confirmation, ResolvedRequest, cancellable_targets, methods::bind_method},
    model::{
        ManagementError, ManagementResult, MethodResult, OperationResult, Record, Verb, WideText,
    },
};

/// Bind known methods or report the error for the original alias or direct syntax.
pub(super) fn call(
    request: &ResolvedRequest,
    entry: &Entry,
    ctx: &Context,
    mut confirmation: Option<&mut dyn Confirmation>,
) -> Result<ManagementResult, ManagementError> {
    let node = &request.connection.node;
    let requested = request.method.clone().unwrap_or_default();
    let method = entry
        .info
        .schema
        .methods
        .iter()
        .find(|m| requested.eq_ignore_case(&m.name.to_string_lossy()))
        .ok_or_else(|| {
            error::input_error(
                &request.class_name,
                crate::backend::methods::unknown_method(&requested, request.from_alias),
            )
        })?;
    let input = bind_method(method, &request.arguments)
        .map_err(|e| error::input_error(&request.class_name, e))?;
    let targets: Vec<(WideText, Option<Record>)> = if method.is_static || request.class_mode {
        vec![(request.class_name.clone(), None)]
    } else {
        query::select(request, entry, ctx)?
            .into_iter()
            .filter_map(|r| r.path.clone().map(|p| (p, Some(r))))
            .collect()
    };
    let had_targets = !targets.is_empty();
    let streamed = confirmation.as_ref().is_some_and(|c| c.streams_methods());
    let interactive = crate::backend::write::interactive(request);
    let mut results: Vec<MethodResult> = vec![];
    for target in cancellable_targets(targets, error::interrupted) {
        let (path, instance) = target.map_err(|_| {
            error::cancelled(node, Some(ManagementResult::Methods(results.clone())))
        })?;
        if interactive {
            let mut prompt: WideText = "Execute (".into();
            prompt.0.extend(&path.0);
            prompt.0.extend(")->".encode_utf16());
            prompt.0.extend(&request.shown_method().0);
            prompt.0.extend("() (Y/N/?)? ".encode_utf16());
            let Some(c) = confirmation.as_deref_mut() else {
                return Err(error::confirmation_unavailable(&request.class_name));
            };
            if !c.confirm(&prompt)? {
                continue;
            }
        }
        if error::interrupted() {
            return Err(error::cancelled(
                node,
                Some(ManagementResult::Methods(results)),
            ));
        }
        let output = entry.provider.call(
            ctx,
            &MethodCall {
                method,
                path: &path,
                instance: instance.as_ref(),
                input: &input,
            },
        );
        let output = match output {
            Ok(output) => output,
            Err(mut e) => {
                e.target = path;
                if !results.is_empty() {
                    e.partial = Some(Box::new(ManagementResult::Methods(results)));
                }
                return Err(e);
            }
        };
        let result = MethodResult {
            path,
            return_value: output.return_value,
            out_parameters: output.out_parameters,
            text: None,
        };
        if streamed {
            if let Some(c) = confirmation.as_deref_mut() {
                c.method_completed(request, &result)?;
            }
        } else {
            results.push(result);
        }
    }
    if results.is_empty() && had_targets {
        return Ok(ManagementResult::Operation(OperationResult {
            action: Verb::Call,
            paths: vec![],
            instances: vec![Record::default()],
        }));
    }
    Ok(ManagementResult::Methods(results))
}
