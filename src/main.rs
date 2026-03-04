use clap::{Parser, Subcommand};
use memchr::memmem;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{env, fs, io, process};

static DEBUG: LazyLock<bool> =
    LazyLock::new(|| env::var("AGENTIC_DEBUG").is_ok_and(|v| !v.is_empty()));

macro_rules! debug {
    ($($arg:tt)*) => {
        if *DEBUG {
            eprintln!("[hook] {}", format!($($arg)*));
        }
    };
}

const SIZE_THRESHOLD: usize = 4096;
const CODE_HEAVY_THRESHOLD: usize = 2048;
const CONFIG_REF: &str = "@project-config.md";

// ── Embedded content ─────────────────────────────────────────────────

const AGENTS: &[(&str, &str)] = &[
    ("dev.md", include_str!("../agents/dev.md")),
    ("reviewer.md", include_str!("../agents/reviewer.md")),
];

// Files from prior installs; cleaned up on install/uninstall.
const LEGACY_REFS: &[&str] = &[
    "analyst.md",
    "architect.md",
    "auditor.md",
    "challenger.md",
    "qa.md",
    "sign-off-protocol.md",
    "worker-protocol.md",
    "orchestration-rules.md",
];

const GLOBAL_CLAUDE_MD: &str = include_str!("../architect.md");
const CODING_STANDARDS_MD: &str = include_str!("../coding-standards.md");
const PLANNING_PROTOCOL: &str = include_str!("../planning-protocol.md");

const AGENTIC_PERMISSIONS: &[&str] = &[
    // Git workflow (universal for worktree-based agents)
    "Bash(git status)",
    "Bash(git diff *)",
    "Bash(git log *)",
    "Bash(git show *)",
    "Bash(git add *)",
    "Bash(git commit *)",
    "Bash(git merge *)",
    "Bash(git branch *)",
    "Bash(git worktree *)",
    "Bash(git stash *)",
    "Bash(git checkout *)",
    "Bash(git rev-parse *)",
    // Read-only shell commands
    "Bash(ls *)",
    "Bash(cat *)",
    "Bash(head *)",
    "Bash(tail *)",
    "Bash(wc *)",
    "Bash(which *)",
    "Bash(file *)",
    "Bash(find *)",
    "Bash(diff *)",
    "Bash(tree *)",
    "Bash(echo *)",
    "Bash(pwd)",
    // Pipe targets (read-only)
    "Bash(grep *)",
    "Bash(rg *)",
    "Bash(sort *)",
    "Bash(uniq *)",
    "Bash(cut *)",
    "Bash(awk *)",
    "Bash(tr *)",
    "Bash(sed *)",
    "Bash(basename *)",
    "Bash(dirname *)",
    // File tools
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "NotebookEdit",
];

const AGENTIC_DENY: &[&str] = &[
    "Bash(sed -i *)",   // block in-place editing — piped sed is fine
    "Edit(/.claude/settings.json)",
    "Edit(/.claude/settings.local.json)",
    "Write(/.claude/settings.json)",
    "Write(/.claude/settings.local.json)",
];

// ── CLI ──────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "agentic")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Install agentic workflow globally
    Install {
        /// Pre-approve recommended permissions (git ops + read-only shell)
        #[arg(long)]
        permissions: bool,
        /// Skip permissions prompt
        #[arg(long)]
        no_permissions: bool,
    },
    /// Remove agentic workflow (agents, hooks, binary)
    Uninstall,
    /// Refresh or create project-config.md
    Refresh,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Install { permissions, no_permissions }) => install(permissions, no_permissions),
        Some(Command::Uninstall) => uninstall(),
        Some(Command::Refresh) => refresh(),
        None => hook_dispatch(),
    }
}

// ── Hook dispatch ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    cwd: String,
}

