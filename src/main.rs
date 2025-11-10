use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use serde::Deserialize;
use serde_json;
use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{self, BufReader},
    path::PathBuf,
    time::Instant,
};
use tui_textarea::TextArea;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// File sumber JSON yang akan diterjemahkan
    source_file: PathBuf,

    /// File output untuk menyimpan hasil terjemahan.
    /// Jika tidak disediakan, akan menggunakan nama file sumber dengan prefix bahasa (misal: id_source.json)
    #[arg(short, long)]
    out: Option<PathBuf>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum JsonValue {
    String(String),
    Object(HashMap<String, JsonValue>),
}

type JsonData = HashMap<String, JsonValue>;

#[derive(Clone, Debug)]
struct TranslationItem {
    key: String,
    source_text: String,
    target_text: Option<String>,
}

impl TranslationItem {
    fn is_translated(&self) -> bool {
        self.target_text.is_some()
    }
}

#[derive(Clone, Debug)]
struct TreeNode {
    key_segment: String,
    full_path: String,
    translation: Option<TranslationItem>,
    children: Vec<TreeNode>,
    expanded: bool,
    fully_translated: bool,
}

impl TreeNode {
    fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

enum AppMode {
    Normal,
    Editing,
}

struct App<'a> {
    tree: Vec<TreeNode>,
    visible_nodes: Vec<(String, usize)>, // (full_path, depth)
    selected_index: usize,
    textarea: TextArea<'a>,
    all_items: HashMap<String, TranslationItem>,
    mode: AppMode,
    output_path: PathBuf,
    status_message: Option<(String, Instant)>,
}

impl<'a> App<'a> {
    fn new(items: Vec<TranslationItem>, output_path: PathBuf) -> App<'a> {
        let all_items = items
            .iter()
            .map(|item| (item.key.clone(), item.clone()))
            .collect();
        let mut tree = App::build_tree(items);
        App::update_node_translation_status(&mut tree);

