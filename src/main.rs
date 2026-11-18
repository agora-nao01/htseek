
use pulldown_cmark::{html, Options, Parser};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Config {
    debug: bool,
    browser: Option<String>,
    target: Option<PathBuf>,
}

fn log(debug: bool, message: &str) {
    if debug {
        eprintln!("[HTseek:debug] {message}");
    }
}

fn parse_args() -> Result<Config> {
    let mut config = Config {
        debug: false,
        browser: None,
        target: None,
    };

    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "-d" {
            config.debug = true;
        } else if let Some(value) = arg.strip_prefix("-q=") {
            if value.is_empty() {
                return Err("Nome do navegador vazio".into());
            }
            config.browser = Some(value.to_string());
        } else if arg == "-q" {
            config.browser = Some(
                args.next().ok_or("Faltou o nome do navegador após -q")?
            );
        } else if arg.starts_with('-') {
            return Err(format!("Argumento desconhecido: {arg}").into());
        } else if config.target.is_none() {
            config.target = Some(PathBuf::from(arg));
        } else {
            return Err("Somente um arquivo pode ser informado".into());
        }
    }

    Ok(config)
}

fn is_html(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            e.eq_ignore_ascii_case("html")
                || e.eq_ignore_ascii_case("htm")
        })
        .unwrap_or(false)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            e.eq_ignore_ascii_case("md")
                || e.eq_ignore_ascii_case("markdown")
        })
        .unwrap_or(false)
}

// Busca o primeiro HTML encontrado na árvore
// de diretórios, sem seguir links simbólicos.
fn find_html(dir: &Path, debug: bool) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;

    let mut directories = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_file() && is_html(&path) {
            log(debug, &format!("Encontrado: {}", path.display()));
            return Some(path);
        }

        if path.is_dir()
            && !path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
        {
            directories.push(path);
        }
    }

    directories.sort();

    for directory in directories {
        if let Some(found) = find_html(&directory, debug) {
            return Some(found);
        }
    }

    None
}

fn markdown_to_html(source: &Path, debug: bool) -> Result<PathBuf> {
    let content = fs::read_to_string(source)?;

    let parser = Parser::new_ext(
        &content,
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_FOOTNOTES,
    );

    let mut body = String::new();
    html::push_html(&mut body, parser);

    let document = format!(
        r#"<!DOCTYPE html>
<html lang="pt-BR">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>
body {{
    max-width: 850px;
    margin: 40px auto;
    padding: 0 20px;
    font-family: system-ui, sans-serif;
    line-height: 1.65;
    overflow-wrap: break-word;
}}
pre, code {{
    background: #eeeeee;
    border-radius: 5px;
}}
pre {{
    padding: 15px;
    overflow-x: auto;
}}
img {{ max-width: 100%; }}
table {{ border-collapse: collapse; }}
th, td {{ border: 1px solid #aaa; padding: 8px; }}
</style>
</head>
<body>
{}
</body>
</html>"#,
        source.file_name().unwrap_or_default().to_string_lossy(),
        body
    );

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos();

    let output = env::temp_dir().join(format!(
        "htseek-{}-{timestamp}.html",
        std::process::id()
    ));

    fs::write(&output, document)?;

    log(debug, &format!("Markdown convertido: {}", output.display()));

    Ok(output)
}

fn open_file(path: &Path, browser: Option<&str>, debug: bool) -> Result<()> {
    let absolute = fs::canonicalize(path)?;

    log(debug, &format!("Arquivo: {}", absolute.display()));

    #[cfg(target_os = "android")]
    {
        let mut command;

        if let Some(package) = browser {
            log(debug, &format!("Pacote Android: {package}"));

            command = Command::new("am");
            command.args([
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                &format!("file://{}", absolute.display()),
                "-p",
                package,
            ]);
        } else {
            log(debug, "Abrindo com o aplicativo padrão");

            command = Command::new("termux-open");
            command.arg(&absolute);
        }

        command.stdout(Stdio::null());

        let status = command.status()?;

        if !status.success() {
            return Err("Falha ao abrir o arquivo no Android".into());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut command = if let Some(browser) = browser {
            let mut cmd = Command::new(browser);
            cmd.arg(&absolute);
            cmd
        } else {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(&absolute);
            cmd
        };

        command.stdout(Stdio::null())
            .stderr(Stdio::null());

        command.spawn()?;
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(browser) = browser {
            Command::new(browser).arg(&absolute).spawn()?;
        } else {
            Command::new("cmd")
                .args(["/C", "start", "", &absolute.to_string_lossy()])
                .spawn()?;
        }
    }

    log(debug, "Comando de abertura enviado");
    Ok(())
}

fn run() -> Result<()> {
    let config = parse_args()?;

    let target = match config.target {
        Some(path) => path,
        None => {
            let current = env::current_dir()?;

            log(config.debug, "Procurando HTML recursivamente...");

            find_html(&current, config.debug)
                .ok_or("Nenhum arquivo HTML encontrado")?
        }
    };

    if !target.exists() {
        return Err(format!("Arquivo inexistente: {}", target.display()).into());
    }

    let target = if is_markdown(&target) {
        markdown_to_html(&target, config.debug)?
    } else if is_html(&target) {
        target
    } else {
        return Err("Formato não suportado. Use HTML, HTM ou MD.".into());
    };

    open_file(
        &target,
        config.browser.as_deref(),
        config.debug,
    )?;

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("HTseek: {error}");
        std::process::exit(1);
    }
}