fn hook_dispatch() {
    let mut input = String::new();
    if io::Read::read_to_string(&mut io::stdin(), &mut input).is_err() {
        process::exit(1);
    }

    let hook: HookInput = match serde_json::from_str(&input) {
        Ok(h) => h,
        Err(_) => process::exit(1),
    };

    let tool = hook.tool_name.as_deref().unwrap_or("");

    let handler = match (hook.hook_event_name.as_str(), tool) {
        ("PreToolUse", "SendMessage") => Some("message_transform"),
        ("PreToolUse", "Agent") => Some("agent_accept_edits"),
        ("PreToolUse", "EnterPlanMode") => Some("planning_protocol"),
        ("PostToolUse", "Bash") => Some("post_git_write"),
        ("SessionStart", _) => Some("session_start"),
        _ => None,
    };

    debug!(
        "{} {}{}",
        hook.hook_event_name,
        tool,
        match handler {
            Some(h) => format!(" → {h}"),
            None => " → no-op".into(),
        }
    );

    match handler {
        Some("message_transform") => message_transform(&hook),
        Some("agent_accept_edits") => agent_accept_edits(&hook),
        Some("planning_protocol") => planning_protocol(),
        Some("post_git_write") => post_git_write(&hook),
        Some("session_start") => session_start(&hook),
        _ => {}
    }
}

/// Hook 1: Message Transformation
/// Intercepts large SendMessage payloads, writes content to disk,
/// replaces the message body with a file reference.
fn message_transform(hook: &HookInput) {
    let input = match &hook.tool_input {
        Some(v) => v,
        None => return,
    };

    let content = match input.get("content").and_then(Value::as_str) {
        Some(s) => s,
        None => return,
    };

    let bytes = content.as_bytes();
    let code_fences = memmem::find_iter(bytes, b"```").count();
    let threshold = if code_fences >= 2 {
        CODE_HEAVY_THRESHOLD
    } else {
        SIZE_THRESHOLD
    };

    if bytes.len() <= threshold {
        debug!("skip: {} bytes ≤ {threshold} threshold", bytes.len());
        return;
    }

    let dir = Path::new(&hook.cwd).join(".claude/messages");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    let hash = Sha256::digest(bytes);
    let name: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{name}.md"));

    if fs::write(&path, content).is_err() {
        return;
    }

    debug!("offloaded {} bytes → {}", bytes.len(), path.display());

    let summary = input
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("(see file)");
    let mut updated = input.clone();
    updated["content"] = Value::String(format!(
        "[Content offloaded to {} ({} bytes). Summary: {summary}]",
        path.display(),
        bytes.len(),
    ));

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": updated
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 2: Agent Accept Edits
/// Forces acceptEdits permission mode on all agent spawns.
fn agent_accept_edits(hook: &HookInput) {
    let input = match &hook.tool_input {
        Some(v) => v,
        None => return,
    };

    let mut updated = input.clone();
    updated["permissionMode"] = Value::String("acceptEdits".into());

    debug!("forcing acceptEdits on Agent spawn");

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": updated
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 4: Adaptive Planning Protocol
/// Injects the full planning protocol as additionalContext when plan mode is entered.
fn planning_protocol() {
    debug!("injecting planning protocol");

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": PLANNING_PROTOCOL
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

fn is_git_write_command(cmd: &str) -> bool {
    cmd.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return false;
        }
        trimmed
            .split("&&")
            .flat_map(|s| s.split(';'))
            .any(|seg| {
                let s = seg.trim();
                s.starts_with("git merge") || s.starts_with("git pull") || s.starts_with("git commit")
            })
    })
}

