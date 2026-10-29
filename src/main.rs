mod discover;

use std::io;
use std::path::PathBuf;
use std::process::Command;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{execute, terminal};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

use discover::{Found, discover};

struct App {
    root: PathBuf,
    files: Vec<Found>,
    /// One per list row: None for a group header, Some(index into files) for a file.
    rows: Vec<Option<usize>>,
    list: ListState,
    content: Text<'static>,
    syntaxes: SyntaxSet,
    theme: Theme,
    scroll: u16,
    /// Preview height and wrapped line count from the last draw, for paging and clamping.
    page: u16,
    lines: u16,
    /// List height from the last draw, for paging the selection.
    list_page: u16,
}

impl App {
    fn new(root: PathBuf) -> Self {
        let files = discover(&root);
        let mut rows = Vec::new();
        let mut group = None;
        for (i, f) in files.iter().enumerate() {
            if group != Some((f.agent, f.kind)) {
                group = Some((f.agent, f.kind));
                rows.push(None);
            }
            rows.push(Some(i));
        }
        let mut app = App {
            root,
            files,
            rows,
            list: ListState::default(),
            content: Text::default(),
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: ThemeSet::load_defaults().themes.remove("base16-ocean.dark").unwrap(),
            scroll: 0,
            page: 0,
            lines: 0,
            list_page: 0,
        };
        app.select(app.rows.iter().position(Option::is_some));
        app
    }

    fn selected(&self) -> Option<&Found> {
        self.rows[self.list.selected()?].map(|i| &self.files[i])
    }

    fn select(&mut self, row: Option<usize>) {
        let Some(row) = row else { return };
        self.list.select(Some(row));
        self.reload();
    }

    fn reload(&mut self) {
        let Some(path) = self.selected().map(|f| self.root.join(&f.path)) else { return };
        self.content = match std::fs::read_to_string(&path) {
            Ok(s) => highlight(&s.replace('\t', "    "), &path, &self.syntaxes, &self.theme),
            Err(e) => Text::raw(format!("<cannot read {}: {e}>", path.display())),
        };
        self.scroll = 0;
    }

    /// Moves to the next file row in the given direction, skipping headers.
    fn step(&mut self, down: bool) {
        let cur = self.list.selected().unwrap_or(0);
        let next = if down {
            (cur + 1..self.rows.len()).find(|&r| self.rows[r].is_some())
        } else {
            (0..cur).rev().find(|&r| self.rows[r].is_some())
        };
        self.select(next);
    }

    /// Moves the selection by one list page.
    fn page_list(&mut self, down: bool) {
        let Some(cur) = self.list.selected() else { return };
        let n = self.list_page.max(1) as usize;
        let target = if down { (cur + n).min(self.rows.len() - 1) } else { cur.saturating_sub(n) };
        // A header is always followed by a file row, and the last row is a file.
        self.select(Some(if self.rows[target].is_some() { target } else { target + 1 }));
    }

