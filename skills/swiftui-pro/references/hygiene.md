# Hygiene

- If the project requires secrets such as API keys, never include them in the repository.
- Code comments and documentation comments should be present where the logic isn't self-evident.
- Unit tests should exist for core application logic. UI tests only where unit tests are not possible.
- `@AppStorage` must never be used to store usernames, passwords, or other sensitive data. Use the keychain for that.
- If SwiftLint is configured, it should return no warnings or errors.
- If the project uses Localizable.xcstrings, prefer to add user-facing strings using symbol keys (e.g. “helloWorld”) in the string catalog with `extractionState` set to "manual", accessing them via generated symbols such as `Text(.helloWorld)`. If new keys need translations into the languages the project supports, tell the orchestrator in a message; translate them only if the task asks for it.
- If the Xcode MCP is configured, prefer its tools over generic alternatives. For example, `RenderPreview` is able to capture images of rendered SwiftUI previews for examination, and `DocumentationSearch` can search Apple’s documentation for latest usage instructions.