/// Hook 3: Post-Git-Write Project Refresh
/// After a git commit/merge/pull, re-injects project-config.md content and verifies @reference.
fn post_git_write(hook: &HookInput) {
    let command = hook
        .tool_input
        .as_ref()
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("");

    if !is_git_write_command(command) {
        debug!("not a git write command");
        return;
    }

    debug!("git write detected: {command}");

    let config_path = Path::new(&hook.cwd).join("project-config.md");
    let config_content = fs::read_to_string(&config_path).unwrap_or_default();

    // Verify @reference is still in project CLAUDE.md
    ensure_config_ref(&Path::new(&hook.cwd).join("CLAUDE.md"));

    let context = if config_content.is_empty() {
        match generate_and_write_config(Path::new(&hook.cwd)) {
            Some(lang) => format!(
                "project-config.md was auto-generated for {lang} project. \
                Review and commit if correct, then /clear to reclaim context."
            ),
            None => "Could not auto-generate project-config.md. \
                Create it manually: document build commands, test commands, \
                and verification commands. Stage and commit when done."
                .to_string(),
        }
    } else {
        format!(
            "Current project-config.md:\n{config_content}\n\n\
            Check if this needs updating to reflect the changes just made. \
            If you update it, stage and commit it separately."
        )
    };

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Hook 5: Session Start — Bootstrap
/// Ensures @project-config.md reference in project CLAUDE.md and bootstraps project-config.md if missing.
fn session_start(hook: &HookInput) {
    let cwd = Path::new(&hook.cwd);

    // Check for git repo — worktree isolation requires one
    if !cwd.join(".git").exists() {
        debug!("not a git repo — injecting git init prompt");
        let output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": "This directory is not a git repository. \
                    Dev agents require worktree isolation, which needs git. \
                    Use AskUserQuestion to ask the user: \
                    option 1: 'Initialize git repo' — run `git init` and create an initial commit, then proceed normally. \
                    option 2: 'Bail' — stop and let the user handle it. \
                    The user can also free-type a response."
            }
        });
        serde_json::to_writer(io::stdout(), &output).ok();
        return;
    }

    let claude_md = cwd.join("CLAUDE.md");
    let config_path = cwd.join("project-config.md");

    // Ensure @project-config.md reference in project CLAUDE.md
    ensure_config_ref(&claude_md);

    // Bootstrap project-config.md if missing
    if !config_path.exists() {
        debug!("project-config.md missing — attempting auto-generation");
        let context = match generate_and_write_config(cwd) {
            Some(lang) => format!(
                "project-config.md was auto-generated for {lang} project. \
                Review and commit if correct, then /clear to reclaim context."
            ),
            None => "Could not auto-generate project-config.md. \
                Create it manually: document build commands, test commands, \
                and verification commands. Stage and commit when done."
                .to_string(),
        };
        let output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context
            }
        });
        serde_json::to_writer(io::stdout(), &output).ok();
    }
}

fn ensure_config_ref(claude_md: &Path) {
    let content = fs::read_to_string(claude_md).unwrap_or_default();
    if content.lines().any(|l| l.trim() == CONFIG_REF) {
        return;
    }
    debug!("adding @project-config.md to {}", claude_md.display());
    let new_content = if content.is_empty() {
        format!("{CONFIG_REF}\n")
    } else {
        format!("{CONFIG_REF}\n\n{content}")
    };
    let _ = fs::write(claude_md, new_content);
}

// ── Project config generation ──────────────────────────────────────

struct ProjectInfo {
    name: String,
    language: String,
    description: String,
    build_commands: Vec<String>,
    test_commands: Vec<String>,
    lint_commands: Vec<String>,
    key_paths: Vec<String>,
}

