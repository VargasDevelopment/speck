# Speck for VS Code and Cursor

A small, declarative extension for `.spk` files: TextMate highlighting, `//`
comment toggling, bracket matching, and paired brackets/quotes. It has no
activation code, runtime dependencies, language server, or completion provider.
Your editor theme controls the colors.

## Try it from this checkout

From the repository root, launch a development window:

```sh
code --extensionDevelopmentPath="$PWD/editors/vscode" "$PWD/examples/platform_array.spk"
# Or use Cursor:
cursor --extensionDevelopmentPath="$PWD/editors/vscode" "$PWD/examples/platform_array.spk"
```

The editor CLI must be on PATH. This loads the extension in that development
window; no npm install or build is needed. Open a `.spk` file and check that the
status bar says **Speck**. If needed, click the language mode and select Speck.

## Install locally

Create a folder named `vargasdevelopment.speck-language-0.1.0` inside your
editor's extensions directory:

| Editor | macOS / Linux | Windows |
| --- | --- | --- |
| VS Code | `~/.vscode/extensions/` | `%USERPROFILE%\.vscode\extensions\` |
| Cursor | `~/.cursor/extensions/` | `%USERPROFILE%\.cursor\extensions\` |

Copy `package.json`, `language-configuration.json`, `syntaxes/`, `README.md`,
and `LICENSE` from this directory into that folder. Restart the editor or run
**Developer: Reload Window**. For a remote editor session, install in the
extensions directory on that remote host. To update, replace those files; to
remove, delete the folder and reload. Nothing is published to a marketplace.

## Highlighting scope

The grammar covers lifecycle/declaration/control keywords, primitive types,
function declarations and calls, named types in declarations and signatures,
CRuMB built-in calls/key constants, operators, decimal numbers, title strings,
and line comments. It also recognizes `import "rooms.spk" as rooms` and `::`
qualified paths; syntax coloring does not imply that a particular compiler
version supports modules. Check the [language reference](../../docs/language.md)
for compiler support.

Speck currently uses raw, single-line title strings: a backslash does not escape
a quote. Numbers support integer and decimal forms, without exponent notation,
hexadecimal notation, or numeric separators. The grammar follows those lexical
limits. Coloring is syntactic; it does not resolve symbols, infer types, validate
programs, or recognize every declaration/call split across unusual line breaks.

## Test changes

Node.js 20 or newer and npm are needed only for development:

```sh
cd editors/vscode
npm ci
npm test
```

The pinned, development-only `vscode-textmate` and `vscode-oniguruma` packages
run the grammar through the same [TextMate engine used by VS Code](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide).
Tests cover keyword/built-in drift against the compiler, Unicode titles,
comments, raw backslashes, number boundaries, operators, declaration contexts,
module paths, and representative nested platform data adapted from BOOTS.
Use **Developer: Inspect Editor Tokens and Scopes** in a development window to
inspect theme-specific results. Compiler contributions still require the root
[contributing checks](../../CONTRIBUTING.md).
