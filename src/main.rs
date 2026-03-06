use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::Value;
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

// ── Embedded content ─────────────────────────────────────────────────

const AGENTS: &[(&str, &str)] = &[
    ("dev.md", include_str!("../agents/dev.md")),
    ("reviewer.md", include_str!("../agents/reviewer.md")),
    ("config-gen.md", include_str!("../agents/config-gen.md")),
];


const GLOBAL_CLAUDE_MD: &str = include_str!("../architect.md");
const CODING_STANDARDS_MD: &str = include_str!("../coding-standards.md");
const PLANNING_PROTOCOL: &str = include_str!("../planning-protocol.md");

const TIER_GIT_ALLOW: &[&str] = &[
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
];
const TIER_GIT_DENY: &[&str] = &[];

const TIER_READONLY_ALLOW: &[&str] = &[
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
    "Glob",
    "Grep",
];
const TIER_READONLY_DENY: &[&str] = &["Bash(sed -i *)"];

const TIER_WRITE_ALLOW: &[&str] = &["Edit", "Write", "NotebookEdit"];
const TIER_WRITE_DENY: &[&str] = &[
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
    Install,
    /// Remove agentic workflow (agents, hooks, binary)
    Uninstall,
    /// Manage project-local permissions
    Permissions {
        #[command(subcommand)]
        command: PermissionsCommand,
    },
}

#[derive(Subcommand)]
enum PermissionsCommand {
    /// Add permission tiers to project-local settings
    Add {
        /// Add git workflow permissions
        #[arg(long)]
        git: bool,
        /// Add read-only shell + file tool permissions
        #[arg(long)]
        readonly: bool,
        /// Add file editing permissions (Edit, Write, NotebookEdit)
        #[arg(long)]
        write: bool,
    },
    /// Remove all agentic-managed permissions from project-local settings
    Remove,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Install) => install(),
        Some(Command::Uninstall) => uninstall(),
        Some(Command::Permissions { command }) => permissions(command),
        None => hook_dispatch(),
    }
}

// ── Hook dispatch ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    tool_name: Option<String>,
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

    match (hook.hook_event_name.as_str(), tool) {
        ("PreToolUse", "EnterPlanMode") => {
            debug!("{} {} → planning_protocol", hook.hook_event_name, tool);
            planning_protocol(&hook);
        }
        ("SessionStart", _) => {
            debug!("{} {} → session_start", hook.hook_event_name, tool);
            session_start(&hook);
        }
        _ => {
            debug!("{} {} → no-op", hook.hook_event_name, tool);
        }
    }
}

/// Hook 1: Adaptive Planning Protocol
/// Injects the full planning protocol + .claude/project-config.md as additionalContext when plan mode is entered.
fn planning_protocol(hook: &HookInput) {
    let cwd = Path::new(&hook.cwd);
    let config_path = cwd.join(".claude/project-config.md");

    let additional_context = if let Ok(config) = fs::read_to_string(&config_path) {
        debug!("injecting planning protocol + project config");
        format!("{PLANNING_PROTOCOL}\n\n{config}")
    } else {
        debug!("injecting planning protocol (no .claude/project-config.md)");
        format!(
            "{PLANNING_PROTOCOL}\n\n\
            .claude/project-config.md does not exist in this project. \
            If this task requires build/test/verification commands, \
            spawn the config-gen agent to generate .claude/project-config.md before converging to spec."
        )
    };

    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": additional_context
        }
    });
    serde_json::to_writer(io::stdout(), &output).ok();
}

/// Walks up from `start`, returning the directory that contains `.git` (file or directory),
/// or `None` if no git root is found.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Hook 2: Session Start — Bootstrap
/// Checks for git repo (prompts init if missing) and nested project detection (asks user to proceed or bail).
fn session_start(hook: &HookInput) {
    let cwd = Path::new(&hook.cwd);

    // Check for git repo — worktree isolation requires one
    match find_git_root(cwd) {
        None => {
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
        }
        Some(root) if root != cwd => {
            let relative = cwd.strip_prefix(&root).unwrap_or(cwd);
            debug!(
                "nested project: git root at {}, subdir {}",
                root.display(),
                relative.display()
            );

            let additional_context = format!(
                "NESTED PROJECT DETECTED: The git root is at {root}, but you're working in {relative} within the repo. \
                Worktrees will clone the entire repo from the git root. \
                Use AskUserQuestion to ask the user: \
                option 1: 'Proceed' — continue working in this subdirectory (worktrees will include the full repo, specs will include `cd {relative}`). \
                option 2: 'Bail' — stop and let the user handle it. \
                The user can also free-type a response.",
                root = root.display(),
                relative = relative.display(),
            );

            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": additional_context
                }
            });
            serde_json::to_writer(io::stdout(), &output).ok();
        }
        Some(_) => {
            // cwd is the git root — no-op
            debug!("git root matches cwd — no action needed");
        }
    }
}

// ── Install & Uninstall ──────────────────────────────────────────────

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

