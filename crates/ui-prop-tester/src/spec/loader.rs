use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use boa_engine::module::{ModuleLoader, Referrer};
use boa_engine::{Context, JsNativeError, JsResult, JsString, Module, Source};

use super::typescript::strip_types;

const EMBEDDED: &[(&str, &str)] = &[
    ("ui-prop-tester", include_str!("../js/core.js")),
    ("ui-prop-tester/internal", include_str!("js/internal.js")),
    ("ui-prop-tester/fake", include_str!("js/fake.js")),
    ("ui-prop-tester/terminal", include_str!("../js/terminal.js")),
    (
        "ui-prop-tester/terminal/defaults",
        include_str!("../js/terminal_defaults.js"),
    ),
    ("ui-prop-tester/browser", include_str!("../js/browser.js")),
    (
        "ui-prop-tester/browser/defaults",
        include_str!("../js/browser_defaults.js"),
    ),
];

pub struct Loader {
    entry: Option<(PathBuf, String)>,
    cache: RefCell<HashMap<String, Module>>,
}

impl Loader {
    pub fn new(entry: Option<(PathBuf, String)>) -> Self {
        Self {
            entry: entry.map(|(path, source)| (normalize(&path), source)),
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn embedded(&self, specifier: &str, context: &mut Context) -> JsResult<Module> {
        let key = format!("embedded:{specifier}");
        if let Some(module) = self.cache.borrow().get(&key) {
            return Ok(module.clone());
        }
        let source = EMBEDDED
            .iter()
            .find(|(name, _)| *name == specifier)
            .map(|(_, source)| *source)
            .ok_or_else(|| {
                JsNativeError::typ().with_message(format!(
                    "there is no module named {specifier}; the ui-prop-tester modules are {}",
                    EMBEDDED
                        .iter()
                        .map(|(name, _)| *name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
        let path = PathBuf::from(specifier);
        let module = Module::parse(
            Source::from_bytes(source.as_bytes()).with_path(&path),
            None,
            context,
        )?;
        self.cache.borrow_mut().insert(key, module.clone());
        Ok(module)
    }

    pub fn file(&self, path: &Path, context: &mut Context) -> JsResult<Module> {
        let path = normalize(path);
        let key = format!("file:{}", path.display());
        if let Some(module) = self.cache.borrow().get(&key) {
            return Ok(module.clone());
        }
        let source = match &self.entry {
            Some((entry, source)) if *entry == path => source.clone(),
            _ => std::fs::read_to_string(&path).map_err(|error| {
                JsNativeError::typ()
                    .with_message(format!("cannot read {}: {error}", path.display()))
            })?,
        };
        let is_typescript = matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "mts" | "cts")
        );
        let source = if is_typescript {
            strip_types(&source).map_err(|message| {
                JsNativeError::syntax().with_message(format!(
                    "cannot strip the types from {}: {message}",
                    path.display()
                ))
            })?
        } else {
            source
        };
        let module = Module::parse(
            Source::from_bytes(source.as_bytes()).with_path(&path),
            None,
            context,
        )?;
        self.cache.borrow_mut().insert(key, module.clone());
        Ok(module)
    }

    fn resolve(
        &self,
        referrer: Option<&Path>,
        specifier: &str,
        context: &mut Context,
    ) -> JsResult<Module> {
        if specifier == "ui-prop-tester" || specifier.starts_with("ui-prop-tester/") {
            return self.embedded(specifier, context);
        }
        let relative = specifier.starts_with("./") || specifier.starts_with("../");
        let target = if relative {
            let base = referrer
                .filter(|path| path.is_absolute())
                .and_then(Path::parent)
                .ok_or_else(|| {
                    JsNativeError::typ().with_message(format!(
                        "cannot import {specifier}: relative imports work only from the specification file"
                    ))
                })?;
            base.join(specifier)
        } else if Path::new(specifier).is_absolute() {
            PathBuf::from(specifier)
        } else {
            return Err(JsNativeError::typ()
                .with_message(format!(
                    "cannot import {specifier}: a specification can import ui-prop-tester modules and relative files only"
                ))
                .into());
        };
        self.file(&target, context)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

impl ModuleLoader for Loader {
    fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> impl std::future::Future<Output = JsResult<Module>> {
        let specifier = specifier.to_std_string_escaped();
        let result = self.resolve(referrer.path(), &specifier, &mut context.borrow_mut());
        async { result }
    }
}
