# skill-intake passes git and its tests only the environment they need

`skills/skill-intake/scripts/intakelib/acquire.py` `_git_env` passes git
only the proxy and CA variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
`NO_PROXY`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `GIT_SSL_CAINFO`) and nothing
else from the environment. The Claude sandbox routes network through a
proxy it sets in these variables, and without them a sandboxed pane cannot
clone from GitHub.

## Consequences

- The Claude sandbox proxy closes a CONNECT that has no
  Proxy-Authorization instead of sending a 407 challenge (T-33). intake's
  git command passes `-c http.proxyAuthMethod=basic`, so git sends the
  proxy URL's credentials on the first connect. Without it git fails with
  "Proxy CONNECT aborted". The sandbox sets this in
  `GIT_CONFIG_PARAMETERS`, which `_git_env` drops.
  `scripts/skill-intake/tests/test_acquire.py` checks the setting.
- `scripts/skill-intake/tests/base.py` deletes every `SKILL_INTAKE_*`,
  `*_API_KEY` and horch fleet variable from the environment. No test
  passes an API key to anything (repository rule), and a test run in a
  horch pane must not see the pane's fleet variables.
