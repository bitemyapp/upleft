//! Rust side of the `local-ai` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/LocalAIDump.swift`.
//!
//! The real model is never called: requests run through
//! `DeterministicLocalAIProvider` and the Apple adapter's own input, prompt
//! and result steps, with the case supplying the model's reply. Normalised:
//! the prompt's random nonce, replaced by `NONCE` in both markers after its
//! shape is checked.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use serde_json::Value;
use upleft_app::ai::local_ai::*;
use upleft_core::contracts::TextEdit;
use upleft_swift_text::NSRange;

use super::json::Object;
use super::{Failure, Request};

pub fn run(request: &Request) -> Result<(), Failure> {
    let data = std::fs::read(&request.input)?;
    let root: Value = serde_json::from_slice(&data).map_err(|error| Failure::Error(format!("local-ai: {error}")))?;
    let Value::Object(root) = root else {
        return Err(Failure::Error("local-ai: the case is not a JSON object".into()));
    };

    let tasks = LocalAITask::ALL_CASES
        .iter()
        .map(|task| Object::new().with("rawValue", task.raw_value()).with("title", task.title()).build())
        .collect::<Vec<_>>();

    let mut requests = Vec::new();
    for entry in array(root.get("requests")) {
        let (request, output) = request_from(entry)?;
        requests.push(dump_request(&request, &output));
    }

    let mut previews = Vec::new();
    for entry in array(root.get("previews")) {
        let (Some(range), Some(original), Some(proposed), Some(current)) = (
            range_value(entry.get("range")),
            entry.get("originalSource").and_then(Value::as_str),
            entry.get("proposedSource").and_then(Value::as_str),
            entry.get("current").and_then(Value::as_str),
        ) else {
            return Err(Failure::Error(format!("local-ai: malformed preview {entry}")));
        };
        let preview = LocalAIPreview { range, original_source: original.into(), proposed_source: proposed.into() };
        previews.push(
            Object::new()
                .with("isNoOp", preview.is_no_op())
                .with("edit", edit(LocalAIEditValidator::edit(&preview, current).as_ref()))
                .build(),
        );
    }

    let mut controllers = Vec::new();
    for sequence in array(root.get("controller")) {
        let mut sequence_requests = Vec::new();
        for entry in array(Some(sequence)) {
            sequence_requests.push(request_from(entry)?.0);
        }
        controllers.push(controller(sequence_requests)?);
    }

    let out = Object::new()
        .with("tasks", tasks)
        .with("requests", requests)
        .with("previews", previews)
        .with("controller", controllers)
        .build();
    Ok(super::json::write(&out, &request.output)?)
}

fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn request_from(entry: &Value) -> Result<(LocalAIRequest, String), Failure> {
    let task = entry.get("task").and_then(Value::as_str).and_then(LocalAITask::from_raw_value);
    let source = entry.get("source").and_then(Value::as_str);
    let (Some(task), Some(source)) = (task, source) else {
        return Err(Failure::Error(format!("local-ai: malformed request {entry}")));
    };
    let output = entry.get("output").and_then(Value::as_str).unwrap_or("").to_owned();
    Ok((LocalAIRequest::new(task, source, range_value(entry.get("selection"))), output))
}

fn range_value(value: Option<&Value>) -> Option<NSRange> {
    let pair = value?.as_array()?;
    let [location, length] = pair.as_slice() else { return None };
    Some(NSRange::new(location.as_i64()? as isize, length.as_i64()? as isize))
}

fn dump_request(request: &LocalAIRequest, output: &str) -> Value {
    let deterministic = DeterministicLocalAIProvider::run_now(request, &TaskCancellation::new());
    let deterministic_edit = match &deterministic {
        Ok(LocalAIResult { preview: Some(preview), .. }) => edit(LocalAIEditValidator::edit(preview, &request.source).as_ref()),
        _ => Value::Null,
    };
    let apple = match AppleOnDeviceAIProvider::input(request) {
        Ok(input) => {
            let prompts = LocalAITask::ALL_CASES
                .iter()
                .map(|task| normalized_prompt(&AppleOnDeviceAIProvider::prompt(*task, &input)))
                .collect::<Vec<_>>();
            let shaped = AppleOnDeviceAIProvider::result(request, &input, output);
            let shaped_edit = shaped
                .preview
                .as_ref()
                .and_then(|preview| LocalAIEditValidator::edit(preview, &request.source));
            Object::new()
                .with("input", lines(&input))
                .with("prompts", prompts)
                .with("result", result(&shaped))
                .with("edit", edit(shaped_edit.as_ref()))
                .build()
        }
        Err(error) => Object::new().with("error", error_name(&error.into())).build(),
    };
    Object::new()
        .with("deterministic", outcome(&deterministic))
        .with("deterministicEdit", deterministic_edit)
        .with("apple", apple)
        .build()
}

