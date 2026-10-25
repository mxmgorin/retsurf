# Security

## Supported versions

| Version | Supported |
|---|---|
| Latest release | yes |
| Nightly | yes |
| Older releases | no |

## Reporting a vulnerability

Report it privately: the **Security** tab of this repository, **Report a
vulnerability** ([form](https://github.com/mxmgorin/retsurf/security/advisories/new)).
Do not open a public issue for it.

Include the retsurf version (Settings > About), the device and firmware or OS,
and the steps to reproduce.

## Scope

- retsurf's own code: the in-app updater, the `retsurf:` pages, the local game
  server (`retsurf PATH`), input and settings handling, the release packages
- The web engine (Servo, SpiderMonkey, WebRender): report to Servo under its
  [security policy](https://github.com/servo/servo/blob/main/SECURITY.md)

## Fixes

A fix ships in the nightly build and the next release, with a GitHub security
advisory published alongside it.