fn write_file_if_changed(content: &str, dst: &Path) {
    if fs::read_to_string(dst).ok().as_deref() == Some(content) {
        println!("ok    {} (unchanged)", dst.display());
        return;
    }
    write_file(content, dst);
}

fn is_agentic_hook(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|h| {
            h.iter()
                .any(|hook| hook.get("command").and_then(Value::as_str) == Some("agentic"))
        })
}

fn load_settings(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn save_settings(path: &Path, settings: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, serde_json::to_string_pretty(settings).unwrap()).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {e}", path.display());
        process::exit(1);
    });
}

fn add_permissions(settings: &mut Value, allow: &[&str], deny: &[&str]) -> (usize, usize) {
    let obj = settings.as_object_mut().unwrap();
    let perms_val = obj
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    if !perms_val.is_object() {
        *perms_val = serde_json::json!({});
    }
    let perms_obj = perms_val.as_object_mut().unwrap();

    let allow_val = perms_obj
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]));
    if !allow_val.is_array() {
        *allow_val = serde_json::json!([]);
    }
    let allow_arr = allow_val.as_array_mut().unwrap();
    let mut allow_added = 0usize;
    for perm in allow {
        if !allow_arr.iter().any(|v| v.as_str() == Some(perm)) {
            allow_arr.push(Value::String(perm.to_string()));
            allow_added += 1;
        }
    }

    let deny_val = perms_obj
        .entry("deny")
        .or_insert_with(|| serde_json::json!([]));
    if !deny_val.is_array() {
        *deny_val = serde_json::json!([]);
    }
    let deny_arr = deny_val.as_array_mut().unwrap();
    let mut deny_added = 0usize;
    for perm in deny {
        if !deny_arr.iter().any(|v| v.as_str() == Some(perm)) {
            deny_arr.push(Value::String(perm.to_string()));
            deny_added += 1;
        }
    }

    (allow_added, deny_added)
}

fn remove_permissions(settings: &mut Value, allow: &[&str], deny: &[&str]) {
    if let Some(obj) = settings.as_object_mut()
        && let Some(perms_obj) = obj.get_mut("permissions").and_then(Value::as_object_mut)
    {
        if let Some(allow_arr) = perms_obj.get_mut("allow").and_then(Value::as_array_mut) {
            allow_arr.retain(|v| !allow.iter().any(|p| v.as_str() == Some(p)));
            if allow_arr.is_empty() {
                perms_obj.remove("allow");
            }
        }
        if let Some(deny_arr) = perms_obj.get_mut("deny").and_then(Value::as_array_mut) {
            deny_arr.retain(|v| !deny.iter().any(|p| v.as_str() == Some(p)));
            if deny_arr.is_empty() {
                perms_obj.remove("deny");
            }
        }
        if perms_obj.is_empty() {
            obj.remove("permissions");
        }
    }
}

fn permissions(cmd: PermissionsCommand) {
    let cwd = env::current_dir().expect("cannot determine current directory");
    let settings_path = cwd.join(".claude/settings.local.json");

    match cmd {
        PermissionsCommand::Add {
            git,
            readonly,
            write,
        } => {
            let interactive = !git && !readonly && !write;

            let add_git = git || (interactive && prompt_yn("Add git workflow permissions?"));
            let add_readonly = readonly
                || (interactive && prompt_yn("Add read-only shell + file tool permissions?"));
            let add_write = write
                || (interactive
                    && prompt_yn("Add file editing permissions (Edit, Write, NotebookEdit)?"));

            if !add_git && !add_readonly && !add_write {
                println!("No tiers selected.");
                return;
            }

            let mut settings = load_settings(&settings_path);
            if !settings.is_object() {
                settings = serde_json::json!({});
            }

            let mut total_allow = 0usize;
            let mut total_deny = 0usize;

            if add_git {
                let (a, d) = add_permissions(&mut settings, TIER_GIT_ALLOW, TIER_GIT_DENY);
                total_allow += a;
                total_deny += d;
                println!("ok    git tier: {a} allow + {d} deny added");
            }
            if add_readonly {
                let (a, d) =
                    add_permissions(&mut settings, TIER_READONLY_ALLOW, TIER_READONLY_DENY);
                total_allow += a;
                total_deny += d;
                println!("ok    readonly tier: {a} allow + {d} deny added");
            }
            if add_write {
                let (a, d) = add_permissions(&mut settings, TIER_WRITE_ALLOW, TIER_WRITE_DENY);
                total_allow += a;
                total_deny += d;
                println!("ok    write tier: {a} allow + {d} deny added");
            }

            save_settings(&settings_path, &settings);
            println!(
                "ok    {} ({total_allow} allow + {total_deny} deny total)",
                settings_path.display()
            );
        }
        PermissionsCommand::Remove => {
            if !settings_path.exists() {
                println!("No project-local settings found.");
                return;
            }

            let mut settings = load_settings(&settings_path);

            let all_allow: Vec<&str> = TIER_GIT_ALLOW
                .iter()
                .chain(TIER_READONLY_ALLOW.iter())
                .chain(TIER_WRITE_ALLOW.iter())
                .copied()
                .collect();
            let all_deny: Vec<&str> = TIER_GIT_DENY
                .iter()
                .chain(TIER_READONLY_DENY.iter())
                .chain(TIER_WRITE_DENY.iter())
                .copied()
                .collect();

            remove_permissions(&mut settings, &all_allow, &all_deny);

            if settings.as_object().is_some_and(|o| o.is_empty()) {
                if let Err(e) = fs::remove_file(&settings_path) {
                    eprintln!("failed to remove {}: {e}", settings_path.display());
                }
                println!("rm    {} (empty after cleanup)", settings_path.display());
            } else {
                save_settings(&settings_path, &settings);
                println!(
                    "ok    {} (agentic permissions removed)",
                    settings_path.display()
                );
            }
        }
    }
}

