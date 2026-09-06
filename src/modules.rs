//! File graph loading and name resolution before ordinary semantic validation.
mod resolve;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::Program;
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{self, Token};
use crate::parser::{self, Import};
use crate::source::{AnalysisError, SourceMap};

const MAX_IMPORT_DEPTH: usize = 128;

struct LoadedModule {
    tokens: Vec<Token>,
    /// Longest dependency path, counting this file, independent of discovery order.
    height: usize,
    imports: Vec<(Import, usize)>,
    qualifier: String,
}

struct Module {
    program: Program,
    imports: Vec<(Import, usize)>,
    qualifier: String,
}

pub(crate) fn load(path: &Path) -> Result<(Program, SourceMap), AnalysisError> {
    let mut loader = Loader {
        sources: SourceMap::default(),
        identities: HashMap::new(),
        active: Vec::new(),
        modules: Vec::new(),
        root_dir: PathBuf::new(),
    };
    let result = loader.visit(path, None).and_then(|_| {
        let files = loader
            .modules
            .into_iter()
            .map(Option::unwrap)
            .collect::<Vec<_>>();
        let struct_names = files
            .iter()
            .map(|file| parser::declared_struct_names(&file.tokens))
            .collect::<Vec<_>>();
        // Parse only after loading unwinds: import depth must not consume the
        // expression parser's stack budget on ordinary library caller threads.
        let mut modules = files
            .into_iter()
            .enumerate()
            .map(|(index, file)| {
                let imported_structs = file
                    .imports
                    .iter()
                    .flat_map(|(import, target)| {
                        struct_names[*target]
                            .iter()
                            .map(|name| format!("{}::{name}", import.alias))
                    })
                    .collect();
                let program = parser::parse_file(file.tokens, index == 0, imported_structs)?;
                Ok(Module {
                    program,
                    imports: file.imports,
                    qualifier: file.qualifier,
                })
            })
            .collect::<Result<Vec<_>, Vec<Diagnostic>>>()?;
        resolve::resolve(&mut modules)?;
        let mut root = modules.remove(0).program;
        for module in modules {
            root.structs.extend(module.program.structs);
            root.constants.extend(module.program.constants);
            root.globals.extend(module.program.globals);
            root.functions.extend(module.program.functions);
        }
        Ok(root)
    });
    match result {
        Ok(program) => Ok((program, loader.sources)),
        Err(diagnostics) => Err(AnalysisError {
            sources: loader.sources,
            diagnostics,
        }),
    }
}

struct Loader {
    sources: SourceMap,
    identities: HashMap<PathBuf, usize>,
    active: Vec<PathBuf>,
    modules: Vec<Option<LoadedModule>>,
    root_dir: PathBuf,
}

impl Loader {
    fn visit(&mut self, path: &Path, import_span: Option<Span>) -> Result<usize, Vec<Diagnostic>> {
        self.sources.record_dependency(path.to_owned());
        let span = import_span.unwrap_or_default();
        let error = |message| vec![Diagnostic::new(message, span)];
        if path.extension().and_then(|extension| extension.to_str()) != Some("spk") {
            return Err(error(
                "Speck source files must use the `.spk` extension".into(),
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(|failure| error(format!("could not read `{}`: {failure}", path.display())))?;
        self.sources.record_dependency(canonical.clone());
        if let Some(cycle_start) = self.active.iter().position(|active| active == &canonical) {
            let chain = self.active[cycle_start..]
                .iter()
                .chain(std::iter::once(&canonical))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(error(format!("import cycle: {chain}")));
        }
        let cached = self.identities.get(&canonical).copied();
        let height = cached.map_or(1, |index| {
            // The cycle check above excludes unfinished entries on the active path.
            self.modules[index].as_ref().unwrap().height
        });
        if self.active.len() + height > MAX_IMPORT_DEPTH {
            return Err(error(format!(
                "import nesting exceeds the limit of {MAX_IMPORT_DEPTH} files"
            )));
        }
        if let Some(index) = cached {
            return Ok(index);
        }
        let source = std::fs::read_to_string(&canonical)
            .map_err(|failure| error(format!("could not read `{}`: {failure}", path.display())))?;
        let index = self.modules.len();
        if index == 0 {
            self.root_dir = canonical.parent().unwrap().to_owned();
        }
        let qualifier = if index == 0 {
            String::new()
        } else {
            canonical
                .strip_prefix(&self.root_dir)
                .unwrap_or(&canonical)
                .to_str()
                .ok_or_else(|| error("module file paths must be valid UTF-8".into()))?
                .to_owned()
        };
        let display_path = if index == 0 {
            path.to_owned()
        } else {
            canonical.clone()
        };
        let id = self.sources.add(display_path, source);
        let tokens = lexer::lex_in(self.sources.get(id).unwrap().text(), id)?;
        let headers = parser::import_headers(&tokens)?;
        self.identities.insert(canonical.clone(), index);
        self.modules.push(None);
        self.active.push(canonical.clone());
        let mut imports = Vec::new();
        let mut aliases = HashSet::new();
        for import in headers {
            if !aliases.insert(import.alias.clone()) {
                return Err(vec![Diagnostic::new(
                    format!("import alias `{}` is declared more than once", import.alias),
                    import.span,
                )]);
            }
            let imported_path = Path::new(&import.path);
            if imported_path.is_absolute() {
                return Err(vec![Diagnostic::new(
                    "import paths must be relative to the importing file",
                    import.span,
                )]);
            }
            let target = self.visit(
                &canonical.parent().unwrap().join(imported_path),
                Some(import.span),
            )?;
            imports.push((import, target));
        }
        let height = 1 + imports
            .iter()
            .map(|(_, target)| self.modules[*target].as_ref().unwrap().height)
            .max()
            .unwrap_or(0);
        self.modules[index] = Some(LoadedModule {
            tokens,
            height,
            imports,
            qualifier,
        });
        self.active.pop();
        Ok(index)
    }
}
