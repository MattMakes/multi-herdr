---
name: swift-security-expert
description: Use when working with iOS/macOS Keychain Services (SecItem queries, kSecClass, OSStatus errors), biometric authentication (LAContext, Face ID, Touch ID), CryptoKit (AES-GCM, ChaChaPoly, ECDSA, ECDH, HPKE, ML-KEM), Secure Enclave, secure credential storage (OAuth tokens, API keys), certificate pinning (SecTrust, SPKI), keychain sharing across apps/extensions, migrating secrets from UserDefaults or plists, or OWASP MASVS/MASTG mobile compliance on Apple platforms.
---

# Keychain & Security Expert Skill

Correctness-focused facts and Apple-documented patterns for client-side security on Apple platforms: iOS 13+ minimum, modern recommendations for iOS 17+, post-quantum guidance for iOS 26. Out of scope: App Transport Security, URLSession TLS configuration, CloudKit encryption, server-side auth, code signing, jailbreak detection and third-party crypto libraries. Say so briefly when a task falls there.

---

## Decision Tree

Determine the task's intent: REVIEW (audit existing code), IMPROVE (migrate or modernize) or IMPLEMENT (build from scratch). Follow the matching branch. If the task is ambiguous, message the orchestrator.


---

### Branch 1 — REVIEW (Audit Existing Code)

**Goal:** Systematically evaluate existing keychain/security code for correctness, security, and compliance.

**Procedure:**

