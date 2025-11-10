use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::{
    error::Error,
    io::{self},
    path::PathBuf,
    time::Instant,
};
use tui_textarea::TextArea;

mod clipboard;
mod translation_data;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(short, long, value_parser)]
    pub source_file: PathBuf,
    #[clap(short, long, value_parser)]
    pub out: Option<PathBuf>,
    #[clap(long, action = clap::ArgAction::SetTrue, default_value_t = true)]
    pub color: bool,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub key_segment: String,
    pub full_path: String,
    pub translation: Option<TranslationItem>,
    pub children: Vec<TreeNode>,
    pub expanded: bool,
    pub fully_translated: bool,
}

impl TreeNode {
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Editing,
}

use crate::clipboard::{Clipboard, WaylandClipboard};
use crate::translation_data::{TranslationItem, TranslationStore};

pub struct App<'a> {
    tree: Vec<TreeNode>,
    visible_nodes: Vec<(String, usize)>, // (full_path, depth)
    selected_index: usize,
    textarea: TextArea<'a>,
    translation_store: TranslationStore,
    mode: AppMode,
    output_path: PathBuf,
    status_message: Option<(String, Instant)>,
    clipboard: Box<dyn Clipboard>,
    color: bool,
    scrolloff: usize,
}

impl<'a> App<'a> {
    fn new(
        items: Vec<TranslationItem>,
        output_path: PathBuf,
        color: bool,
    ) -> Result<App<'a>, Box<dyn Error>> {
        let translation_store = TranslationStore::new(items);
        let mut tree = App::build_tree(translation_store.all_items.values().cloned().collect());
        App::update_node_translation_status(&mut tree);

        let clipboard: Box<dyn Clipboard> = Box::new(WaylandClipboard);