fn detect_rust(cwd: &Path) -> Option<ProjectInfo> {
    let cargo_toml = cwd.join("Cargo.toml");
    if !cargo_toml.exists() {
        return None;
    }
    let content = fs::read_to_string(&cargo_toml).unwrap_or_default();

    let name = content
        .lines()
        .find(|l| l.trim_start().starts_with("name") && l.contains('='))
        .and_then(|l| l.split('=').nth(1))
        .map(|s| s.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| {
            cwd.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

    let is_workspace = content.contains("[workspace]");
    let description = if is_workspace {
        "Rust workspace".to_string()
    } else {
        String::new()
    };

    let mut key_paths = vec!["src/".to_string(), "Cargo.toml".to_string()];
    if cwd.join("Cargo.lock").exists() {
        key_paths.push("Cargo.lock".to_string());
    }

    Some(ProjectInfo {
        name,
        language: "Rust".to_string(),
        description,
        build_commands: vec!["cargo build --release".to_string()],
        test_commands: vec!["cargo test".to_string()],
        lint_commands: vec![
            "cargo clippy -- -D warnings".to_string(),
            "cargo fmt --check".to_string(),
        ],
        key_paths,
    })
}

fn detect_node(cwd: &Path) -> Option<ProjectInfo> {
    let pkg_json = cwd.join("package.json");
    if !pkg_json.exists() {
        return None;
    }
    let content = fs::read_to_string(&pkg_json).unwrap_or_default();
    let parsed: Value = serde_json::from_str(&content).unwrap_or_default();

    let name = parsed
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            cwd.file_name().and_then(|n| n.to_str()).unwrap_or("unknown")
        })
        .to_string();

    let language = if cwd.join("tsconfig.json").exists() {
        "TypeScript"
    } else {
        "JavaScript"
    };

    let pm = if cwd.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if cwd.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    };

    let scripts = parsed.get("scripts");

    let build_commands = if scripts
        .and_then(|s| s.get("build"))
        .is_some()
    {
        vec![format!("{pm} run build")]
    } else {
        vec![]
    };

    let test_commands = if scripts.and_then(|s| s.get("test")).is_some() {
        if pm == "pnpm" {
            vec![format!("{pm} run test")]
        } else {
            vec![format!("{pm} test")]
        }
    } else {
        vec![]
    };

    let lint_commands = if scripts.and_then(|s| s.get("lint")).is_some() {
        vec![format!("{pm} run lint")]
    } else {
        vec![]
    };

    let mut key_paths = vec!["package.json".to_string()];
    if cwd.join("src").exists() {
        key_paths.push("src/".to_string());
    }
    if cwd.join("tsconfig.json").exists() {
        key_paths.push("tsconfig.json".to_string());
    }

    Some(ProjectInfo {
        name,
        language: language.to_string(),
        description: String::new(),
        build_commands,
        test_commands,
        lint_commands,
        key_paths,
    })
}