fn controller(requests: Vec<LocalAIRequest>) -> Result<Value, Failure> {
    let mtm = MainThreadMarker::new().ok_or_else(|| Failure::Error("local-ai: not on the main thread".into()))?;
    let controller = LocalAILatestWinsController::new(Arc::new(DeterministicLocalAIProvider::new()), mtm);
    let delivered: Rc<RefCell<Vec<Value>>> = Rc::default();
    let count = requests.len();
    for request in requests {
        let sink = delivered.clone();
        controller.submit(request, move |result| sink.borrow_mut().push(outcome(&result)));
    }
    if count == 0 {
        return Ok(Value::Array(Vec::new()));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while delivered.borrow().is_empty() && Instant::now() < deadline {
        run_loop(0.005);
    }
    if delivered.borrow().is_empty() {
        return Err(Failure::Error("local-ai: the controller delivered nothing".into()));
    }
    // Grace: a superseded request would deliver now if it were going to.
    let grace = Instant::now() + Duration::from_millis(100);
    while Instant::now() < grace {
        run_loop(0.005);
    }
    let values = delivered.borrow().clone();
    Ok(Value::Array(values))
}

/// `RunLoop.main.run(until:)`: drains the main queue for up to `seconds`.
fn run_loop(seconds: f64) {
    unsafe extern "C" {
        fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
        static kCFRunLoopDefaultMode: *const std::ffi::c_void;
    }
    // SAFETY: runs the main thread's run loop; the oracle runs on it.
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 1) };
}

fn outcome(value: &Result<LocalAIResult, LocalAIRunError>) -> Value {
    match value {
        Ok(value) => Object::new().with("result", result(value)).build(),
        Err(error) => Object::new().with("error", error_name(error)).build(),
    }
}

fn result(value: &LocalAIResult) -> Value {
    let preview = value.preview.as_ref().map_or(Value::Null, |preview| {
        Object::new()
            .with("range", range(preview.range))
            .with("originalSource", lines(&preview.original_source))
            .with("proposedSource", lines(&preview.proposed_source))
            .with("isNoOp", preview.is_no_op())
            .build()
    });
    Object::new()
        .with("task", value.task.raw_value())
        .with("text", lines(&value.text))
        .with("preview", preview)
        .build()
}

fn range(range: NSRange) -> Value {
    Value::Array(vec![range.location.into(), range.length.into()])
}

fn edit(value: Option<&TextEdit>) -> Value {
    let Some(value) = value else { return Value::Null };
    Object::new()
        .with("range", range(value.range))
        .with("replacement", lines(&value.replacement))
        .with("summary", value.summary.as_str())
        .build()
}

fn error_name(error: &LocalAIRunError) -> Value {
    let name = match error {
        LocalAIRunError::LocalAI(LocalAIError::EmptyInput) => "emptyInput",
        LocalAIRunError::LocalAI(LocalAIError::Cancelled) => "cancelled",
        LocalAIRunError::LocalAI(LocalAIError::Unavailable(LocalAIAvailability::Available)) => "unavailable(available)",
        LocalAIRunError::LocalAI(LocalAIError::Unavailable(LocalAIAvailability::FrameworkUnavailable)) => {
            "unavailable(frameworkUnavailable)"
        }
        LocalAIRunError::LocalAI(LocalAIError::Unavailable(LocalAIAvailability::SystemUnavailable)) => {
            "unavailable(systemUnavailable)"
        }
        LocalAIRunError::Cancellation => "cancellation",
        LocalAIRunError::Other(_) => "other",
    };
    Value::String(name.into())
}

/// Text as lines split on each U+000A, so a difference names the line.
fn lines(text: &str) -> Value {
    Value::Array(text.split('\n').map(|line| Value::String(line.into())).collect())
}

/// The prompt as lines, with the nonce in both markers replaced by `NONCE`.
fn normalized_prompt(prompt: &str) -> Value {
    let lead = "=====BEGIN DOCUMENT [";
    let Some(start) = prompt.find(lead).map(|index| index + lead.len()) else {
        return Object::new().with("nonce", "missing").build();
    };
    let Some(close) = prompt[start..].find(']').map(|index| index + start) else {
        return Object::new().with("nonce", "missing").build();
    };
    let nonce = &prompt[start..close];
    let shape = nonce.len() == 8 && nonce.bytes().all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte));
    let normalized = prompt
        .replace(&format!("=====BEGIN DOCUMENT [{nonce}]====="), "=====BEGIN DOCUMENT [NONCE]=====")
        .replace(&format!("=====END DOCUMENT [{nonce}]====="), "=====END DOCUMENT [NONCE]=====");
    let nonce_text = if shape { "8 upper-case hex digits".to_owned() } else { format!("unexpected: {nonce}") };
    Object::new().with("nonce", nonce_text).with("lines", lines(&normalized)).build()
}
