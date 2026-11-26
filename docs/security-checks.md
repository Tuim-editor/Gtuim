# Keeping secrets out of GitHub

The initial pushed commit was checked for credential/private-key patterns and
then scanned in full with Gitleaks 8.30.1: no findings. Subsequent history is
checked before each agent push. This is defense in depth, not a guarantee that
all possible sensitive information can be detected.

## Local push protection

Install [Gitleaks](https://github.com/gitleaks/gitleaks), then enable the tracked
hook in each clone:

```sh
git config core.hooksPath .githooks
bash scripts/check-secrets.sh
```

The pre-push hook scans **all reachable Git history** and fails closed if the
scanner is missing or a finding is reported. Output is fully redacted. Set
`GITLEAKS_BIN` if the executable is not on `PATH`. Hooks are local Git settings:
cloning the repository does not automatically enable them, and they can be
bypassed. They are not GitHub server-side push protection.

## CI check

The Secret scan workflow checks full fetched history on pushes and pull
requests. Its Gitleaks release archive is SHA-256 pinned, and output is redacted.
CI runs **after** a push, so the local pre-push check remains important.

`.gitignore` excludes build artifacts, logs, local environment files and common
private-key files. Ignore rules do not protect already tracked files or secrets
embedded in application source. Never embed a GitHub token, SMTP credential,
private key or personal settings in the project. Report drafts intentionally
use the reporter's own browser session rather than an embedded API credential.

If a real secret is ever committed or pushed, revoke/rotate it immediately;
deleting it in a later commit is not sufficient because Git retains history.
Do not publish raw scanner reports or credential values while investigating.