        let mut app = App {
            tree,
            visible_nodes: Vec::new(),
            selected_index: 0,
            textarea: TextArea::default(),
            translation_store,
            mode: AppMode::Normal,
            output_path,
            status_message: None,
            clipboard,
            color,
            scrolloff: 7,
        };
        app.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit Terjemahan"),
        );
        app.update_visible_nodes();
        Ok(app)
    }

    fn save_translations(&self) -> Result<(), Box<dyn Error>> {
        self.translation_store.save_translations(&self.output_path)
    }

    fn build_tree(items: Vec<TranslationItem>) -> Vec<TreeNode> {
        let mut root_nodes: Vec<TreeNode> = Vec::new();

        let mut sorted_items = items;
        sorted_items.sort_by(|a, b| a.key.cmp(&b.key));

        for item in sorted_items {
            let segments: Vec<&str> = item.key.split('.').collect();
            let mut current_level_nodes = &mut root_nodes;
            let mut path_so_far = String::new();

            for (i, segment) in segments.iter().enumerate() {
                path_so_far = if path_so_far.is_empty() {
                    segment.to_string()
                } else {
                    format!("{}.{}", path_so_far, segment)
                };

                // Helper closure to find or create a child node
                let find_or_create_node =
                    |nodes: &mut Vec<TreeNode>, segment: &str, full_path: String| {
                        let position = nodes.iter().position(|n| n.key_segment == segment);
                        match position {
                            Some(pos) => pos,
                            None => {
                                let new_node = TreeNode {
                                    key_segment: segment.to_string(),
                                    full_path,
                                    translation: None,
                                    children: Vec::new(),
                                    expanded: false,
                                    fully_translated: false,
                                };
                                nodes.push(new_node);
                                nodes.len() - 1
                            }
                        }
                    };

                let node_index =
                    find_or_create_node(current_level_nodes, segment, path_so_far.clone());

                if i == segments.len() - 1 {
                    current_level_nodes[node_index].translation = Some(item.clone());
                }

                current_level_nodes = &mut current_level_nodes[node_index].children;
            }
        }
        root_nodes
    }

    fn update_node_translation_status(nodes: &mut [TreeNode]) -> bool {
        let mut all_children_translated = true;
        for node in nodes.iter_mut() {
            if node.is_leaf() {
                node.fully_translated = node
                    .translation
                    .as_ref()
                    .map_or(false, |t| t.is_translated());
            } else {
                let children_translated = Self::update_node_translation_status(&mut node.children);
                node.fully_translated = children_translated;
            }
            if !node.fully_translated {
                all_children_translated = false;
            }
        }
        all_children_translated
    }

    fn update_visible_nodes(&mut self) {
        self.visible_nodes.clear();
        Self::generate_visible_list_recursive(&self.tree, 0, &mut self.visible_nodes);
        if self.selected_index >= self.visible_nodes.len() && !self.visible_nodes.is_empty() {
            self.selected_index = self.visible_nodes.len() - 1;
        }
    }

    fn generate_visible_list_recursive(
        nodes: &[TreeNode],
        depth: usize,
        visible_list: &mut Vec<(String, usize)>,
    ) {
        for node in nodes {
            visible_list.push((node.full_path.clone(), depth));
            if node.expanded {
                Self::generate_visible_list_recursive(&node.children, depth + 1, visible_list);
            }
        }
    }

    fn get_node(&self, path: &str) -> Option<&TreeNode> {
        let mut segments = path.split('.');
        let root_segment = segments.next()?;
        let mut current_node = self.tree.iter().find(|n| n.key_segment == root_segment)?;
        for segment in segments {
            current_node = current_node
                .children
                .iter()
                .find(|n| n.key_segment == segment)?;
        }
        Some(current_node)
    }

    fn get_node_mut(&mut self, path: &str) -> Option<&mut TreeNode> {
        let mut segments = path.split('.');
        let root_segment = segments.next()?;
        let mut current_node = self
            .tree
            .iter_mut()
            .find(|n| n.key_segment == root_segment)?;
        for segment in segments {
            current_node = current_node
                .children
                .iter_mut()
                .find(|n| n.key_segment == segment)?;
        }
        Some(current_node)
    }

    fn next(&mut self) {
        if self.selected_index < self.visible_nodes.len() - 1 {
            self.selected_index += 1;
        }
    }

    fn previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    fn toggle_expand(&mut self) {
        if let Some((path, _)) = self.visible_nodes.get(self.selected_index).cloned() {
            if let Some(node) = self.get_node_mut(&path) {
                if !node.is_leaf() {
                    node.expanded = !node.expanded;
                }
            }
        }
        self.update_visible_nodes();
    }

    fn render_key_list(&self, f: &mut Frame, area: Rect) {
        let list_style = if self.color && matches!(self.mode, AppMode::Normal) {
            Style::default()
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
        };

        let items: Vec<ListItem> = self
            .visible_nodes
            .iter()
            .map(|(path, depth)| {
                let node = self.get_node(path).unwrap(); // Should exist
                let is_leaf = node.is_leaf();

                let status_span = if is_leaf {
                    if node
                        .translation
                        .as_ref()
                        .map_or(false, |t| t.is_translated())
                    {
                        if self.color {
                            Span::styled(
                                "[✓]",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[✓]")
                        }
                    } else {
                        if self.color {
                            Span::styled("[ ]", Style::default().fg(Color::LightRed))
                        } else {
                            Span::raw("[ ]")
                        }
                    }
                } else {
                    // It's a folder
                    if node.fully_translated {
                        if self.color {
                            Span::styled(
                                "[✓]",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw("[✓]")
                        }
                    } else if node.expanded {
                        if self.color {
                            Span::styled("[-] ", Style::default().fg(Color::Blue))
                        } else {
                            Span::raw("[-] ")
                        }
                    } else {
                        if self.color {
                            Span::styled("[+] ", Style::default().fg(Color::LightCyan))
                        } else {
                            Span::raw("[+] ")
                        }
                    }
                };

                let indentation = "  ".repeat(*depth);

                let line = Line::from(vec![
                    Span::raw(indentation),
                    status_span,
                    Span::raw(node.key_segment.clone()),
                ]);

                ListItem::new(line)
            })
            .collect();

        let items_list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Daftar Kunci"))
            .highlight_style(list_style)
            .highlight_symbol(">> ");

        let mut list_state = ratatui::widgets::ListState::default();
        list_state.select(Some(self.selected_index));

        let height = area.height as usize;
        let scrolloff = self.scrolloff;
        let num_items = self.visible_nodes.len();

        let mut offset = list_state.offset();

        // Adjust offset if selected item is too far up
        if self.selected_index < offset + scrolloff {
            offset = self.selected_index.saturating_sub(scrolloff);
        }
        // Adjust offset if selected item is too far down
        else if self.selected_index >= offset + height.saturating_sub(scrolloff) {
            offset = self.selected_index.saturating_sub(height.saturating_sub(scrolloff).saturating_sub(1));
        }

        // Ensure offset does not exceed the maximum possible offset
        let max_offset = num_items.saturating_sub(height);
        *list_state.offset_mut() = offset.min(max_offset);

        f.render_stateful_widget(items_list, area, &mut list_state);
    }

    fn render_source_text(&self, f: &mut Frame, area: Rect) {
        let (source_text, target_display_text) =
            if let Some((path, _)) = self.visible_nodes.get(self.selected_index) {
                if let Some(item) = self.translation_store.all_items.get(path) {
                    (item.source_text.clone(), item.get_display_text())
                } else {
                    ("Select a translatable key.".to_string(), String::new())
                }
            } else {
                (String::new(), String::new())
            };

        let mut text_lines = vec![Line::from(vec![
            Span::styled("Source: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(source_text),
        ])];

        if !target_display_text.is_empty() {
            text_lines.push(Line::from(vec![
                Span::styled("Target: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(target_display_text),
            ]));
        }

        let source_paragraph = Paragraph::new(text_lines)
            .block(Block::default().borders(Borders::ALL).title("Teks Sumber"));
        f.render_widget(source_paragraph, area);
    }

    fn render_editor(&self, f: &mut Frame, area: Rect) {
        f.render_widget(&self.textarea, area);
    }

    fn render_status_message(&self, f: &mut Frame, area: Rect) {
        if let Some((msg, _)) = &self.status_message {
            let footer = if self.color {
                Paragraph::new(msg.as_str()).style(Style::default().fg(Color::LightYellow))
            } else {
                Paragraph::new(msg.as_str()).style(Style::default())
            };
            f.render_widget(footer, area);
        }
    }
} // End of impl App

fn ui(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
        .split(f.area());

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(main_chunks[0]);

    app.render_key_list(f, top_chunks[0]);

    // Panel Kanan
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(top_chunks[1]);

    // Panel Kanan Atas: Teks Sumber
    app.render_source_text(f, right_chunks[0]);

    // Panel Kanan Bawah: Area Input
    app.render_editor(f, right_chunks[1]);

    // Footer untuk status message
    app.render_status_message(f, main_chunks[1]);
}

fn restore_terminal<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
) -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load translation items from files
    let items = match TranslationStore::load_from_files(&cli.source_file, cli.out.as_ref()) {
        Ok(items) => items,
        Err(e) => {
            restore_terminal(&mut terminal)?;
            eprintln!("Error loading translation files: {}", e);
            return Err(e);
        }
    };

    // Buat app dan jalankan
    let output_path = cli.out.clone().unwrap_or_else(|| {
        let source_path = cli.source_file.clone();
        let file_name = source_path.file_name().unwrap().to_str().unwrap();
        let new_file_name = format!("id_{}", file_name);
        source_path.with_file_name(new_file_name)
    });
    let mut app = match App::new(items, output_path, cli.color) {
        Ok(app) => app,
        Err(e) => {
            restore_terminal(&mut terminal)?;
            eprintln!("Error initializing app: {}", e);
            return Err(e);
        }
    };
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    restore_terminal(&mut terminal)?;

    if let Err(err) = res {
        println!("Error in TUI: {:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Some((_, instant)) = app.status_message.as_ref() {
            if instant.elapsed().as_secs() >= 2 {
                app.status_message = None;
            }
        }

        let event = event::read()?;

        match app.mode {
            AppMode::Normal => {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('s') => {
                            if app.save_translations().is_ok() {
                                app.status_message =
                                    Some(("File saved!".to_string(), Instant::now()));
                            } else {
                                app.status_message =
                                    Some(("Error saving file!".to_string(), Instant::now()));
                            }
                        }
                        KeyCode::Char('y') => {
                            if let Some((path, _)) =
                                app.visible_nodes.get(app.selected_index).cloned()
                            {
                                if let Some(item) = app.translation_store.all_items.get(&path) {
                                    let text_to_copy = item.source_text.clone();
                                    match app.clipboard.copy(&text_to_copy) {
                                        Ok(_) => {
                                            app.status_message = Some((
                                                "Copied to clipboard!".to_string(),
                                                Instant::now(),
                                            ));
                                        }
                                        Err(e) => {
                                            app.status_message = Some((
                                                format!("Failed to copy to clipboard: {}", e),
                                                Instant::now(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('p') => {
                            let mut pasted_text: Option<String> = None;
                            let status_msg = match app.clipboard.paste() {
                                Ok(text) => {
                                    pasted_text = Some(text);
                                    "Pasted from clipboard!".to_string()
                                }
                                Err(e) => {
                                    format!("Failed to paste from clipboard: {}", e)
                                }
                            };

                            if let Some(text) = pasted_text {
                                if let Some((path, _)) =
                                    app.visible_nodes.get(app.selected_index).cloned()
                                {
                                    if let Some(item) =
                                        app.translation_store.all_items.get_mut(&path)
                                    {
                                        item.target_text = Some(text.clone());
                                    }
                                    if let Some(node) = app.get_node_mut(&path) {
                                        if let Some(trans_item) = &mut node.translation {
                                            trans_item.target_text = Some(text);
                                        }
                                    }
                                    App::update_node_translation_status(&mut app.tree);
                                }
                            }
                            app.status_message = Some((status_msg, Instant::now()));
                        }
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::Char(' ') => app.toggle_expand(),
                        KeyCode::Right | KeyCode::Char('l') => app.toggle_expand(),
                        KeyCode::Left | KeyCode::Char('h') => app.toggle_expand(),
                        KeyCode::Enter => {
                            let selected_path = app.visible_nodes.get(app.selected_index).cloned();
                            if let Some((path, _)) = selected_path {
                                let is_leaf = app.get_node(&path).map_or(false, |n| n.is_leaf());

                                if is_leaf {
                                    app.mode = AppMode::Editing;
                                    let node = app.get_node(&path).unwrap(); // Re-borrow
                                    let source_text = node
                                        .translation
                                        .as_ref()
                                        .map(|t| t.source_text.clone())
                                        .unwrap_or_default();
                                    let target_text = node
                                        .translation
                                        .as_ref()
                                        .and_then(|t| t.target_text.clone())
                                        .unwrap_or_default();
                                    app.textarea = TextArea::new(
                                        target_text.lines().map(String::from).collect(),
                                    );
                                    app.textarea.set_placeholder_text(source_text);
                                    app.textarea.set_block(
                                        Block::default()
                                            .borders(Borders::ALL)
                                            .title("Edit Terjemahan (Tekan Esc untuk keluar)")
                                            .style(Style::default().fg(Color::LightYellow)),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            AppMode::Editing => {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Esc => {
                            app.mode = AppMode::Normal;
                            if let Some((path, _)) =
                                app.visible_nodes.get(app.selected_index).cloned()
                            {
                                let new_text = app.textarea.lines().join("\n");
                                let is_translated = !new_text.is_empty();

                                if let Some(item) = app.translation_store.all_items.get_mut(&path) {
                                    item.target_text = if is_translated {
                                        Some(new_text.clone())
                                    } else {
                                        None
                                    };
                                }
                                if let Some(node) = app.get_node_mut(&path) {
                                    if let Some(trans_item) = &mut node.translation {
                                        trans_item.target_text =
                                            if is_translated { Some(new_text) } else { None };
                                    }
                                }
                                App::update_node_translation_status(&mut app.tree);
                            }
                            app.textarea.set_block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Edit Terjemahan"),
                            );
                        }
                        KeyCode::Char('q')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            app.mode = AppMode::Normal;
                            if let Some((path, _)) =
                                app.visible_nodes.get(app.selected_index).cloned()
                            {
                                let new_text = app.textarea.lines().join("\n");
                                let is_translated = !new_text.is_empty();

                                if let Some(item) = app.translation_store.all_items.get_mut(&path) {
                                    item.target_text = if is_translated {
                                        Some(new_text.clone())
                                    } else {
                                        None
                                    };
                                }
                                if let Some(node) = app.get_node_mut(&path) {
                                    if let Some(trans_item) = &mut node.translation {
                                        trans_item.target_text =
                                            if is_translated { Some(new_text) } else { None };
                                    }
                                }
                                App::update_node_translation_status(&mut app.tree);
                            }
                            app.textarea.set_block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Edit Terjemahan"),
                            );
                        }
                        _ => {
                            app.textarea.input(key);
                        }
                    }
                }
            }
        }
    }
}
