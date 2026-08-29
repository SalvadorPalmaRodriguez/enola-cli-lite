// enola-context-gen — Genera un archivo de contexto de proyecto (dev-tool)
//
// Aplana un repositorio en un único archivo de texto optimizado para
// ventanas de contexto de LLMs. Usa GitCodeFlattener de domain/.
//
// Uso:
//   enola-context-gen --root <dir> --extensions <ext1,ext2> [--ignore-dirs <d1,d2>] --output <file>
//
// Ejemplo:
//   enola-context-gen --root . --extensions rs,toml,md --ignore-dirs target,node_modules --output /tmp/context.md

use enola_core::domain::git_flattening::GitCodeFlattener;
use std::path::PathBuf;
use std::process;

fn usage() -> ! {
    eprintln!(
        "Enola Context Generator — dev-tool\n\
         \n\
         Uso:\n\
           enola-context-gen --root <dir> --extensions <ext1,ext2> [--ignore-dirs <d1,d2>] --output <file>\n\
         \n\
         Ejemplo:\n\
           enola-context-gen --root . --extensions rs,toml,md --ignore-dirs target,node_modules --output /tmp/context.md"
    );
    process::exit(1);
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut root: Option<PathBuf> = None;
    let mut extensions: Vec<String> = Vec::new();
    let mut ignore_dirs: Vec<String> = Vec::new();
    let mut output: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                root = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "--extensions" => {
                i += 1;
                extensions = parse_list(args.get(i).unwrap_or_else(|| usage()));
            }
            "--ignore-dirs" => {
                i += 1;
                ignore_dirs = parse_list(args.get(i).unwrap_or_else(|| usage()));
            }
            "--output" => {
                i += 1;
                output = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            _ => usage(),
        }
        i += 1;
    }

    let root = root.unwrap_or_else(|| usage());
    let output = output.unwrap_or_else(|| usage());

    match GitCodeFlattener::flatten(&root, &extensions, &ignore_dirs, &output) {
        Ok(path) => {
            println!("✅ Contexto generado: {}", path);
        }
        Err(e) => {
            eprintln!("❌ Error generando contexto: {:?}", e);
            process::exit(1);
        }
    }
}