1. **Run the Top-Level Review Checklist** (below) against the code under review. Score each item ✅ / ❌ / ⚠️ N/A.
2. **For each ❌ failure**, load the cited reference file and locate the specific anti-pattern or correct pattern.
3. **Cross-check anti-patterns** — scan code against all 10 entries in `common-anti-patterns.md`. Pay special attention to: `UserDefaults` for secrets (#1), hardcoded keys (#2), `LAContext.evaluatePolicy()` as sole auth gate (#3), ignored `OSStatus` (#4).
4. **Check compliance** — if the project requires OWASP MASVS or enterprise audit readiness, map findings to `compliance-owasp-mapping.md` categories M1, M3, M9, M10.
5. **Report format:** For each finding, state: what's wrong → which reference file covers it → the ✅ correct pattern → severity (CRITICAL / HIGH / MEDIUM).

**Key reference files for review:**

- Start with: `common-anti-patterns.md` (backbone — covers 10 most dangerous patterns)
- Then domain-specific files based on what the code does
- Finish with: `compliance-owasp-mapping.md` (if compliance is relevant)

---

### Branch 2 — IMPROVE (Migrate / Modernize)

**Goal:** Upgrade existing code from insecure storage, deprecated APIs, or legacy patterns to current best practices.

**Procedure:**

1. **Identify the migration type:**
   - Insecure storage → Keychain: Load `migration-legacy-stores.md` + `credential-storage-patterns.md`
   - Legacy Security framework → CryptoKit: Load `cryptokit-symmetric.md` or `cryptokit-public-key.md` + `migration-legacy-stores.md`
   - RSA → Elliptic Curve: Load `cryptokit-public-key.md` (RSA migration section)
   - GenericPassword → InternetPassword (AutoFill): Load `keychain-item-classes.md` (migration section)
   - LAContext-only → Keychain-bound biometrics: Load `biometric-authentication.md`
   - File-based keychain → Data protection keychain (macOS): Load `keychain-fundamentals.md` (TN3137 section)
   - Single app → Shared keychain (extensions): Load `keychain-sharing.md`
   - Leaf pinning → SPKI/CA pinning: Load `certificate-trust.md`

2. **Follow the migration pattern** in the relevant reference file. Every migration section includes: pre-migration validation, atomic migration step, legacy data secure deletion, post-migration verification.

3. **Run the domain-specific checklist** from the reference file after migration completes.

4. **Verify no regressions** using guidance from `testing-security-code.md`.

---

### Branch 3 — IMPLEMENT (Build from Scratch)

**Goal:** Build new keychain/security functionality correctly from the start.

**Procedure:**

1. **Identify which domain(s) the task touches.** Use the Domain Selection Guide below.
2. **Load the relevant reference file(s).** Follow ✅ code patterns — never deviate from them for the core security logic.
3. **Apply Core Guidelines** (below) to every implementation.
4. **Run the domain-specific checklist** before considering the implementation complete.
5. **Add tests** following `testing-security-code.md` — protocol-based abstraction for unit tests, real keychain for integration tests on device.

**Domain Selection Guide:**

| If the task involves… | Load these reference files |
| --- | --- |
| Storing/reading a password or token | `keychain-fundamentals.md` + `credential-storage-patterns.md` |
| Choosing which `kSecClass` to use | `keychain-item-classes.md` |
| Setting when items are accessible | `keychain-access-control.md` |
| Face ID / Touch ID gating | `biometric-authentication.md` + `keychain-access-control.md` |
| Hardware-backed keys | `secure-enclave.md` |
| Encrypting / hashing data | `cryptokit-symmetric.md` |
| Signing / key exchange / HPKE | `cryptokit-public-key.md` |
| OAuth tokens / API keys / logout | `credential-storage-patterns.md` |
| Sharing between app and extension | `keychain-sharing.md` |
| TLS pinning / client certificates | `certificate-trust.md` |
| Replacing UserDefaults / plist secrets | `migration-legacy-stores.md` |
| Writing tests for security code | `testing-security-code.md` |
| Enterprise audit / OWASP compliance | `compliance-owasp-mapping.md` |

---

## Core Guidelines

These seven rules are non-negotiable. Every keychain/security implementation must satisfy all of them.

**1. Never ignore `OSStatus`.** Every `SecItem*` call returns an `OSStatus`. Use an exhaustive `switch` covering at minimum: `errSecSuccess`, `errSecDuplicateItem` (-25299), `errSecItemNotFound` (-25300), `errSecInteractionNotAllowed` (-25308). Silently discarding the return value is the root cause of most keychain bugs. → `keychain-fundamentals.md`

**2. Never use `LAContext.evaluatePolicy()` as a standalone auth gate.** This returns a `Bool` that is trivially patchable at runtime via Frida. Biometric authentication must be keychain-bound: store the secret behind `SecAccessControl` with `.biometryCurrentSet`, then let the keychain prompt for Face ID/Touch ID during `SecItemCopyMatching`. The keychain handles authentication in the Secure Enclave — there is no `Bool` to patch. → `biometric-authentication.md`

**3. Never store secrets in `UserDefaults`, `Info.plist`, `.xcconfig`, or `NSCoding` archives.** These produce plaintext artifacts readable from unencrypted backups. The Keychain is the only Apple-sanctioned store for credentials. → `credential-storage-patterns.md`, `common-anti-patterns.md`

**4. Never call `SecItem*` on `@MainActor`.** Every keychain call is an IPC round-trip to `securityd` that blocks the calling thread. Use a dedicated `actor` (iOS 17+) or serial `DispatchQueue` (iOS 13–16) for all keychain access. → `keychain-fundamentals.md`

**5. Always set `kSecAttrAccessible` explicitly.** The system default (`kSecAttrAccessibleWhenUnlocked`) breaks all background operations and may not match your threat model. Choose the most restrictive class that satisfies your access pattern. For background tasks: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. For highest sensitivity: `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly`. → `keychain-access-control.md`

**6. Always use the add-or-update pattern.** `SecItemAdd` followed by `SecItemUpdate` on `errSecDuplicateItem`. Never delete-then-add (creates a race window and destroys persistent references). Never call `SecItemAdd` without handling the duplicate case. → `keychain-fundamentals.md`

**7. Always target the data protection keychain on macOS.** Set `kSecUseDataProtectionKeychain: true` for every `SecItem*` call on macOS targets. Without it, queries silently route to the legacy file-based keychain which has different behavior, ignores unsupported attributes, and cannot use biometric protection or Secure Enclave keys. Mac Catalyst and iOS-on-Mac do this automatically. → `keychain-fundamentals.md`

---

## Quick Reference Tables

### Accessibility Constants — Selection Guide

| Constant | When Decryptable | Survives Backup | Survives Device Migration | Background Safe | Use When |
| --- | --- | --- | --- | --- | --- |
| `WhenPasscodeSetThisDeviceOnly` | Unlocked + passcode set | ❌ | ❌ | ❌ | Highest-security secrets; removed if passcode removed |
| `WhenUnlockedThisDeviceOnly` | Unlocked | ❌ | ❌ | ❌ | Device-bound secrets not needed in background |
| `WhenUnlocked` | Unlocked | ✅ | ✅ | ❌ | Syncable secrets (system default — avoid implicit use) |
| `AfterFirstUnlockThisDeviceOnly` | After first unlock → restart | ❌ | ❌ | ✅ | **Background tasks, push handlers, device-bound** |
| `AfterFirstUnlock` | After first unlock → restart | ✅ | ✅ | ✅ | Background tasks that must survive restore |

**Deprecated (never use):** `kSecAttrAccessibleAlways`, `kSecAttrAccessibleAlwaysThisDeviceOnly` — deprecated iOS 12.

**Rule of thumb:** Need background access (push handlers, background refresh)? Start with `AfterFirstUnlockThisDeviceOnly`. Foreground-only? Start with `WhenUnlockedThisDeviceOnly`. Tighten to `WhenPasscodeSetThisDeviceOnly` for high-value secrets. Use non-`ThisDeviceOnly` variants only when iCloud sync or backup migration is required.

### CryptoKit Algorithm Selection

| Need | Algorithm | Min iOS | Notes |
| --- | --- | --- | --- |
| Hash data | `SHA256` / `SHA384` / `SHA512` | 13 | `SHA3_256`/`SHA3_512` available iOS 18+ |
| Authenticate data (MAC) | `HMAC<SHA256>` | 13 | Always verify with constant-time comparison (built-in) |
| Encrypt data (authenticated) | `AES.GCM` | 13 | 256-bit key, 96-bit nonce, 128-bit tag. **Never reuse nonce with same key** |
| Encrypt data (mobile-optimized) | `ChaChaPoly` | 13 | Better on devices without AES-NI (older Apple Watch) |
| Sign data | `P256.Signing` / `Curve25519.Signing` | 13 | Use P256 for interop, Curve25519 for performance |
| Key agreement | `P256.KeyAgreement` / `Curve25519.KeyAgreement` | 13 | Always derive symmetric key via `HKDF` — never use raw shared secret |
| Hybrid public-key encryption | `HPKE` | 17 | Replaces manual ECDH+HKDF+AES-GCM chains |
| Hardware-backed signing | `SecureEnclave.P256.Signing` | 13 | P256 only; key never leaves hardware |
| Post-quantum key exchange | `MLKEM768` | 26 | Formal verification (ML-KEM FIPS 203) |
| Post-quantum signing | `MLDSA65` | 26 | Formal verification (ML-DSA FIPS 204) |
| Password → key derivation | PBKDF2 (via `CommonCrypto`) | 13 | ≥600,000 iterations SHA-256 (OWASP 2024) |
| Key → key derivation | `HKDF<SHA256>` | 13 | Extract-then-expand; always use info parameter for domain separation |

### Anti-Pattern Detection — Quick Scan

When reviewing code, search for these patterns. Any match is a finding.
`❌` = insecure pattern signature to detect in user code. `✅` = apply the corrective pattern in the referenced file.

| Search For | Anti-Pattern | Severity | Reference |
| --- | --- | --- | --- |
| `UserDefaults.standard.set` + token/key/secret/password | Plaintext credential storage | CRITICAL | `common-anti-patterns.md` #1 |
| Hardcoded base64/hex strings (≥16 chars) in source | Hardcoded cryptographic key | CRITICAL | `common-anti-patterns.md` #2 |
| `evaluatePolicy` without `SecItemCopyMatching` nearby | LAContext-only biometric gate | CRITICAL | `common-anti-patterns.md` #3 |
| `SecItemAdd` without checking return / `OSStatus` | Ignored error code | HIGH | `common-anti-patterns.md` #4 |
| No `kSecAttrAccessible` in add dictionary | Implicit accessibility class | HIGH | `common-anti-patterns.md` #5 |
| `AES.GCM.Nonce()` inside a loop with same key | Potential nonce reuse | CRITICAL | `common-anti-patterns.md` #6 |
| `sharedSecret.withUnsafeBytes` without HKDF | Raw shared secret as key | HIGH | `common-anti-patterns.md` #7 |
| `kSecAttrAccessibleAlways` | Deprecated accessibility | HIGH | `keychain-access-control.md` |
| `SecureEnclave.isAvailable` without `#if !targetEnvironment(simulator)` | Simulator false-negative trap | MEDIUM | `secure-enclave.md` |
| `kSecAttrSynchronizable: true` + `ThisDeviceOnly` | Contradictory constraints | MEDIUM | `keychain-item-classes.md` |
| `SecTrustEvaluate` (sync, deprecated) | Legacy trust evaluation | MEDIUM | `certificate-trust.md` |
| `kSecClassGenericPassword` + `kSecAttrServer` | Wrong class for web credentials | MEDIUM | `keychain-item-classes.md` |

---

## Top-Level Review Checklist

Use this checklist for a rapid sweep across all 14 domains. Each item maps to one or more reference files for deep-dive investigation. For domain-specific deep checks, use the Summary Checklist at the bottom of each reference file.

- [ ] **1. Secrets are in Keychain, not UserDefaults/plist/source** — No credentials, tokens, or cryptographic keys in `UserDefaults`, `Info.plist`, `.xcconfig`, hardcoded strings, or `NSCoding` archives. OWASP M9 (Insecure Data Storage) directly violated. → `common-anti-patterns.md` #1–2, `credential-storage-patterns.md`, `migration-legacy-stores.md`, `compliance-owasp-mapping.md`

- [ ] **2. Every `OSStatus` is checked** — All `SecItem*` calls handle return codes with exhaustive `switch` or equivalent. No ignored returns. `errSecInteractionNotAllowed` is handled non-destructively (retry later, never delete). → `keychain-fundamentals.md`, `common-anti-patterns.md` #4

- [ ] **3. Biometric auth is keychain-bound** — If biometrics are used, authentication is enforced via `SecAccessControl` + keychain access, not `LAContext.evaluatePolicy()` alone. → `biometric-authentication.md`, `common-anti-patterns.md` #3

- [ ] **4. Accessibility classes are explicit and correct** — Every keychain item has an explicit `kSecAttrAccessible` value matching its access pattern (background vs foreground, device-bound vs syncable). No deprecated `Always` constants. → `keychain-access-control.md`

- [ ] **5. No `SecItem*` calls on `@MainActor`** — All keychain operations run on a dedicated `actor` or background queue. No synchronous keychain access in UI code, `viewDidLoad`, or `application(_:didFinishLaunchingWithOptions:)`. → `keychain-fundamentals.md`

- [ ] **6. Correct `kSecClass` for each item type** — Web credentials use `InternetPassword` (not GenericPassword) for AutoFill. Cryptographic keys use `kSecClassKey` with proper `kSecAttrKeyType`. App secrets use `GenericPassword` with `kSecAttrService` + `kSecAttrAccount`. → `keychain-item-classes.md`

- [ ] **7. CryptoKit used correctly** — Nonces never reused with the same key. ECDH shared secrets always derived through `HKDF` before use as symmetric keys. `SymmetricKey` material stored in Keychain, not in memory or files. Crypto operations covered by protocol-based unit tests. → `cryptokit-symmetric.md`, `cryptokit-public-key.md`, `testing-security-code.md`

- [ ] **8. Secure Enclave constraints respected** — SE keys are P256 only (classical), never imported (always generated on-device), device-bound (no backup/sync). Availability checks guard against simulator and keychain-access-groups entitlement issues. → `secure-enclave.md`

- [ ] **9. Sharing and access groups configured correctly** — `kSecAttrAccessGroup` uses full `TEAMID.group.identifier` format. Entitlements match between app and extensions. No accidental cross-app data exposure. → `keychain-sharing.md`

- [ ] **10. Certificate trust evaluation is current** — Uses `SecTrustEvaluateAsyncWithError` (not deprecated synchronous `SecTrustEvaluate`). Pinning strategy uses SPKI hash or `NSPinnedDomains` (not leaf certificate pinning which breaks on annual rotation). → `certificate-trust.md`

- [ ] **11. macOS targets data protection keychain** — All macOS `SecItem*` calls include `kSecUseDataProtectionKeychain: true` (except Mac Catalyst / iOS-on-Mac where it's automatic). → `keychain-fundamentals.md`

---

## References Index

| # | File | One-Line Description | Risk |
| --- | --- | --- | --- |
| 1 | `keychain-fundamentals.md` | SecItem\* CRUD, query dictionaries, OSStatus handling, actor-based wrappers, macOS TN3137 routing | CRITICAL |
| 2 | `keychain-item-classes.md` | Five kSecClass types, composite primary keys, GenericPassword vs InternetPassword, ApplicationTag vs ApplicationLabel | HIGH |
| 3 | `keychain-access-control.md` | Seven accessibility constants, SecAccessControl flags, data protection tiers, NSFileProtection sidebar | CRITICAL |
| 4 | `biometric-authentication.md` | Keychain-bound biometrics, LAContext bypass vulnerability, enrollment change detection, fallback chains | CRITICAL |
| 5 | `secure-enclave.md` | Hardware-backed P256 keys, CryptoKit SecureEnclave module, persistence, simulator traps, iOS 26 post-quantum | HIGH |
| 6 | `cryptokit-symmetric.md` | SHA-2/3 hashing, HMAC, AES-GCM/ChaChaPoly encryption, SymmetricKey management, nonce handling, HKDF/PBKDF2 | HIGH |
| 7 | `cryptokit-public-key.md` | ECDSA signing, ECDH key agreement, HPKE (iOS 17+), ML-KEM/ML-DSA post-quantum (iOS 26+), curve selection | HIGH |
| 8 | `credential-storage-patterns.md` | OAuth2/OIDC token lifecycle, API key storage, refresh token rotation, runtime secrets, logout cleanup | CRITICAL |
| 9 | `keychain-sharing.md` | Access groups, Team ID prefixes, app extensions, Keychain Sharing vs App Groups entitlements, iCloud sync | MEDIUM |
| 10 | `certificate-trust.md` | SecTrust evaluation, SPKI/CA/leaf pinning, NSPinnedDomains, client certificates (mTLS), trust policies | HIGH |
| 11 | `migration-legacy-stores.md` | UserDefaults/plist/NSCoding → Keychain migration, secure deletion, first-launch cleanup, versioned migration | MEDIUM |
| 12 | `common-anti-patterns.md` | Top 10 AI-generated security mistakes with ❌/✅ code pairs, detection heuristics, OWASP mapping | CRITICAL |
| 13 | `testing-security-code.md` | Protocol-based mocking, simulator vs device differences, CI/CD keychain, Swift Testing, mutation testing | MEDIUM |
| 14 | `compliance-owasp-mapping.md` | OWASP Mobile Top 10 (2024), MASVS v2.1.0, MASTG test IDs, M1/M3/M9/M10 mapping, audit readiness | MEDIUM |

---

## Authoritative Sources

These are the primary sources underpinning all reference files. When in doubt, defer to these over any secondary source.

- **Apple Keychain Services Documentation** — canonical API reference
- **Apple Platform Security Guide** (updated annually) — architecture and encryption design
- **TN3137: "On Mac Keychain APIs and Implementations"** — macOS data protection vs file-based keychain
- **Quinn "The Eskimo!" DTS Posts** — "SecItem: Fundamentals" and "SecItem: Pitfalls and Best Practices" (updated through 2025)
- **WWDC 2019 Session 709** — "Cryptography and Your Apps" (CryptoKit introduction)
- **WWDC 2025 Session 314** — "Get ahead with quantum-secure cryptography" (ML-KEM, ML-DSA)
- **OWASP Mobile Top 10 (2024)** + **MASVS v2.1.0** + **MASTG v2** — compliance framework
- **CISA/FBI "Product Security Bad Practices" v2.0** (January 2025) — hardcoded credentials classified as national security risk

---

## Agent Rules

### Common AI Mistakes — The 10 Most Likely Incorrect Outputs

Before finalizing any output, scan for all 10. Each links to the reference file containing the correct pattern.
Each entry is intentionally paired: `❌` incorrect generated behavior and `✅` corrective pattern to use instead.

**Mistake #1 — Generating `LAContext.evaluatePolicy()` as the sole biometric gate.** AI produces the boolean-callback pattern where `evaluatePolicy` returns `success: Bool` and the app gates access on that boolean. The boolean exists in hookable user-space memory — Frida/objection bypass it with one command. **✅ Correct pattern:** Store a secret behind `SecAccessControl` with `.biometryCurrentSet`, retrieve via `SecItemCopyMatching`. → `biometric-authentication.md`

**Mistake #2 — Suggesting `SecureEnclave.isAvailable` without simulator guard.** AI generates `if SecureEnclave.isAvailable { ... }` without `#if !targetEnvironment(simulator)`. Apple documents `isAvailable` only as "if the device supports Secure Enclave access" and says nothing about the Simulator. The Simulator has no Secure Enclave, and its `isAvailable` value is not a guaranteed contract: it is usually `false`, which silently takes the fallback path in all simulator testing. **✅ Correct pattern:** Use `#if targetEnvironment(simulator)` to throw/return a clear error at compile time, check `SecureEnclave.isAvailable` only in device builds. → `secure-enclave.md`

**Mistake #3 — Importing external keys into the Secure Enclave.** AI generates `SecureEnclave.P256.Signing.PrivateKey(rawRepresentation: someData)`. SE keys must be generated inside the hardware — there is no `init(rawRepresentation:)` on SE types. `init(dataRepresentation:)` accepts only the opaque encrypted blob from a previously created SE key. **✅ Correct pattern:** Generate inside SE, persist opaque `dataRepresentation` to keychain, restore via `init(dataRepresentation:)`. → `secure-enclave.md`

**Mistake #4 — Using `SecureEnclave.AES` or SE for symmetric encryption.** AI generates references to non-existent SE symmetric APIs. The SE's internal AES engine is not exposed as a developer API. Pre-iOS 26, the SE supports only P256 signing and key agreement. iOS 26 adds ML-KEM and ML-DSA, not symmetric primitives. **✅ Correct pattern:** Use SE for signing/key agreement; derive a `SymmetricKey` via ECDH + HKDF for encryption. → `secure-enclave.md`, `cryptokit-symmetric.md`

**Mistake #5 — Omitting `kSecAttrAccessible` in `SecItemAdd`.** AI builds add dictionaries without an accessibility attribute. The system applies `kSecAttrAccessibleWhenUnlocked` by default, which breaks background operations and makes security policy invisible in code review. **✅ Correct pattern:** Always set `kSecAttrAccessible` explicitly. → `keychain-access-control.md`

**Mistake #6 — Using `SecItemAdd` without handling `errSecDuplicateItem`.** AI checks only for `errSecSuccess`, or uses delete-then-add. Without duplicate handling, the second save silently fails. Delete-then-add creates a race window and destroys persistent references. **✅ Correct pattern:** Add-or-update pattern. → `keychain-fundamentals.md`

**Mistake #7 — Specifying explicit nonces for AES-GCM encryption.** AI creates a nonce manually and passes it to `AES.GCM.seal`. Manual nonce management invites reuse — a single reuse reveals the XOR of both plaintexts. CryptoKit generates a cryptographically random nonce automatically when you omit the parameter. **✅ Correct pattern:** Call `AES.GCM.seal(plaintext, using: key)` without a `nonce:` parameter. → `cryptokit-symmetric.md`, `common-anti-patterns.md` #6

**Mistake #8 — Using raw ECDH shared secret as a symmetric key.** AI takes the output of `sharedSecretFromKeyAgreement` and uses it directly via `withUnsafeBytes`. Raw shared secrets have non-uniform distribution. CryptoKit's `SharedSecret` deliberately has no `withUnsafeBytes` — this code requires an unsafe workaround, which is a clear signal of misuse. **✅ Correct pattern:** Always derive via `sharedSecret.hkdfDerivedSymmetricKey(...)`. → `cryptokit-public-key.md`, `common-anti-patterns.md` #7

**Mistake #9 — Claiming SHA-3 requires iOS 26.** AI conflates the post-quantum WWDC 2025 additions with the SHA-3 additions from 2024. SHA-3 family types were added in **iOS 18 / macOS 15**. iOS 26 introduced ML-KEM and ML-DSA, not SHA-3. **✅ Correct version tags:** SHA-3 → iOS 18+. ML-KEM/ML-DSA → iOS 26+. → `cryptokit-symmetric.md`

**Mistake #10 — Missing first-launch keychain cleanup.** AI generates a standard `@main struct MyApp: App` without keychain cleanup. Keychain items survive app uninstallation. A reinstalled app inherits stale tokens, expired keys, and orphaned credentials. **✅ Correct pattern:** Check a `UserDefaults` flag, `SecItemDelete` across all five `kSecClass` types on first launch. → `common-anti-patterns.md` #9, `migration-legacy-stores.md`

---

### Reference File Loading Rules

Load the **minimum set** of files needed to answer the query. Do not load all 14 — they total ~7,000+ lines and will dilute focus.

| Query type | Load these files | Reason |
| --- | --- | --- |
| "Review my keychain code" | `common-anti-patterns.md` → then domain-specific files based on what the code does | Anti-patterns file is the review backbone |
| "Is this biometric auth secure?" | `biometric-authentication.md` + `common-anti-patterns.md` (#3) | Boolean gate is the #1 biometric risk |
| "Store a token / password" | `keychain-fundamentals.md` + `credential-storage-patterns.md` | CRUD + lifecycle |
| "Encrypt / hash data" | `cryptokit-symmetric.md` | Symmetric operations |
| "Sign data / key exchange" | `cryptokit-public-key.md` | Asymmetric operations |
| "Use Secure Enclave" | `secure-enclave.md` + `keychain-fundamentals.md` | SE keys need keychain persistence |
| "Share keychain with extension" | `keychain-sharing.md` + `keychain-fundamentals.md` | Access groups + CRUD |
| "Migrate from UserDefaults" | `migration-legacy-stores.md` + `credential-storage-patterns.md` | Migration + target patterns |
| "TLS pinning / mTLS" | `certificate-trust.md` | Trust evaluation |
| "Which kSecClass?" | `keychain-item-classes.md` | Class selection + primary keys |
| "Set up data protection" | `keychain-access-control.md` | Accessibility constants |
| "Write tests for keychain code" | `testing-security-code.md` | Protocol mocks + CI/CD |
| "OWASP compliance audit" | `compliance-owasp-mapping.md` + `common-anti-patterns.md` | Mapping + detection |
| "Full security review" | `common-anti-patterns.md` + all files touched by the code | Start with anti-patterns, expand |

**Loading order:** (1) Most specific file for the query. (2) Add `common-anti-patterns.md` for any review/audit. (3) Add `keychain-fundamentals.md` for any `SecItem*` task. (4) Add `compliance-owasp-mapping.md` only if OWASP/audit is mentioned. (5) Never load files speculatively.

---

### Output Format Rules

**1. Always include ✅/❌ code examples.** Show both the incorrect/insecure version and the correct/secure version. Exception: pure informational queries ("what accessibility constants exist?") do not need ❌ examples.

**2. Always cite iOS version requirements.** Every API recommendation must include the minimum iOS version inline: "Use `HPKE` (iOS 17+) for hybrid public-key encryption."

**3. Always cite the reference file.** When referencing a pattern or anti-pattern, name the source: "See `biometric-authentication.md` for the full keychain-bound pattern."

**4. Always include `OSStatus` handling in keychain code.** Never output bare `SecItemAdd` / `SecItemCopyMatching` calls without error handling. At minimum: `errSecSuccess`, `errSecDuplicateItem` (for add), `errSecItemNotFound` (for read), `errSecInteractionNotAllowed` (non-destructive retry).

**5. Always specify `kSecAttrAccessible` in add examples.** Every `SecItemAdd` code example must include an explicit accessibility constant.

**6. State severity for findings.** CRITICAL = exploitable vulnerability. HIGH = silent data loss or wrong security boundary. MEDIUM = suboptimal but not immediately exploitable.

**7. Prefer modern APIs with fallback notes.** Default to iOS 17+ (actor-based). Note fallbacks: iOS 15–16 (serial DispatchQueue + async/await bridge), iOS 13–14 (completion handlers).

**8. Never fabricate citations or WWDC session numbers.** If a session/reference is not in the loaded references, say it is unverified and avoid inventing identifiers.

**9. Implementation and improvement responses must conclude with a `## Reference Files` section.** List every reference file that informed the response with a one-line note on what it contributed. This applies to all response types — code generation, migration guides, and improvements — not just reviews. Example: `- \`keychain-fundamentals.md\` — SecItem CRUD and error handling`.

---

### Behavioral Boundaries

**Things the agent must do:**

- Ground every code pattern in the reference files. If a pattern is not documented, say so and suggest verifying against Apple documentation.
- Flag when code is simulator-only tested. Simulator behavior differs for Secure Enclave, keychain, and biometrics.
- Distinguish compile-time vs runtime errors. SE key import = compile-time. Missing accessibility class = runtime (silent wrong default). Missing OSStatus check = runtime (lost error).

**Things the agent must not do:**

- Do not invent WWDC session numbers. Only cite sessions documented in the reference files.
- ✅ examples must always use native APIs — never third-party library code (KeychainAccess, SAMKeychain, Valet). When a user explicitly asks to compare native APIs with a third-party library, adopt advisory tone: present objective tradeoffs without directive rejection. Model: _"Native APIs have no dependency overhead; KeychainAccess and Valet reduce boilerplate at the cost of coupling to a third-party maintenance schedule."_ Do not say "This skill does not recommend..." — that is directive output outside the Core Guidelines.
- Do not claim Apple APIs are buggy without evidence. Guide debugging (query dictionary errors, missing entitlements, wrong keychain) before suggesting API defects.
- Do not generate Security framework code when CryptoKit covers the use case (iOS 13+).
- Do not output partial keychain operations. Never show `SecItemAdd` without `errSecDuplicateItem` fallback. Never show `SecItemCopyMatching` without `errSecItemNotFound` handling.
- Do not escalate tone beyond what the reference files support.

---

### Cross-Reference Protocol

- **Canonical source:** Each pattern has one primary reference file (per the References Index above).
- **Brief mention + redirect elsewhere:** Other files get a one-sentence summary, not the full code example.
- **Agent behavior:** Cite the canonical file. Load it for detail. Do not reconstruct patterns from secondary mentions.

---

### Version Baseline Quick Reference

| API / Feature | Minimum iOS | Common AI mistake |
| --- | --- | --- |
| CryptoKit (SHA-2, AES-GCM, P256, ECDH) | 13 | Claiming iOS 15+ |
| `SecureEnclave.P256` (CryptoKit) | 13 | Claiming iOS 15+ |
| SHA-3 (`SHA3_256`, `SHA3_384`, `SHA3_512`) | **18** | Claiming iOS 26+ |
| HPKE (`HPKE.Sender`, `HPKE.Recipient`) | **17** | Claiming iOS 15+ or iOS 18+ |
| ML-KEM / ML-DSA (post-quantum) | **26** | Conflating with SHA-3 |
| `SecAccessControl` with `.biometryCurrentSet` | 11.3 | Claiming iOS 13+ |
| `kSecUseDataProtectionKeychain` (macOS) | macOS 10.15 | Omitting entirely on macOS |
| Swift concurrency `actor` | 13 (runtime), 17+ (recommended) | Claiming iOS 15 minimum |
| `LAContext.evaluatedPolicyDomainState` | 9 | Not knowing it exists |
| `NSPinnedDomains` (declarative pinning) | 14 | Claiming iOS 16+ |

---
