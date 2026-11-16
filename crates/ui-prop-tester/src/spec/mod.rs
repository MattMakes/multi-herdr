pub mod sampling;
pub mod typescript;

mod loader;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use boa_engine::builtins::promise::PromiseState;
use boa_engine::gc::{Finalize, Trace};
use boa_engine::object::builtins::JsArray;
use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{
    js_string, Context, JsError, JsNativeError, JsResult, JsValue, Module, NativeFunction, Source,
};
use serde_json::Value;

use crate::driver::{Action, CustomAction, DriverKind, RemoteExtractor};
use crate::rng::SplitMix64;
use crate::trace::{SpecificationRecord, Violation};

use loader::Loader;

pub const CORE: &str = "ui-prop-tester";

const CALL_ARGUMENTS: &str = "__uiPropTesterArguments";

#[derive(Debug, Clone, PartialEq)]
pub struct Specification {
    pub path: PathBuf,
    pub source: String,
}

impl Specification {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let source = std::fs::read_to_string(&absolute).map_err(|error| {
            format!(
                "cannot read the specification {}: {error}",
                absolute.display()
            )
        })?;
        Ok(Self {
            path: absolute,
            source,
        })
    }

    pub fn from_record(record: &SpecificationRecord) -> Self {
        let path = PathBuf::from(&record.path);
        match std::fs::read_to_string(&path) {
            Ok(source) => Self { path, source },
            Err(_) => Self {
                path,
                source: record.source.clone(),
            },
        }
    }

    pub fn record(&self) -> SpecificationRecord {
        SpecificationRecord {
            path: self.path.display().to_string(),
            source: self.source.clone(),
        }
    }
}

pub fn driver_module(kind: DriverKind) -> &'static str {
    match kind {
        DriverKind::Browser => "ui-prop-tester/browser",
        DriverKind::Terminal => "ui-prop-tester/terminal",
    }
}

pub fn defaults_module(kind: DriverKind) -> &'static str {
    match kind {
        DriverKind::Browser => "ui-prop-tester/browser/defaults",
        DriverKind::Terminal => "ui-prop-tester/terminal/defaults",
    }
}

#[derive(Clone)]
struct SharedRng(Rc<RefCell<SplitMix64>>);

impl Finalize for SharedRng {}

unsafe impl Trace for SharedRng {
    boa_engine::gc::empty_trace!();
}

pub struct SpecRuntime {
    context: Context,
    pub properties: Vec<String>,
    pub categories: Vec<String>,
    pub extractors: Vec<RemoteExtractor>,
    pub extractor_names: Vec<String>,
    pub custom_actions: Vec<CustomAction>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    properties: Vec<String>,
    categories: Vec<String>,
    extractors: Vec<InstalledExtractor>,
    custom_actions: Vec<InstalledCustomAction>,
}

#[derive(serde::Deserialize)]
struct InstalledExtractor {
    id: u32,
    name: String,
    source: String,
}

#[derive(serde::Deserialize)]
struct InstalledCustomAction {
    name: String,
    source: String,
}

impl SpecRuntime {
    pub fn load(
        kind: DriverKind,
        specification: Option<&Specification>,
        seed: u64,
    ) -> Result<Self, String> {
        let entry = specification.map(|spec| (spec.path.clone(), spec.source.clone()));
        let loader = Rc::new(Loader::new(entry));
        let mut context = Context::builder()
            .module_loader(loader.clone())
            .build()
            .map_err(|error| format!("cannot start the JavaScript engine: {error}"))?;
        let rng = SharedRng(Rc::new(RefCell::new(SplitMix64::new(seed))));
        install_natives(&mut context, rng)
            .map_err(|error| format!("cannot start the JavaScript engine: {error}"))?;

        let core = loader
            .embedded(CORE, &mut context)
            .map_err(|error| error.to_string())?;
        evaluate(&core, &mut context).map_err(|error| describe(&error, &mut context))?;
        let driver = loader
            .embedded(driver_module(kind), &mut context)
            .map_err(|error| error.to_string())?;
        evaluate(&driver, &mut context).map_err(|error| describe(&error, &mut context))?;
        let spec_module = match specification {
            Some(spec) => loader
                .file(&spec.path, &mut context)
                .map_err(|error| format!("cannot load the specification: {error}"))?,
            None => loader
                .embedded(defaults_module(kind), &mut context)
                .map_err(|error| error.to_string())?,
        };
        evaluate(&spec_module, &mut context).map_err(|error| {
            format!(
                "the specification failed to load: {}",
                describe(&error, &mut context)
            )
        })?;

        let started = context
            .global_object()
            .get(js_string!("__uiPropTesterEngine"), &mut context)
            .is_ok_and(|value| value.is_object());
        if !started {
            return Err("the ui-prop-tester core module did not start".into());
        }
        let spec_namespace = spec_module.namespace(&mut context);
        let driver_namespace = driver.namespace(&mut context);
        let mut runtime = Self {
            context,
            properties: Vec::new(),
            categories: Vec::new(),
            extractors: Vec::new(),
            extractor_names: Vec::new(),
            custom_actions: Vec::new(),
        };
        let installed =
            runtime.call_text("install", &[spec_namespace.into(), driver_namespace.into()])?;
        let installed: Installed = serde_json::from_str(&installed)
            .map_err(|error| format!("the specification did not install: {error}"))?;
        runtime.properties = installed.properties;
        runtime.categories = installed.categories;
        runtime.extractor_names = installed
            .extractors
            .iter()
            .map(|extractor| extractor.name.clone())
            .collect();
        runtime.extractors = installed
            .extractors
            .into_iter()
            .map(|extractor| RemoteExtractor {
                id: extractor.id,
                source: extractor.source,
            })
            .collect();
        runtime.custom_actions = installed
            .custom_actions
            .into_iter()
            .map(|action| CustomAction {
                name: action.name,
                source: action.source,
            })
            .collect();
        Ok(runtime)
    }

