use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

pub struct Found {
    pub agent: &'static str,
    pub kind: &'static str,
    /// Relative to the repo root.
    pub path: PathBuf,
}

/// Returns (agent, kind) for a repo-relative path. Add a new agent by adding match arms.
fn classify(rel: &Path) -> Option<(&'static str, &'static str)> {
    let name = rel.file_name()?.to_str()?;
    let comps: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    // Components after the last `.claude` dir, so nested `.claude` dirs in monorepos match too.
    let in_claude: &[&str] = match comps.iter().rposition(|c| *c == ".claude") {
        Some(i) => &comps[i + 1..],
        None => &[],
    };
    let md = name.ends_with(".md");
    let kind = match (name, in_claude) {
        ("AGENTS.md", _) => return Some(("agents.md", "Instructions")),
        ("CLAUDE.md" | "CLAUDE.local.md", _) => "Memory",
        (".mcp.json", []) => "MCP",
        (_, ["settings.json" | "settings.local.json"]) => "Settings",
        (_, ["rules", _, ..]) if md => "Rules",
        (_, ["skills", _, _, ..]) => "Skills",
        (_, ["commands", _, ..]) if md => "Commands",
        (_, ["agents", _, ..]) if md => "Subagents",
        (_, ["output-styles", _]) if md => "Output styles",
        _ => return None,
    };
    Some(("claude", kind))
}

/// The directory a file configures: the parent of its `.claude` dir, or its own parent.
fn project_dir(rel: &Path) -> PathBuf {
    let comps: Vec<_> = rel.components().collect();
    match comps.iter().rposition(|c| c.as_os_str() == ".claude") {
        Some(i) => comps[..i].iter().collect(),
        None => rel.parent().unwrap_or(Path::new("")).to_path_buf(),
    }
}

pub fn discover(root: &Path) -> Vec<Found> {
    let mut paths = BTreeSet::new();
    let walk = WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walk.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(root)
            && classify(rel).is_some()
        {
            paths.insert(rel.to_path_buf());
        }
    }

    // Local files are usually gitignored, so the walk skips them.
    // shortcut: only looks next to already-found files and the root, walk without gitignore if that misses some
    let dirs: BTreeSet<PathBuf> = paths
        .iter()
        .map(|p| project_dir(p))
        .chain([PathBuf::new()])
        .collect();
    for dir in dirs {
        for local in ["CLAUDE.local.md", ".claude/settings.local.json"] {
            let p = dir.join(local);
            if root.join(&p).is_file() {
                paths.insert(p);
            }
        }
    }

    let mut found: Vec<Found> = paths
        .into_iter()
        .filter_map(|path| classify(&path).map(|(agent, kind)| Found { agent, kind, path }))
        .collect();
    found.sort_by(|a, b| (a.agent, a.kind, &a.path).cmp(&(b.agent, b.kind, &b.path)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn classify_paths() {
        let cases = [
            ("CLAUDE.md", Some("Memory")),
            ("sub/dir/CLAUDE.md", Some("Memory")),
            ("CLAUDE.local.md", Some("Memory")),
            (".claude/CLAUDE.md", Some("Memory")),
            ("AGENTS.md", Some("Instructions")),
            (".mcp.json", Some("MCP")),
            (".claude/settings.json", Some("Settings")),
            ("pkg/.claude/settings.local.json", Some("Settings")),
            (".claude/rules/style.md", Some("Rules")),
            (".claude/rules/py/typing.md", Some("Rules")),
            (".claude/skills/deploy/SKILL.md", Some("Skills")),
            (".claude/skills/deploy/scripts/run.sh", Some("Skills")),
            (".claude/commands/review.md", Some("Commands")),
            (".claude/commands/git/commit.md", Some("Commands")),
            (".claude/agents/tester.md", Some("Subagents")),
            (".claude/output-styles/terse.md", Some("Output styles")),
            ("docs/claude.md", None),
            ("README.md", None),
            (".claude/skills.md", None),
            (".claude/skills/README.md", None),
            (".claude/rules/notes.txt", None),
            (".claude/foo/settings.json", None),
            (".claude/.mcp.json", None),
        ];
        for (path, want) in cases {
            assert_eq!(classify(Path::new(path)).map(|(_, k)| k), want, "{path}");
        }
    }

    #[test]
    fn discover_fixture() {
        let root = std::env::temp_dir().join(format!("brainscan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let files = [
            ".git/HEAD",
            ".gitignore",
            "CLAUDE.md",
            "CLAUDE.local.md",
            "AGENTS.md",
            ".claude/settings.local.json",
            ".claude/skills/deploy/SKILL.md",
            "node_modules/x/CLAUDE.md",
            "pkg/CLAUDE.md",
            "src/main.rs",
        ];
        for f in files {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
        }
        fs::write(
            root.join(".gitignore"),
            "node_modules/\nCLAUDE.local.md\n.claude/settings.local.json\n",
        )
        .unwrap();

        let got: Vec<String> = discover(&root)
            .iter()
            .map(|f| format!("{}|{}|{}", f.agent, f.kind, f.path.display()))
            .collect();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            got,
            [
                "agents.md|Instructions|AGENTS.md",
                "claude|Memory|CLAUDE.local.md",
                "claude|Memory|CLAUDE.md",
                "claude|Memory|pkg/CLAUDE.md",
                "claude|Settings|.claude/settings.local.json",
                "claude|Skills|.claude/skills/deploy/SKILL.md",
            ]
        );
    }
}
