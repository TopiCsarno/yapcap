# YapCap

COSMIC panel applet for Codex, Claude Code, and Cursor usage.

This branch is a template-based rebuild of YapCap. The current baseline keeps
the COSMIC applet template structure, project metadata, app identity, resources,
fixtures, and reference docs in place before the old implementation is ported
back in small reviewed slices.

## Status

This is not the full YapCap application yet. The previous implementation lives
outside this checkout and will be ported gradually into the template
architecture.

## Development

A [justfile](./justfile) is included for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project
- `just check-json` can be used by IDEs that support LSP

## Resources

- App ID: `com.topi.YapCap`
- Repository: <https://github.com/TopiCsarno/yapcap>
- Fixture data: [fixtures](./fixtures)
- Project spec: [docs/spec.md](./docs/spec.md)

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the
`vendor` rule, and build with the vendored sources using the `build-vendored`
rule. When installing files, use the `rootdir` and `prefix` variables to change
installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/yapcap prefix=/usr install
```

## Translators

[Fluent][fluent] is used for localization. Fluent translation files live in
the [i18n directory](./i18n). New translations may copy the English
localization under `i18n/en`, rename `en` to the desired ISO 639-1 language
code, and translate each message identifier.

[fluent]: https://projectfluent.org/
[just]: https://github.com/casey/just