fn prompt_yn(question: &str) -> bool {
    eprint!("{question} [y/N] ");
    let mut answer = String::new();
    io::BufRead::read_line(&mut io::stdin().lock(), &mut answer).unwrap_or(0);
    matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

fn install() {
    let home = home_dir();
    let claude_dir = home.join(".claude");

    // Agents
    let agents_dir = claude_dir.join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    for (name, content) in AGENTS {
        write_file_if_changed(content, &agents_dir.join(name));
    }

    // Global CLAUDE.md + coding-standards.md
    write_file_if_changed(GLOBAL_CLAUDE_MD, &claude_dir.join("CLAUDE.md"));
    write_file_if_changed(CODING_STANDARDS_MD, &claude_dir.join("coding-standards.md"));

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

    // Configure settings.json
    let settings_path = claude_dir.join("settings.json");
    let mut settings: Value = fs::read_to_string(&settings_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if !settings.is_object() {
        eprintln!("Warning: settings.json is not a JSON object, resetting to {{}}");
        settings = serde_json::json!({});
    }

    let obj = settings.as_object_mut().unwrap();

    // Merge agentic hooks into existing config (preserve user hooks)
    let agentic_hooks: &[(&str, &[&str])] = &[
        ("PreToolUse", &["EnterPlanMode"]),
        ("SessionStart", &["startup"]),
    ];

    let hooks_val = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks_val.is_object() {
        *hooks_val = serde_json::json!({});
    }
    let hooks_obj = hooks_val.as_object_mut().unwrap();

    for (event, matchers) in agentic_hooks {
        let arr_val = hooks_obj
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]));
        if !arr_val.is_array() {
            *arr_val = serde_json::json!([]);
        }
        let arr = arr_val.as_array_mut().unwrap();

        for matcher in *matchers {
            let already_installed = arr.iter().any(|entry| {
                entry.get("matcher").and_then(Value::as_str) == Some(matcher)
                    && is_agentic_hook(entry)
            });
            if !already_installed {
                arr.push(serde_json::json!({
                    "matcher": matcher,
                    "hooks": [{ "type": "command", "command": "agentic" }]
                }));
            }
        }
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
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("failed to remove {}: {e}", path.display());
            }
            println!("rm    {}", path.display());
        }
    }

    let claude_md = claude_dir.join("CLAUDE.md");
    if claude_md.exists() {
        if fs::read_to_string(&claude_md).ok().as_deref() == Some(GLOBAL_CLAUDE_MD) {
            if let Err(e) = fs::remove_file(&claude_md) {
                eprintln!("failed to remove {}: {e}", claude_md.display());
            }
            println!("rm    {}", claude_md.display());
        } else {
            println!("skip  {} (user-modified)", claude_md.display());
        }
    }

    let coding_standards = claude_dir.join("coding-standards.md");
    if coding_standards.exists() {
        if fs::read_to_string(&coding_standards).ok().as_deref() == Some(CODING_STANDARDS_MD) {
            if let Err(e) = fs::remove_file(&coding_standards) {
                eprintln!("failed to remove {}: {e}", coding_standards.display());
            }
            println!("rm    {}", coding_standards.display());
        } else {
            println!("skip  {} (user-modified)", coding_standards.display());
        }
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
                        entries.retain(|entry| !is_agentic_hook(entry));
                    }
                }
                // Remove empty event arrays
                hooks_obj.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
                // Remove hooks object entirely if empty
                if hooks_obj.is_empty() {
                    obj.remove("hooks");
                }
            }
        }
        fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap_or_else(|e| {
            eprintln!("Failed to write {}: {e}", settings_path.display());
        });
        println!("ok    {} (hooks removed)", settings_path.display());
    }

    let bin = home.join(".local/bin").join("agentic");
    if bin.exists() {
        if let Err(e) = fs::remove_file(&bin) {
            eprintln!("failed to remove {}: {e}", bin.display());
        }
        println!("rm    {}", bin.display());
    }

    println!("\nUninstall complete.");
}