    fn scroll_by(&mut self, delta: i32) {
        let max = self.lines.saturating_sub(self.page) as i32;
        self.scroll = (self.scroll as i32 + delta).clamp(0, max.max(0)) as u16;
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [main, footer] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        frame.render_widget(
            Line::from(" j/k PgDn/PgUp: select  J/K ^d/^u: scroll  g/G: first/last  e: edit  q: quit").dim(),
            footer,
        );

        if self.files.is_empty() {
            let msg = format!("No agent files found in {}", self.root.display());
            frame.render_widget(Paragraph::new(msg).block(Block::bordered()), main);
            return;
        }

        let [left, right] = Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)]).areas(main);
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(r, row)| match row {
                Some(i) => ListItem::new(format!("  {}", self.files[*i].path.display())),
                None => {
                    let f = &self.files[self.rows[r + 1].unwrap()];
                    ListItem::new(format!("{} / {}", f.agent, f.kind)).bold().cyan()
                }
            })
            .collect();
        let list = List::new(items)
            .block(Block::bordered().title(format!(" {} ", self.root.display())))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, left, &mut self.list);
        self.list_page = left.height.saturating_sub(2);

        let title = self.selected().map(|f| format!(" {} ", f.path.display())).unwrap_or_default();
        let preview = Paragraph::new(self.content.clone()).wrap(Wrap { trim: false });
        let inner_width = right.width.saturating_sub(2);
        self.lines = preview.line_count(inner_width).min(u16::MAX as usize) as u16;
        self.page = right.height.saturating_sub(2);
        // Clamp here too: the content or terminal size may have changed since the last scroll.
        self.scroll = self.scroll.min(self.lines.saturating_sub(self.page));
        frame.render_widget(preview.block(Block::bordered().title(title)).scroll((self.scroll, 0)), right);
    }

    fn edit(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let Some(path) = self.selected().map(|f| self.root.join(&f.path)) else { return Ok(()) };
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".into());
        ratatui::restore();
        // `sh -c` so EDITOR values with arguments (e.g. "code --wait") work.
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$1\""))
            .arg("sh")
            .arg(&path)
            .status();
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen)?;
        terminal.clear()?;
        status?;
        self.reload();
        Ok(())
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            let Event::Key(key) = event::read()? else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Char('d') if ctrl => self.scroll_by(self.page as i32),
                KeyCode::Char('u') if ctrl => self.scroll_by(-(self.page as i32)),
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => self.step(true),
                KeyCode::Char('k') | KeyCode::Up => self.step(false),
                KeyCode::Char('g') | KeyCode::Home => self.select(self.rows.iter().position(Option::is_some)),
                KeyCode::Char('G') | KeyCode::End => self.select(self.rows.iter().rposition(Option::is_some)),
                KeyCode::Char('J') => self.scroll_by(1),
                KeyCode::Char('K') => self.scroll_by(-1),
                KeyCode::PageDown => self.page_list(true),
                KeyCode::PageUp => self.page_list(false),
                KeyCode::Char('e') => self.edit(terminal)?,
                _ => {}
            }
        }
    }
}

/// Colors `src` by the syntax matching `path`'s extension, keeping the terminal's own background.
fn highlight(src: &str, path: &std::path::Path, syntaxes: &SyntaxSet, theme: &Theme) -> Text<'static> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let syntax = syntaxes.find_syntax_by_extension(ext).unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, theme);
    syntect::util::LinesWithEndings::from(src)
        .map(|line| match h.highlight_line(line, syntaxes) {
            Ok(regions) => Line::from_iter(regions.into_iter().map(|(style, text)| {
                let c = style.foreground;
                Span::styled(text.trim_end_matches(['\r', '\n']).to_string(), Style::new().fg(Color::Rgb(c.r, c.g, c.b)))
            })),
            Err(_) => Line::raw(line.trim_end_matches(['\r', '\n']).to_string()),
        })
        .collect()
}

fn main() -> io::Result<()> {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let root = std::fs::canonicalize(&root).map_err(|e| io::Error::new(e.kind(), format!("{root}: {e}")))?;
    let app = App::new(root);
    ratatui::run(|terminal| app.run(terminal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_keeps_lines_and_colors_by_extension() {
        let (syntaxes, theme) = (SyntaxSet::load_defaults_newlines(), ThemeSet::load_defaults().themes.remove("base16-ocean.dark").unwrap());
        let md = highlight("# Title\r\nbody\n", std::path::Path::new("x.md"), &syntaxes, &theme);
        assert_eq!(md.lines.len(), 2);
        assert_eq!(md.lines[0].to_string(), "# Title");
        assert!(md.lines[0].spans.iter().all(|s| matches!(s.style.fg, Some(Color::Rgb(..)))));
        let unknown = highlight("a\nb", std::path::Path::new("x.unknownext"), &syntaxes, &theme);
        assert_eq!(unknown.lines.len(), 2);
    }
}
