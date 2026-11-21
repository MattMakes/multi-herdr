# Chrome starts with MacAppCodeSignClone disabled

Every Chrome that horch tooling launches gets
`--disable-features=MacAppCodeSignClone`. On macOS, Chrome copies its app
into a `code_sign_clone` folder at each start. A Chrome killed during its
start leaves that copy on the disk, and macOS records each copy as one
more "Google Chrome" in its privacy lists. The switch stops the copy.
The launchers are `crates/ui-prop-tester/src/browser/process.rs` and the
skill scripts `skills/video-capture/scripts/capture.mjs`,
`skills/video-capture/scripts/materials.mjs` and
`skills/video-qa/scripts/text_audit.mjs`.