fn detect_go(cwd: &Path) -> Option<ProjectInfo> {
    let go_mod = cwd.join("go.mod");
    if !go_mod.exists() {
        return None;
    }
    let content = fs::read_to_string(&go_mod).unwrap_or_default();

    let name = content
        .lines()
        .find(|l| l.trim_start().starts_with("module"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|m| {
            m.split('/').next_back().unwrap_or(m).to_string()
        })
        .unwrap_or_else(|| {
            cwd.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

    let mut key_paths = vec!["go.mod".to_string()];
    if cwd.join("go.sum").exists() {
        key_paths.push("go.sum".to_string());
    }

    Some(ProjectInfo {
        name,
        language: "Go".to_string(),
        description: String::new(),
        build_commands: vec!["go build ./...".to_string()],
        test_commands: vec!["go test ./...".to_string()],
        lint_commands: vec!["go vet ./...".to_string()],
        key_paths,
    })
}

fn detect_python(cwd: &Path) -> Option<ProjectInfo> {
    let pyproject = cwd.join("pyproject.toml");
    if !pyproject.exists() {
        return None;
    }
    let content = fs::read_to_string(&pyproject).unwrap_or_default();

    let name = content
        .lines()
        .find(|l| l.trim_start().starts_with("name") && l.contains('='))
        .and_then(|l| l.split('=').nth(1))
        .map(|s| s.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| {
            cwd.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

    let has_build_system = content.contains("[build-system]");
    let build_commands = if has_build_system {
        vec!["pip install -e .".to_string()]
    } else {
        vec![]
    };

    let test_commands = if content.lines().any(|l| l.contains("pytest")) {
        vec!["pytest".to_string()]
    } else {
        vec!["python -m unittest".to_string()]
    };

    let lint_commands = if content.lines().any(|l| l.contains("ruff")) {
        vec!["ruff check .".to_string()]
    } else {
        vec![]
    };

    let mut key_paths = vec!["pyproject.toml".to_string()];
    if cwd.join("src").exists() {
        key_paths.push("src/".to_string());
    }

    Some(ProjectInfo {
        name,
        language: "Python".to_string(),
        description: String::new(),
        build_commands,
        test_commands,
        lint_commands,
        key_paths,
    })
}

fn detect_makefile(cwd: &Path) -> Option<ProjectInfo> {
    let makefile = cwd.join("Makefile");
    if !makefile.exists() {
        return None;
    }
    let content = fs::read_to_string(&makefile).unwrap_or_default();

    let name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let has_target = |target: &str| -> bool {
        content
            .lines()
            .any(|l| l.starts_with(&format!("{target}:")))
    };

    let build_commands = if has_target("build") {
        vec!["make build".to_string()]
    } else if has_target("all") {
        vec!["make".to_string()]
    } else {
        vec![]
    };

    let test_commands = if has_target("test") {
        vec!["make test".to_string()]
    } else {
        vec![]
    };

    let lint_commands = if has_target("lint") {
        vec!["make lint".to_string()]
    } else if has_target("check") {
        vec!["make check".to_string()]
    } else {
        vec![]
    };

    Some(ProjectInfo {
        name,
        language: "Make".to_string(),
        description: String::new(),
        build_commands,
        test_commands,
        lint_commands,
        key_paths: vec!["Makefile".to_string()],
    })
}

fn detect_project(cwd: &Path) -> ProjectInfo {
    if let Some(info) = detect_rust(cwd) {
        return info;
    }
    if let Some(info) = detect_node(cwd) {
        return info;
    }
    if let Some(info) = detect_go(cwd) {
        return info;
    }
    if let Some(info) = detect_python(cwd) {
        return info;
    }
    if let Some(info) = detect_makefile(cwd) {
        return info;
    }
    // Fallback
    ProjectInfo {
        name: cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        language: "Unknown".to_string(),
        description: String::new(),
        build_commands: vec![],
        test_commands: vec![],
        lint_commands: vec![],
        key_paths: vec![],
    }
}

fn extract_readme_description(cwd: &Path) -> String {
    let readme = cwd.join("README.md");
    let content = match fs::read_to_string(&readme) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let mut lines = content.lines();

    // Find first line starting with `# `
    let found_heading = lines.any(|l| l.starts_with("# "));
    if !found_heading {
        return String::new();
    }

    // Collect subsequent non-empty lines until blank line or another heading
    let mut desc_lines: Vec<&str> = vec![];
    for line in lines {
        if line.is_empty() || line.starts_with('#') {
            break;
        }
        desc_lines.push(line);
    }

    if desc_lines.is_empty() {
        return String::new();
    }

    let joined = desc_lines.join(" ");
    if joined.len() > 200 {
        joined[..200].to_string()
    } else {
        joined
    }
}

fn render_project_config(info: &ProjectInfo) -> String {
    let mut out = format!("# Project: {}\n", info.name);

    if !info.description.is_empty() {
        out.push('\n');
        out.push_str(&info.description);
        out.push('\n');
    }

    out.push_str("\n## Language\n\n");
    out.push_str(&info.language);
    out.push('\n');

    if !info.build_commands.is_empty() {
        out.push_str("\n## Build\n\n```bash\n");
        for cmd in &info.build_commands {
            out.push_str(cmd);
            out.push('\n');
        }
        out.push_str("```\n");
    }

    if !info.test_commands.is_empty() {
        out.push_str("\n## Test\n\n```bash\n");
        for cmd in &info.test_commands {
            out.push_str(cmd);
            out.push('\n');
        }
        out.push_str("```\n");
    }

    // Omit Verification if it would be identical to just Test (no lint commands)
    if !info.lint_commands.is_empty() {
        out.push_str("\n## Verification\n\n```bash\n");
        for cmd in &info.lint_commands {
            out.push_str(cmd);
            out.push('\n');
        }
        for cmd in &info.test_commands {
            out.push_str(cmd);
            out.push('\n');
        }
        out.push_str("```\n");
    }

    if !info.key_paths.is_empty() {
        out.push_str("\n## Key Paths\n\n");
        for path in &info.key_paths {
            out.push_str(&format!("- {path}\n"));
        }
    }

    out
}

fn generate_and_write_config(cwd: &Path) -> Option<String> {
    let config_path = cwd.join("project-config.md");
    // Never overwrite existing file
    if config_path.exists() {
        return None;
    }

    let mut info = detect_project(cwd);
    info.description = extract_readme_description(cwd);

    let content = render_project_config(&info);

    match fs::write(&config_path, &content) {
        Ok(_) => {
            debug!("wrote project-config.md for {} project", info.language);
            Some(info.language)
        }
        Err(e) => {
            eprintln!("Failed to write project-config.md: {e}");
            None
        }
    }
}

// ── Install, Uninstall & Refresh ────────────────────────────────────

fn refresh() {
    let cwd = env::current_dir().unwrap_or_default();
    if cwd.join("project-config.md").exists() {
        println!("Refresh project-config.md: review and update to reflect current project state. \
            If changed, stage and commit separately.");
    } else {
        match generate_and_write_config(&cwd) {
            Some(lang) => println!(
                "project-config.md was auto-generated for {lang} project. \
                Review and commit if correct, then /clear to reclaim context."
            ),
            None => println!(
                "Could not auto-generate project-config.md. \
                Create it manually: document build commands, test commands, \
                and verification commands. Stage and commit when done."
            ),
        }
    }
}

fn home_dir() -> PathBuf {
    PathBuf::from(env::var("HOME").expect("HOME not set"))
}

fn write_file(content: &str, dst: &Path) {
    fs::write(dst, content).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {e}", dst.display());
        process::exit(1);
    });
    println!("ok    {}", dst.display());
}

fn install(permissions: bool, no_permissions: bool) {
    let home = home_dir();
    let claude_dir = home.join(".claude");

    // Agents
    let agents_dir = claude_dir.join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    for (name, content) in AGENTS {
        write_file(content, &agents_dir.join(name));
    }

    // Remove files from prior installs
    for name in LEGACY_REFS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {} (legacy)", path.display());
        }
    }
    // Also clean standalone legacy files from the claude dir root
    for name in &["sign-off-protocol.md", "worker-protocol.md", "orchestration-rules.md"] {
        let path = claude_dir.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {} (legacy)", path.display());
        }
    }

    // Global CLAUDE.md + coding-standards.md
    write_file(GLOBAL_CLAUDE_MD, &claude_dir.join("CLAUDE.md"));
    write_file(CODING_STANDARDS_MD, &claude_dir.join("coding-standards.md"));

    // Install binary
    let bin_dir = home.join(".local/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let exe = env::current_exe().unwrap();
    let bin_dst = bin_dir.join("agentic");
    if exe != bin_dst {
        fs::copy(&exe, &bin_dst).unwrap_or_else(|e| {
            eprintln!("Failed to install binary: {e}");
            process::exit(1);
        });
        println!("ok    {}", bin_dst.display());
    }

    // Clean up old agentic-hooks binary
    let old_bin = bin_dir.join("agentic-hooks");
    if old_bin.exists() {
        let _ = fs::remove_file(&old_bin);
        println!("rm    {} (renamed)", old_bin.display());
    }

    // Configure settings.json
    let settings_path = claude_dir.join("settings.json");
    let mut settings: Value = fs::read_to_string(&settings_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let obj = settings.as_object_mut().unwrap();

    obj.entry("env")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .unwrap()
        .insert(
            "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS".into(),
            Value::String("1".into()),
        );

    // Merge agentic hooks into existing config (preserve user hooks)
    let agentic_hooks: &[(&str, &[&str])] = &[
        ("PreToolUse", &["SendMessage", "Agent", "EnterPlanMode"]),
        ("PostToolUse", &["Bash"]),
        ("SessionStart", &["startup"]),
    ];

    let hooks_obj = obj
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .unwrap();

    for (event, matchers) in agentic_hooks {
        let arr = hooks_obj
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        for matcher in *matchers {
            let dominated = arr.iter().any(|entry| {
                entry.get("matcher").and_then(Value::as_str) == Some(matcher)
                    && entry
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|h| {
                            h.iter().any(|hook| {
                                hook.get("command").and_then(Value::as_str) == Some("agentic")
                            })
                        })
            });
            if !dominated {
                arr.push(serde_json::json!({
                    "matcher": matcher,
                    "hooks": [{ "type": "command", "command": "agentic" }]
                }));
            }
        }
    }

    // Determine whether to install permissions
    let install_perms = if permissions {
        true
    } else if no_permissions {
        false
    } else {
        // Interactive prompt
        eprint!("Install recommended permissions (git ops + read-only shell)? [y/N] ");
        let mut answer = String::new();
        io::BufRead::read_line(&mut io::stdin().lock(), &mut answer).unwrap_or(0);
        matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
    };

    if install_perms {
        let perms_obj = obj
            .entry("permissions")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .unwrap();

        let allow_arr = perms_obj
            .entry("allow")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        let mut added = 0usize;
        for perm in AGENTIC_PERMISSIONS {
            if !allow_arr.iter().any(|v| v.as_str() == Some(perm)) {
                allow_arr.push(Value::String(perm.to_string()));
                added += 1;
            }
        }

        let deny_arr = perms_obj
            .entry("deny")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .unwrap();

        for perm in AGENTIC_DENY {
            if !deny_arr.iter().any(|v| v.as_str() == Some(perm)) {
                deny_arr.push(Value::String(perm.to_string()));
                added += 1;
            }
        }

        println!("ok    {added} permissions added to settings.json");
    }

    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    println!("ok    {}", settings_path.display());

    println!("\nInstall complete. Restart Claude Code for changes to take effect.");
}

fn uninstall() {
    let home = home_dir();
    let claude_dir = home.join(".claude");

    for (name, _) in AGENTS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }

    for name in LEGACY_REFS {
        let path = claude_dir.join("agents").join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }
    for name in &["sign-off-protocol.md", "worker-protocol.md", "orchestration-rules.md"] {
        let path = claude_dir.join(name);
        if path.exists() {
            let _ = fs::remove_file(&path);
            println!("rm    {}", path.display());
        }
    }

    // Remove coding-standards.md
    let coding_standards = claude_dir.join("coding-standards.md");
    if coding_standards.exists() {
        let _ = fs::remove_file(&coding_standards);
        println!("rm    {}", coding_standards.display());
    }

    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists()
        && let Ok(content) = fs::read_to_string(&settings_path)
        && let Ok(mut settings) = serde_json::from_str::<Value>(&content)
    {
        if let Some(obj) = settings.as_object_mut() {
            // Remove only agentic hook entries, preserve user hooks
            if let Some(hooks_obj) = obj.get_mut("hooks").and_then(Value::as_object_mut) {
                for arr in hooks_obj.values_mut() {
                    if let Some(entries) = arr.as_array_mut() {
                        entries.retain(|entry| {
                            !entry
                                .get("hooks")
                                .and_then(Value::as_array)
                                .is_some_and(|h| {
                                    h.iter().any(|hook| {
                                        hook.get("command").and_then(Value::as_str)
                                            == Some("agentic")
                                    })
                                })
                        });
                    }
                }
                // Remove empty event arrays
                hooks_obj.retain(|_, v| {
                    v.as_array().is_none_or(|a| !a.is_empty())
                });
                // Remove hooks object entirely if empty
                if hooks_obj.is_empty() {
                    obj.remove("hooks");
                }
            }
            if let Some(env_obj) = obj.get_mut("env").and_then(Value::as_object_mut) {
                env_obj.remove("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS");
            }
            // Remove agentic permissions
            if let Some(perms_obj) = obj.get_mut("permissions").and_then(Value::as_object_mut) {
                if let Some(allow_arr) = perms_obj.get_mut("allow").and_then(Value::as_array_mut) {
                    allow_arr.retain(|v| {
                        !AGENTIC_PERMISSIONS.iter().any(|p| v.as_str() == Some(p))
                    });
                    if allow_arr.is_empty() {
                        perms_obj.remove("allow");
                    }
                }
                if let Some(deny_arr) = perms_obj.get_mut("deny").and_then(Value::as_array_mut) {
                    deny_arr.retain(|v| {
                        !AGENTIC_DENY.iter().any(|p| v.as_str() == Some(p))
                    });
                    if deny_arr.is_empty() {
                        perms_obj.remove("deny");
                    }
                }
                if perms_obj.is_empty() {
                    obj.remove("permissions");
                }
            }
        }
        let _ = fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        );
        println!("ok    {} (hooks removed)", settings_path.display());
    }

    // Remove both current and old binary names
    for name in &["agentic", "agentic-hooks"] {
        let bin = home.join(".local/bin").join(name);
        if bin.exists() {
            let _ = fs::remove_file(&bin);
            println!("rm    {}", bin.display());
        }
    }

    println!("\nUninstall complete.");
}