    pub fn extract_local(&mut self, state: &Value) -> Result<(), String> {
        let text = JsValue::from(js_string!(state.to_string()));
        self.call("extractLocal", &[text]).map(|_| ())
    }

    pub fn set_cell_values(&mut self, values: &[Value]) -> Result<(), String> {
        let text = Value::Array(values.to_vec()).to_string();
        self.call("setCellValues", &[JsValue::from(js_string!(text))])
            .map(|_| ())
    }

    pub fn exported_cells(&mut self) -> Result<Value, String> {
        let text = self.call_text("exportedCells", &[])?;
        parse_json(&text)
    }

    pub fn step(&mut self, index: u64, timestamp_ms: u64) -> Result<Vec<Violation>, String> {
        let text = self.call_text(
            "step",
            &[
                JsValue::from(index as f64),
                JsValue::from(timestamp_ms as f64),
            ],
        )?;
        parse_violations(&text)
    }

    pub fn finish(&mut self, index: u64, timestamp_ms: u64) -> Result<Vec<Violation>, String> {
        let text = self.call_text(
            "finish",
            &[
                JsValue::from(index as f64),
                JsValue::from(timestamp_ms as f64),
            ],
        )?;
        parse_violations(&text)
    }

    pub fn sample(&mut self, enabled: &[String]) -> Result<Option<Action>, String> {
        let names = serde_json::to_string(enabled).unwrap_or_else(|_| "[]".into());
        let value = self.call("sample", &[JsValue::from(js_string!(names))])?;
        if value.is_null() || value.is_undefined() {
            return Ok(None);
        }
        let text = value
            .to_string(&mut self.context)
            .map_err(|error| error.to_string())?
            .to_std_string_escaped();
        parse_json(&text).map(Some)
    }

    pub fn offered(&mut self) -> Result<Vec<Action>, String> {
        let text = self.call_text("offered", &[])?;
        match parse_json(&text)? {
            Value::Array(actions) => Ok(actions),
            _ => Err("the generators did not list their actions".into()),
        }
    }

    fn call_text(&mut self, method: &str, args: &[JsValue]) -> Result<String, String> {
        let value = self.call(method, args)?;
        value
            .to_string(&mut self.context)
            .map(|text| text.to_std_string_escaped())
            .map_err(|error| error.to_string())
    }

    fn call(&mut self, method: &str, args: &[JsValue]) -> Result<JsValue, String> {
        let list = JsArray::from_iter(args.iter().cloned(), &mut self.context);
        self.context
            .global_object()
            .set(js_string!(CALL_ARGUMENTS), list, true, &mut self.context)
            .map_err(|error| error.to_string())?;
        let script =
            format!("globalThis.__uiPropTesterEngine.{method}(...globalThis.{CALL_ARGUMENTS})");
        let result = self.context.eval(Source::from_bytes(script.as_bytes()));
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let activity = self.activity();
                let detail = describe(&error, &mut self.context);
                Err(match activity {
                    Some(activity) => format!("{activity} threw {detail}"),
                    None => detail,
                })
            }
        }
    }

    fn activity(&mut self) -> Option<String> {
        let value = self
            .context
            .eval(Source::from_bytes(
                "globalThis.__uiPropTesterEngine.activity()".as_bytes(),
            ))
            .ok()?;
        if value.is_null() || value.is_undefined() {
            return None;
        }
        value
            .to_string(&mut self.context)
            .ok()
            .map(|text| text.to_std_string_escaped())
    }
}

fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| format!("bad JSON from the specification: {error}"))
}

fn parse_violations(text: &str) -> Result<Vec<Violation>, String> {
    serde_json::from_str(text)
        .map_err(|error| format!("bad violation report from the specification: {error}"))
}

fn evaluate(module: &Module, context: &mut Context) -> JsResult<()> {
    let promise = module.load_link_evaluate(context);
    context.run_jobs()?;
    match promise.state() {
        PromiseState::Fulfilled(_) => Ok(()),
        PromiseState::Rejected(reason) => Err(JsError::from_opaque(reason)),
        PromiseState::Pending => Err(JsNativeError::error()
            .with_message("a module did not finish loading")
            .into()),
    }
}

pub fn describe(error: &JsError, context: &mut Context) -> String {
    let full = error.to_string();
    let stack = full
        .find("\n    at ")
        .map(|start| full[start..].to_string())
        .unwrap_or_default();
    let headline = match error.try_native(context) {
        Ok(native) => {
            let mut line = native.to_string();
            let mut cause = native.cause().cloned();
            while let Some(inner) = cause {
                let text = inner.to_string();
                line.push_str(&format!(" (caused by {})", first_line(&text)));
                cause = inner
                    .try_native(context)
                    .ok()
                    .and_then(|native| native.cause().cloned());
            }
            line
        }
        Err(_) => first_line(&full).to_string(),
    };
    format!("{headline}{stack}")
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn install_natives(context: &mut Context, rng: SharedRng) -> JsResult<()> {
    let below = NativeFunction::from_copy_closure_with_captures(
        |_, args, rng: &SharedRng, context| {
            let bound = args
                .first()
                .cloned()
                .unwrap_or_default()
                .to_number(context)?;
            if !bound.is_finite() || bound < 1.0 {
                return Ok(JsValue::from(0));
            }
            let value = rng.0.borrow_mut().below(bound as u64);
            Ok(JsValue::from(value as f64))
        },
        rng.clone(),
    );
    let unit = NativeFunction::from_copy_closure_with_captures(
        |_, _, rng: &SharedRng, _| Ok(JsValue::from(rng.0.borrow_mut().unit_f64())),
        rng.clone(),
    );
    let regex = NativeFunction::from_copy_closure_with_captures(
        |_, args, rng: &SharedRng, context| {
            let pattern = string_argument(args, 0, context)?;
            let text = sampling::sample_regex(&pattern, &mut rng.0.borrow_mut())
                .map_err(|message| JsNativeError::syntax().with_message(message))?;
            Ok(JsValue::from(js_string!(text)))
        },
        rng.clone(),
    );
    let check_regex = NativeFunction::from_copy_closure(|_, args, context| {
        let pattern = string_argument(args, 0, context)?;
        sampling::parse_regex(&pattern)
            .map_err(|message| JsNativeError::syntax().with_message(message))?;
        Ok(JsValue::undefined())
    });
    let text = NativeFunction::from_copy_closure_with_captures(
        |_, args, rng: &SharedRng, context| {
            let min = args
                .first()
                .cloned()
                .unwrap_or_default()
                .to_number(context)?;
            let max = args
                .get(1)
                .cloned()
                .unwrap_or_default()
                .to_number(context)?;
            let text = sampling::sample_text(min as u32, max as u32, &mut rng.0.borrow_mut());
            Ok(JsValue::from(js_string!(text)))
        },
        rng.clone(),
    );
    let email = NativeFunction::from_copy_closure_with_captures(
        |_, _, rng: &SharedRng, _| {
            let text = sampling::sample_email(&mut rng.0.borrow_mut());
            Ok(JsValue::from(js_string!(text)))
        },
        rng,
    );
    let natives = ObjectInitializer::new(context)
        .function(below, js_string!("below"), 1)
        .function(unit, js_string!("unit"), 0)
        .function(regex, js_string!("regex"), 1)
        .function(check_regex, js_string!("checkRegex"), 1)
        .function(text, js_string!("text"), 2)
        .function(email, js_string!("email"), 0)
        .build();
    context.register_global_property(
        js_string!("__uiPropTesterNative"),
        natives,
        Attribute::empty(),
    )
}

fn string_argument(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    Ok(args
        .get(index)
        .cloned()
        .unwrap_or_default()
        .to_string(context)?
        .to_std_string_escaped())
}

#[cfg(test)]
mod tests;