        let mut app = App {
            tree,
            visible_nodes: Vec::new(),
            selected_index: 0,
            textarea: TextArea::default(),
            all_items,
            mode: AppMode::Normal,
            output_path,
            status_message: None,
        };
        app.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title("Edit Terjemahan"),
        );
        app.update_visible_nodes();
        app
    }

    fn save_translations(&self) -> Result<(), Box<dyn Error>> {
        let json_data = self.unflatten_to_json_value();
        let file = File::create(&self.output_path)?;
        serde_json::to_writer_pretty(file, &json_data)?;
        Ok(())
    }

    fn unflatten_to_json_value(&self) -> serde_json::Value {
        let mut root = serde_json::Value::Object(serde_json::Map::new());

        let mut sorted_keys: Vec<_> = self.all_items.keys().cloned().collect();
        sorted_keys.sort();

        for key in sorted_keys {
            if let Some(item) = self.all_items.get(&key) {
                if let Some(text) = &item.target_text {
                    let mut current = &mut root;
                    let segments: Vec<&str> = key.split('.').collect();
                    for (i, segment) in segments.iter().enumerate() {
                        if i == segments.len() - 1 {
                            if let Some(obj) = current.as_object_mut() {
                                obj.insert(
                                    segment.to_string(),
                                    serde_json::Value::String(text.clone()),
                                );
                            }
                        } else {
                            current = current
                                .as_object_mut()
                                .unwrap()
                                .entry(segment.to_string())
                                .or_insert_with(|| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                        }
                    }
                }
            }
        }
        root
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

                let position = current_level_nodes
                    .iter()
                    .position(|n| n.key_segment == *segment);

                let node_index = match position {
                    Some(pos) => pos,
                    None => {
                        let new_node = TreeNode {
                            key_segment: segment.to_string(),
                            full_path: path_so_far.clone(),
                            translation: None, // Will be set if it's a leaf
                            children: Vec::new(),
                            expanded: false, // Start all nodes collapsed
                            fully_translated: false,
                        };
                        current_level_nodes.push(new_node);
                        current_level_nodes.len() - 1
                    }
                };

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
}

fn load_translation_items(
    source_path: &PathBuf,
    output_path: Option<&PathBuf>,
) -> Result<Vec<TranslationItem>, Box<dyn Error>> {
    // Load source file
    let source_file = File::open(source_path)?;
    let reader = BufReader::new(source_file);
    let source_data: JsonData = serde_json::from_reader(reader)?;

    // Load target file if provided
    let mut target_data: JsonData = HashMap::new();
    if let Some(path) = output_path {
        if path.exists() {
            let target_file = File::open(path)?;
            let target_reader = BufReader::new(target_file);
            target_data = serde_json::from_reader(target_reader)?;
        }
    }

    // Helper function to flatten the nested JsonData
    fn flatten_json(data: &JsonData) -> HashMap<String, String> {
        let mut flat_map = HashMap::new();
        for (key, value) in data {
            flatten_recursive(key, value, &mut flat_map);
        }
        flat_map
    }

    fn flatten_recursive(prefix: &str, value: &JsonValue, flat_map: &mut HashMap<String, String>) {
        match value {
            JsonValue::String(s) => {
                flat_map.insert(prefix.to_string(), s.clone());
            }
            JsonValue::Object(obj) => {
                for (key, inner_value) in obj {
                    let new_prefix = format!("{}.{}", prefix, key);
                    flatten_recursive(&new_prefix, inner_value, flat_map);
                }
            }
        }
    }

    let flat_source_data = flatten_json(&source_data);
    let flat_target_data = flatten_json(&target_data);

    // Create TranslationItems
    let mut items: Vec<TranslationItem> = Vec::new();
    for (key, source_text) in flat_source_data {
        let target_text = flat_target_data.get(&key).cloned();
        items.push(TranslationItem {
            key,
            source_text,
            target_text,
        });
    }

    // Sort items by key for consistent display
    items.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(items)
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
    let items = match load_translation_items(&cli.source_file, cli.out.as_ref()) {
        Ok(items) => items,
        Err(e) => {
            // Restore terminal before printing error
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;
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
    let mut app = App::new(items, output_path);
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error in TUI: {:?}", err)
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
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
                                    let source_text = node.translation.as_ref().map(|t| t.source_text.clone()).unwrap_or_default();
                                    let target_text = node
                                        .translation
                                        .as_ref()
                                        .and_then(|t| t.target_text.clone())
                                        .unwrap_or_default();
                                    app.textarea =
                                        TextArea::new(target_text.lines().map(String::from).collect());
                                    app.textarea.set_placeholder_text(source_text);
                                    app.textarea.set_block(
                                        Block::default()
                                            .borders(Borders::ALL)
                                            .title("Edit Terjemahan (Tekan Esc untuk keluar)")
                                            .style(Style::default().fg(Color::Yellow)),
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

                                if let Some(item) = app.all_items.get_mut(&path) {
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

fn ui(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
        .split(f.area());

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(main_chunks[0]);

    // Panel Kiri: Daftar Kunci (Tree View)
    let list_style = if matches!(app.mode, AppMode::Normal) {
        Style::default()
            .bg(Color::LightGreen)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let items: Vec<ListItem> = app
        .visible_nodes
        .iter()
        .map(|(path, depth)| {
            let node = app.get_node(path).unwrap(); // Should exist
            let is_leaf = node.is_leaf();

            let status_span = if is_leaf {
                if node
                    .translation
                    .as_ref()
                    .map_or(false, |t| t.is_translated())
                {
                    Span::styled("[✓]", Style::default().fg(Color::Green))
                } else {
                    Span::styled("[ ]", Style::default().fg(Color::Gray))
                }
            } else {
                // It's a folder
                if node.fully_translated {
                    Span::styled("[✓]", Style::default().fg(Color::Green))
                } else if node.expanded {
                    Span::styled("[-] ", Style::default().fg(Color::Blue))
                } else {
                    Span::styled("[+] ", Style::default().fg(Color::Blue))
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
    list_state.select(Some(app.selected_index));

    f.render_stateful_widget(items_list, top_chunks[0], &mut list_state);

    // Panel Kanan
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(top_chunks[1]);

    // Panel Kanan Atas: Teks Sumber
    let source_text = if let Some((path, _)) = app.visible_nodes.get(app.selected_index) {
        if let Some(item) = app.all_items.get(path) {
            item.source_text.clone()
        } else {
            "Select a translatable key.".to_string()
        }
    } else {
        String::new()
    };
    let source_paragraph = Paragraph::new(source_text)
        .block(Block::default().borders(Borders::ALL).title("Teks Sumber"));
    f.render_widget(source_paragraph, right_chunks[0]);

    // Panel Kanan Bawah: Area Input
    f.render_widget(&app.textarea, right_chunks[1]);

    // Footer untuk status message
    if let Some((msg, _)) = &app.status_message {
        let footer = Paragraph::new(msg.as_str()).style(Style::default().fg(Color::Yellow));
        f.render_widget(footer, main_chunks[1]);
    }
}
