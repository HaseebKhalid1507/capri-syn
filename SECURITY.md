# Security policy

## Supported versions

Security fixes go into the latest release. Please update before reporting.

| Version | Supported |
|---|---|
| the latest release | ✅ |
| older releases | ❌ |

## Reporting a vulnerability

**Please don't open a public issue.** Use GitHub's private reporting instead:
[Security → Report a vulnerability](https://github.com/HaseebKhalid1507/capri-syn/security/advisories/new).
Only the maintainer can see it.

Please include:
- what's affected (her app, the Linux or Windows installer, the voice sidecar, `capri-watch`)
- the version and your OS (Settings ▸ ABOUT ▸ COPY copies both; on Linux, `synaps-avatar --version`)
- steps to reproduce, and what an attacker could do with it

You should hear back within a few days. Once there's a fix, it ships in a release, and the advisory is
published with credit to you unless you'd rather stay anonymous.

## Scope

In scope: this repository's code, its installers (`install.sh`, `install.ps1`, the setup exes) and
its release assets.

Out of scope (please report these upstream):
- the Synaps agent runtime: [SynapsCLI](https://github.com/HaseebKhalid1507/SynapsCLI)
- the model providers she talks to
- third-party art packs and their sources

Worth knowing: her brain is an agent session that can run tools on your machine. Tool calls she
makes for you go through Synaps' own approval flow; a way around that approval is in scope here if
capri-syn is what lets it happen.
